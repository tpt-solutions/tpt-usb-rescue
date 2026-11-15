//! Linux backend: `O_DIRECT | O_SYNC` reads, `BLKSSZGET` / `BLKGETSIZE64` geometry, and
//! `/sys/block` based device lookups.

use crate::{DeviceGeometry, HalError};
use std::fs::{self, File, OpenOptions};
use std::os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt};
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};

/// `BLKGETSIZE64` = `_IOR(0x12, 114, size_t)`. Not exported by every libc version, so it is
/// computed from the kernel's ioctl encoding (direction bits, size of `size_t`, type, number).
/// MIPS, PowerPC and SPARC use a 13-bit size field, so their direction bits sit at bit 29.
#[cfg(any(
    target_arch = "mips",
    target_arch = "mips64",
    target_arch = "powerpc",
    target_arch = "powerpc64",
    target_arch = "sparc",
    target_arch = "sparc64"
))]
const IOC_DIR_SHIFT: u64 = 29;
#[cfg(not(any(
    target_arch = "mips",
    target_arch = "mips64",
    target_arch = "powerpc",
    target_arch = "powerpc64",
    target_arch = "sparc",
    target_arch = "sparc64"
)))]
const IOC_DIR_SHIFT: u64 = 30;
const BLKGETSIZE64: u64 =
    (2u64 << IOC_DIR_SHIFT) | ((std::mem::size_of::<usize>() as u64) << 16) | (0x12 << 8) | 114;

/// Opens a block device read-only with direct I/O and queries its geometry.
pub(crate) fn open_direct(path: &Path) -> Result<(File, DeviceGeometry), HalError> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECT | libc::O_SYNC)
        .open(path)?;

    let mut sector_size: libc::c_int = 0;
    let mut size_bytes: u64 = 0;
    let fd = file.as_raw_fd();
    // SAFETY: `fd` is valid while `file` is alive; BLKSSZGET writes one c_int through the pointer.
    let rc = unsafe {
        libc::ioctl(
            fd,
            libc::BLKSSZGET as _,
            &mut sector_size as *mut libc::c_int,
        )
    };
    if rc != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    // SAFETY: `fd` is valid; BLKGETSIZE64 writes one u64 through the pointer.
    let rc = unsafe { libc::ioctl(fd, BLKGETSIZE64 as _, &mut size_bytes as *mut u64) };
    if rc != 0 {
        return Err(std::io::Error::last_os_error().into());
    }

    let sector_size = u32::try_from(sector_size)
        .ok()
        .filter(|s| *s > 0)
        .ok_or_else(|| HalError::InvalidArgument("device reported zero sector size".into()))?;
    let geometry = DeviceGeometry {
        sector_size,
        sector_count: size_bytes / u64::from(sector_size),
    };
    Ok((file, geometry))
}

/// Returns whether `path` lives on `device` or one of its partitions.
///
/// Both sides are resolved to physical disk names through sysfs, so partitions, LVM, LUKS and
/// md devices all map back to the disk underneath. Filesystems with anonymous device numbers
/// (btrfs, for example) are resolved through the mount source in `/proc/self/mountinfo`.
pub(crate) fn path_on_device(path: &Path, device: &Path) -> Result<bool, HalError> {
    let dev_path = fs::canonicalize(device)?;
    let meta = fs::metadata(&dev_path)?;
    if !meta.file_type().is_block_device() {
        // Not a block device (e.g. an image file); nothing on it lives on a block device.
        return Ok(false);
    }
    let (maj, min) = split_dev(meta.rdev());
    let target_disks = disks_of_devnum(maj, min);
    if target_disks.is_empty() {
        return Err(HalError::InvalidArgument(format!(
            "cannot resolve {} to a physical disk",
            device.display()
        )));
    }

    // The output file may not exist yet; its filesystem is that of the nearest existing ancestor.
    let probe = crate::safety::nearest_existing(path);
    let (pmaj, pmin) = split_dev(fs::metadata(&probe)?.dev());
    let mut backing = Vec::new();
    if pmaj != 0 {
        backing.push((pmaj, pmin));
    } else if let Ok(info) = fs::read_to_string("/proc/self/mountinfo") {
        let canon = fs::canonicalize(&probe).unwrap_or(probe);
        if let Some(src) = crate::mounts::mount_source(&info, &canon) {
            if src.starts_with("/dev/") {
                if let Ok(m) = fs::metadata(fs::canonicalize(&src).unwrap_or_else(|_| src.into())) {
                    backing.push(split_dev(m.rdev()));
                }
            }
        }
    }
    Ok(backing.into_iter().any(|(a, b)| {
        disks_of_devnum(a, b)
            .iter()
            .any(|d| target_disks.contains(d))
    }))
}

/// Names of the physical disks under block device `major:minor`.
fn disks_of_devnum(major: u32, minor: u32) -> Vec<String> {
    let mut out = Vec::new();
    collect_disks(
        Path::new(&format!("/sys/dev/block/{major}:{minor}")),
        &mut out,
        0,
    );
    out
}

fn collect_disks(sys: &Path, out: &mut Vec<String>, depth: u32) {
    if depth > 8 {
        return;
    }
    let Ok(real) = fs::canonicalize(sys) else {
        return;
    };
    // Stacked devices (dm, md, LUKS) list what they sit on under `slaves/`.
    let slaves: Vec<PathBuf> = fs::read_dir(real.join("slaves"))
        .map(|rd| rd.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    if !slaves.is_empty() {
        for s in slaves {
            collect_disks(&s, out, depth + 1);
        }
        return;
    }
    // A partition's parent directory is its disk.
    let disk = if real.join("partition").exists() {
        real.parent()
    } else {
        Some(real.as_path())
    };
    if let Some(name) = disk.and_then(|d| d.file_name()).and_then(|n| n.to_str()) {
        out.push(name.to_owned());
    }
}

/// Lists whole block devices from `/sys/block`.
pub(crate) fn list() -> Result<Vec<crate::devices::DeviceInfo>, HalError> {
    let mut out = Vec::new();
    for entry in fs::read_dir("/sys/block")? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        // Skip virtual devices that never hold a physical USB stick.
        if name.starts_with("loop") || name.starts_with("ram") || name.starts_with("zram") {
            continue;
        }
        let base = entry.path();
        let sectors: u64 = fs::read_to_string(base.join("size"))
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0);
        let removable = fs::read_to_string(base.join("removable"))
            .ok()
            .map(|s| s.trim() == "1");
        out.push(crate::devices::DeviceInfo {
            path: Path::new("/dev").join(&name),
            size_bytes: sectors * 512,
            removable,
        });
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

/// Splits a Linux `dev_t` into (major, minor).
fn split_dev(dev: u64) -> (u32, u32) {
    let major = (((dev >> 8) & 0xfff) | ((dev >> 32) & !0xfff)) as u32;
    let minor = ((dev & 0xff) | ((dev >> 12) & !0xff)) as u32;
    (major, minor)
}

/// Re-enumerates the USB device behind `device` (for example `/dev/sdb`). Needs root. The open
/// handle is invalid afterwards, so the caller should stop and resume later.
pub(crate) fn reset_usb_device(device: &Path) -> Result<(), HalError> {
    let name = device
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| HalError::InvalidArgument("device path has no name".into()))?;
    crate::usb_reset::reset_block_device(Path::new("/sys"), name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_dev_matches_kernel_encoding() {
        // makedev(8, 16) == /dev/sdb
        let dev = (8u64 << 8) | 16;
        assert_eq!(split_dev(dev), (8, 16));
        // A large minor that spills into the high bits (makedev(259, 1000)).
        let major = 259u64;
        let minor = 1000u64;
        let encoded = (minor & 0xff)
            | ((major & 0xfff) << 8)
            | ((minor & !0xff) << 12)
            | ((major & !0xfff) << 32);
        assert_eq!(split_dev(encoded), (259, 1000));
    }
}
