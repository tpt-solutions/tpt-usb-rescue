//! macOS backend: raw `/dev/rdiskN` reads with the page cache disabled (`F_NOCACHE`), geometry via
//! the `DKIOC` ioctls, and `/dev/diskN[sM]` lookups for the safety guard.
//!
//! Only type-checked for macOS in this repository (`cargo check --target aarch64-apple-darwin`);
//! it has not been run on a Mac.

use crate::devices::DeviceInfo;
use crate::{DeviceGeometry, HalError};
use std::fs::{self, File, OpenOptions};
use std::io;
use std::os::unix::fs::MetadataExt;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};

/// `DKIOCGETBLOCKSIZE` = `_IOR('d', 24, uint32_t)`.
const DKIOCGETBLOCKSIZE: u64 = 0x4004_6418;
/// `DKIOCGETBLOCKCOUNT` = `_IOR('d', 25, uint64_t)`.
const DKIOCGETBLOCKCOUNT: u64 = 0x4008_6419;

/// Maps `/dev/diskN` to the raw character node `/dev/rdiskN`, which bypasses the buffer cache.
fn raw_node(path: &Path) -> PathBuf {
    match (path.parent(), path.file_name().and_then(|n| n.to_str())) {
        (Some(dir), Some(name)) if dir == Path::new("/dev") && name.starts_with("disk") => {
            Path::new("/dev").join(format!("r{name}"))
        }
        _ => path.to_path_buf(),
    }
}

/// Opens a disk read-only with caching disabled and queries its geometry.
pub(crate) fn open_direct(path: &Path) -> Result<(File, DeviceGeometry), HalError> {
    let file = OpenOptions::new().read(true).open(raw_node(path))?;
    let fd = file.as_raw_fd();
    // SAFETY: `fd` is valid while `file` lives; F_NOCACHE takes an int flag and changes only
    // this descriptor's caching behaviour.
    if unsafe { libc::fcntl(fd, libc::F_NOCACHE, 1) } == -1 {
        return Err(io::Error::last_os_error().into());
    }
    let mut block_size: u32 = 0;
    let mut block_count: u64 = 0;
    // SAFETY: `fd` is valid; DKIOCGETBLOCKSIZE writes one u32 through the pointer.
    if unsafe { libc::ioctl(fd, DKIOCGETBLOCKSIZE as _, &mut block_size as *mut u32) } == -1 {
        return Err(io::Error::last_os_error().into());
    }
    // SAFETY: `fd` is valid; DKIOCGETBLOCKCOUNT writes one u64 through the pointer.
    if unsafe { libc::ioctl(fd, DKIOCGETBLOCKCOUNT as _, &mut block_count as *mut u64) } == -1 {
        return Err(io::Error::last_os_error().into());
    }
    if block_size == 0 {
        return Err(HalError::InvalidArgument(
            "device reported zero block size".into(),
        ));
    }
    let geometry = DeviceGeometry {
        sector_size: block_size,
        sector_count: block_count,
    };
    Ok((file, geometry))
}

/// Whole disks in `/dev` (`diskN`, not slices such as `diskNsM`).
/// Maps a slice name such as `disk2s1` to its whole disk `disk2`; other names pass through.
fn whole_disk_of(name: &str) -> &str {
    if let Some(rest) = name.strip_prefix("disk") {
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        let tail = &rest[digits..];
        let is_slice = tail
            .strip_prefix('s')
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
        if digits > 0 && is_slice {
            return &name[..4 + digits];
        }
    }
    name
}

fn is_whole_disk(name: &str) -> bool {
    name.strip_prefix("disk")
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// Lists whole disks, sorted by name.
pub(crate) fn list() -> Result<Vec<DeviceInfo>, HalError> {
    let mut out = Vec::new();
    for entry in fs::read_dir("/dev")? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !is_whole_disk(&name) {
            continue;
        }
        let path = Path::new("/dev").join(&name);
        if let Ok((_file, geo)) = open_direct(&path) {
            out.push(DeviceInfo {
                path,
                size_bytes: geo.sector_count * u64::from(geo.sector_size),
                removable: None,
            });
        }
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

/// Returns whether `path` is stored on `device` or one of its slices.
pub(crate) fn path_on_device(path: &Path, device: &Path) -> Result<bool, HalError> {
    let name = match device.file_name().and_then(|n| n.to_str()) {
        Some(n) => whole_disk_of(n.trim_start_matches('r')).to_string(),
        None => return Ok(false),
    };
    if !is_whole_disk(&name) {
        // Not a disk node (for example an image file): nothing can live on it.
        return Ok(false);
    }
    // The output file may not exist yet; its filesystem is that of the nearest existing ancestor.
    let target = fs::metadata(crate::safety::nearest_existing(path))?.dev();

    let mut nodes = vec![Path::new("/dev").join(&name)];
    for entry in fs::read_dir("/dev")? {
        let entry = entry?;
        let fname = entry.file_name().to_string_lossy().into_owned();
        let slice = fname
            .strip_prefix(&name)
            .and_then(|rest| rest.strip_prefix('s'))
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
        if slice {
            nodes.push(entry.path());
        }
    }
    for node in nodes {
        if fs::metadata(&node).is_ok_and(|meta| meta.rdev() == target) {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slices_map_to_their_whole_disk() {
        assert_eq!(whole_disk_of("disk2s1"), "disk2");
        assert_eq!(whole_disk_of("disk12s10"), "disk12");
        assert_eq!(whole_disk_of("disk2"), "disk2");
        assert_eq!(whole_disk_of("notadisk"), "notadisk");
    }

    #[test]
    fn raw_node_mapping() {
        assert_eq!(
            raw_node(Path::new("/dev/disk2")),
            PathBuf::from("/dev/rdisk2")
        );
        assert_eq!(
            raw_node(Path::new("/tmp/x.img")),
            PathBuf::from("/tmp/x.img")
        );
    }

    #[test]
    fn whole_disk_names() {
        assert!(is_whole_disk("disk2"));
        assert!(!is_whole_disk("disk2s1"));
        assert!(!is_whole_disk("disk"));
    }
}
