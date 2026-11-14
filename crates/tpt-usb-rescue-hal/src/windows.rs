//! Windows backend: `FILE_FLAG_NO_BUFFERING` reads on `\\.\PhysicalDriveN`, geometry via
//! `DeviceIoControl`, and volume-to-disk mapping for the safety guard.

use crate::devices::DeviceInfo;
use crate::{DeviceGeometry, HalError};
use std::ffi::c_void;
use std::fs::{File, OpenOptions};
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::AsRawHandle;
use std::path::{Path, PathBuf};

const FILE_FLAG_NO_BUFFERING: u32 = 0x2000_0000;
const FILE_SHARE_READ: u32 = 0x1;
const FILE_SHARE_WRITE: u32 = 0x2;
const IOCTL_DISK_GET_DRIVE_GEOMETRY: u32 = 0x0007_0000;
const IOCTL_DISK_GET_LENGTH_INFO: u32 = 0x0007_405C;
const IOCTL_STORAGE_GET_DEVICE_NUMBER: u32 = 0x002D_1080;

#[repr(C)]
#[derive(Default)]
struct DiskGeometry {
    cylinders: i64,
    media_type: u32,
    tracks_per_cylinder: u32,
    sectors_per_track: u32,
    bytes_per_sector: u32,
}

#[repr(C)]
#[derive(Default)]
struct GetLengthInformation {
    length: i64,
}

#[repr(C)]
#[derive(Default)]
struct StorageDeviceNumber {
    device_type: u32,
    device_number: u32,
    partition_number: u32,
}

unsafe extern "system" {
    fn DeviceIoControl(
        device: *mut c_void,
        io_control_code: u32,
        in_buffer: *const c_void,
        in_buffer_size: u32,
        out_buffer: *mut c_void,
        out_buffer_size: u32,
        bytes_returned: *mut u32,
        overlapped: *mut c_void,
    ) -> i32;
}

/// Issues a buffered IOCTL that writes into `out`.
fn ioctl_out<T: Default>(file: &File, code: u32) -> Result<T, HalError> {
    let mut out = T::default();
    let mut returned: u32 = 0;
    // SAFETY: the handle is valid while `file` lives; `out` is a repr(C) value of exactly
    // `size_of::<T>()` bytes that the driver writes into; no input buffer is passed.
    let ok = unsafe {
        DeviceIoControl(
            file.as_raw_handle(),
            code,
            std::ptr::null(),
            0,
            (&raw mut out).cast(),
            std::mem::size_of::<T>() as u32,
            &mut returned,
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(out)
}

/// Opens a physical drive or volume read-only with no OS caching.
pub(crate) fn open_direct(path: &Path) -> Result<(File, DeviceGeometry), HalError> {
    let file = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_NO_BUFFERING)
        .open(path)?;
    let geo: DiskGeometry = ioctl_out(&file, IOCTL_DISK_GET_DRIVE_GEOMETRY)?;
    let len: GetLengthInformation = ioctl_out(&file, IOCTL_DISK_GET_LENGTH_INFO)?;
    if geo.bytes_per_sector == 0 {
        return Err(HalError::InvalidArgument(
            "device reported zero sector size".into(),
        ));
    }
    let sector_size = geo.bytes_per_sector;
    let geometry = DeviceGeometry {
        sector_size,
        sector_count: u64::try_from(len.length).unwrap_or(0) / u64::from(sector_size),
    };
    Ok((file, geometry))
}

/// Returns true if the volume or path lives on the physical drive named by `device`.
pub(crate) fn path_on_device(path: &Path, device: &Path) -> Result<bool, HalError> {
    // Not a physical drive (for example an image file), so nothing can live on it.
    let Some(target) = disk_number_from_device(device) else {
        return Ok(false);
    };
    let absolute = std::path::absolute(path)?;
    let drive = absolute
        .components()
        .find_map(|c| match c {
            std::path::Component::Prefix(p) => Some(p.as_os_str().to_string_lossy().into_owned()),
            _ => None,
        })
        .ok_or_else(|| {
            HalError::InvalidArgument(format!("no drive prefix on {}", path.display()))
        })?;
    // `C:` or `\\?\C:` style prefixes both end in the drive letter.
    let letter = drive
        .trim_end_matches(['\\', '/'])
        .chars()
        .last()
        .filter(|c| c.is_ascii_alphabetic());
    let Some(letter) = letter else {
        return Err(HalError::Unsupported);
    };
    let volume = PathBuf::from(format!(r"\\.\{letter}:"));
    let file = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .open(&volume)?;
    let num: StorageDeviceNumber = ioctl_out(&file, IOCTL_STORAGE_GET_DEVICE_NUMBER)?;
    Ok(num.device_number == target)
}

/// Enumerates `\\.\PhysicalDrive0..31` that can be opened.
pub(crate) fn list() -> Result<Vec<DeviceInfo>, HalError> {
    let mut out = Vec::new();
    for n in 0..32 {
        let path = PathBuf::from(format!(r"\\.\PhysicalDrive{n}"));
        if let Ok((_file, geo)) = open_direct(&path) {
            out.push(DeviceInfo {
                path,
                size_bytes: geo.sector_count * u64::from(geo.sector_size),
                removable: None,
            });
        }
    }
    Ok(out)
}

fn disk_number_from_device(device: &Path) -> Option<u32> {
    let s = device.to_string_lossy();
    let tail = s.rsplit("PhysicalDrive").next()?;
    if tail.len() == s.len() {
        return None;
    }
    tail.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_physical_drive_number() {
        assert_eq!(
            disk_number_from_device(Path::new(r"\.\PhysicalDrive2")),
            Some(2)
        );
        assert_eq!(disk_number_from_device(Path::new("/dev/sdb")), None);
    }
}
