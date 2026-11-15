//! Linux backend: `O_DIRECT | O_SYNC` reads, `BLKSSZGET` / `BLKGETSIZE64` geometry, and
//! `/sys/block` based device lookups.

use crate::{DeviceGeometry, HalError};
use std::fs::{self, File, OpenOptions};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::os::unix::io::AsRawFd;
use std::path::Path;

/// `BLKGETSIZE64` = `_IOR(0x12, 114, size_t)`. Not exported by every libc version, so it is
/// computed from the kernel's ioctl encoding (direction bits, size of `size_t`, type, number).
const BLKGETSIZE64: u64 =
    (2u64 << 30) | ((std::mem::size_of::<usize>() as u64) << 16) | (0x12 << 8) | 114;

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
pub(crate) fn path_on_device(path: &Path, device: &Path) -> Result<bool, HalError> {
    let dev_path = fs::canonicalize(device)?;
    let name = dev_path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| HalError::InvalidArgument("device path has no name".into()))?
        .to_owned();

    let path_dev = split_dev(fs::metadata(path)?.dev());

    let sys = Path::new("/sys/block").join(&name);
    if !sys.exists() {
        // Not a whole block device (e.g. an image file); nothing on it lives on a block device.
        return Ok(false);
    }

    // The whole disk and each partition appear as /sys/block/<disk>[/<disk><N>]/dev.
    let mut candidates = vec![sys.join("dev")];
    for entry in fs::read_dir(&sys)? {
        let entry = entry?;
        let fname = entry.file_name();
        let fname = fname.to_string_lossy();
        if fname.starts_with(&name) && entry.path().join("dev").exists() {
            candidates.push(entry.path().join("dev"));
        }
    }
    for dev_file in candidates {
        let text = fs::read_to_string(dev_file)?;
        if parse_major_minor(text.trim()) == Some(path_dev) {
            return Ok(true);
        }
    }
    Ok(false)
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

fn parse_major_minor(s: &str) -> Option<(u32, u32)> {
    let (a, b) = s.split_once(':')?;
    Some((a.parse().ok()?, b.parse().ok()?))
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

/// Re-enumerates the USB device behind `device` (for example `/dev/sdb`). Needs root. The open
/// handle is invalid afterwards, so the caller should stop and resume later.
pub(crate) fn reset_usb_device(device: &Path) -> Result<(), HalError> {
    let name = device
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| HalError::InvalidArgument("device path has no name".into()))?;
    crate::usb_reset::reset_block_device(Path::new("/sys"), name)
}
