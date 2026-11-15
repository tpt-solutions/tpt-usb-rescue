//! USB device re-enumeration through sysfs.
//!
//! Writing `0` and then `1` to a USB device's `authorized` attribute makes the kernel detach and
//! re-probe the device. That recovers many firmware hangs. The logic works on any directory tree
//! with the same layout, so it is tested against a temporary fake sysfs tree on every platform.
//! Only the Linux backend calls it for real, with `/sys` as the root.

use crate::HalError;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Finds the USB device that a block device sits under, by walking up from its sysfs directory
/// until a directory with both `authorized` and `idVendor` appears.
pub fn usb_device_above(start: &Path) -> Option<PathBuf> {
    let mut dir = Some(start);
    while let Some(d) = dir {
        if d.join("authorized").exists() && d.join("idVendor").exists() {
            return Some(d.to_path_buf());
        }
        dir = d.parent();
    }
    None
}

/// Clears and then sets `authorized` on the USB device above block device `name`.
pub fn reset_block_device(sys_root: &Path, name: &str) -> Result<(), HalError> {
    let block_dir = sys_root.join("block").join(name).join("device");
    let resolved = fs::canonicalize(&block_dir)?;
    let usb = usb_device_above(&resolved).ok_or_else(|| {
        HalError::Io(io::Error::new(
            io::ErrorKind::NotFound,
            "block device is not behind a USB device",
        ))
    })?;
    toggle_authorized(&usb)
}

fn toggle_authorized(usb: &Path) -> Result<(), HalError> {
    fs::write(usb.join("authorized"), "0")?;
    fs::write(usb.join("authorized"), "1")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_tree(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join("tpt-sysfs-tests").join(name);
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn finds_the_usb_device_above_a_block_device() {
        let root = temp_tree("ancestor");
        let usb = root.join("devices/usb1/1-1");
        let block = usb.join("1-1/interface0/host0/target0/block/sdb");
        fs::create_dir_all(&block).unwrap();
        fs::write(usb.join("authorized"), "1").unwrap();
        fs::write(usb.join("idVendor"), "0781").unwrap();
        assert_eq!(usb_device_above(&block), Some(usb));
    }

    #[test]
    fn no_usb_ancestor_means_none() {
        let root = temp_tree("no-usb");
        let block = root.join("devices/pci0000/block/nvme0n1");
        fs::create_dir_all(&block).unwrap();
        assert_eq!(usb_device_above(&block), None);
    }

    #[test]
    fn toggling_leaves_device_authorized() {
        let root = temp_tree("toggle");
        fs::write(root.join("authorized"), "0").unwrap();
        toggle_authorized(&root).unwrap();
        assert_eq!(fs::read_to_string(root.join("authorized")).unwrap(), "1");
    }

    #[cfg(unix)]
    #[test]
    fn reset_block_device_resolves_through_sys_block() {
        let root = temp_tree("reset");
        let usb = root.join("devices/usb1/1-2");
        let real = usb.join("1-2/interface1/host1/block/sdc");
        fs::create_dir_all(&real).unwrap();
        fs::write(usb.join("authorized"), "1").unwrap();
        fs::write(usb.join("idVendor"), "0951").unwrap();
        fs::create_dir_all(root.join("block/sdc")).unwrap();
        std::os::unix::fs::symlink(&real, root.join("block/sdc/device")).unwrap();
        reset_block_device(&root, "sdc").unwrap();
        assert_eq!(fs::read_to_string(usb.join("authorized")).unwrap(), "1");
    }
}
