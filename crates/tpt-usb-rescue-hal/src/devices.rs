//! Device enumeration shared by the CLI's `list-devices` command.

use crate::HalError;
use std::path::PathBuf;

/// A block device that can be imaged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    pub path: PathBuf,
    pub size_bytes: u64,
    /// `None` when the platform does not report removability.
    pub removable: Option<bool>,
}

/// Lists physical block devices visible to this process.
///
/// Opening physical devices usually needs administrator or root rights; devices that cannot be
/// opened are silently omitted.
pub fn list_devices() -> Result<Vec<DeviceInfo>, HalError> {
    #[cfg(target_os = "linux")]
    {
        crate::linux::list()
    }
    #[cfg(windows)]
    {
        crate::windows::list()
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        Err(HalError::Unsupported)
    }
}
