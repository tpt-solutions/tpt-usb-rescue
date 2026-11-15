//! Guards that stop the imager from writing to, or imaging, the wrong disk.
//!
//! The imager only ever reads from the source device. These checks protect against the two
//! ways a user can still destroy data: pointing the output file at the source disk, and imaging
//! the disk the operating system is running from.

use crate::HalError;
use std::path::{Path, PathBuf};

/// Refuses an output path that is a device node or lives on the device being imaged.
pub fn check_output(device: &Path, output: &Path) -> Result<(), HalError> {
    let out_str = output.to_string_lossy();
    if out_str.starts_with(r"\\.\") || out_str.starts_with("/dev/") {
        return Err(HalError::Refused(format!(
            "output {} is a device node; write images to a regular file",
            output.display()
        )));
    }
    if same_path(device, output) {
        return Err(HalError::Refused("output path is the source device".into()));
    }
    if path_on_device(output, device)? {
        return Err(HalError::Refused(format!(
            "output {} is on the device being imaged ({})",
            output.display(),
            device.display()
        )));
    }
    Ok(())
}

/// Refuses to image the disk that hosts the running operating system unless `allow` is set.
pub fn check_not_system_disk(device: &Path, allow: bool) -> Result<(), HalError> {
    if allow {
        return Ok(());
    }
    let root = system_root();
    if path_on_device(&root, device)? {
        return Err(HalError::Refused(format!(
            "{} hosts the running system ({}); pass --allow-system-disk to override",
            device.display(),
            root.display()
        )));
    }
    Ok(())
}

/// The path whose device is considered the system disk.
fn system_root() -> PathBuf {
    #[cfg(windows)]
    {
        let drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
        PathBuf::from(format!("{drive}\\"))
    }
    #[cfg(not(windows))]
    {
        PathBuf::from("/")
    }
}

/// Returns `path` if it exists, otherwise its nearest existing ancestor.
///
/// An output image usually doesn't exist yet, but the filesystem it will be created on does.
#[cfg(unix)]
pub(crate) fn nearest_existing(path: &Path) -> PathBuf {
    let mut current = path.to_path_buf();
    loop {
        if current.exists() {
            return current;
        }
        match current.parent() {
            Some(p) if p.as_os_str().is_empty() => return PathBuf::from("."),
            Some(p) => current = p.to_path_buf(),
            None => return PathBuf::from("."),
        }
    }
}

fn same_path(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => a == b,
    }
}

/// Returns true if `path` is stored on `device` (or one of its partitions).
///
/// Unknown answers are reported as errors rather than `false`, so callers can refuse.
pub fn path_on_device(path: &Path, device: &Path) -> Result<bool, HalError> {
    #[cfg(target_os = "linux")]
    {
        crate::linux::path_on_device(path, device)
    }
    #[cfg(target_os = "macos")]
    {
        crate::macos::path_on_device(path, device)
    }
    #[cfg(windows)]
    {
        crate::windows::path_on_device(path, device)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        let _ = (path, device);
        Err(HalError::Unsupported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_device_node_outputs() {
        let dev = Path::new("/dev/sdb");
        assert!(check_output(dev, Path::new(r"\\.\PhysicalDrive3")).is_err());
        assert!(check_output(dev, Path::new("/dev/sdc")).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn nearest_existing_walks_up_for_new_files() {
        let tmp = std::env::temp_dir();
        let missing = tmp.join("tpt-no-such-dir-xyz").join("sub").join("out.img");
        assert_eq!(nearest_existing(&missing), tmp);
        assert_eq!(
            nearest_existing(Path::new("recovered-does-not-exist.img")),
            PathBuf::from(".")
        );
    }
}
