//! Hardware abstraction layer: raw, cache-bypassing block device access.
//!
//! Platform backends: Linux (`O_DIRECT`), Windows (`FILE_FLAG_NO_BUFFERING`),
//! macOS (raw `/dev/rdiskX`, post-1.0; currently reports [`HalError::Unsupported`]).
//!
//! Every handle is opened read-only. Nothing in this crate writes to a device.

pub mod buf;
pub mod devices;
pub mod safety;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

use std::fs::File;
use std::io;
use std::path::Path;

pub use buf::AlignedBuf;
pub use devices::{DeviceInfo, list_devices};
pub use safety::{check_not_system_disk, check_output, path_on_device};

/// Errors returned by the HAL.
#[derive(Debug, thiserror::Error)]
pub enum HalError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("unsupported platform")]
    Unsupported,
    #[error("invalid argument: {0}")]
    InvalidArgument(String),
    #[error("out of memory allocating aligned buffer")]
    OutOfMemory,
    #[error("offset {offset} / length {len} is not aligned to sector size {sector}")]
    Misaligned {
        offset: u64,
        len: usize,
        sector: u32,
    },
    #[error("refused: {0}")]
    Refused(String),
}

/// Geometry of a block device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceGeometry {
    pub sector_size: u32,
    pub sector_count: u64,
}

/// A read-only raw block device.
pub trait RawDevice {
    fn geometry(&self) -> Result<DeviceGeometry, HalError>;
    /// Reads up to `buf.len()` bytes at `offset`, returning the number read (0 at end of device).
    ///
    /// For direct-I/O devices `offset`, `buf.len()` and the buffer address must all be
    /// sector-aligned; otherwise [`HalError::Misaligned`] is returned.
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<usize, HalError>;
}

/// A read-only handle backed by a file, either a regular image or an OS device opened
/// with direct I/O.
pub struct FileDevice {
    file: File,
    geometry: DeviceGeometry,
    direct: bool,
}

impl RawDevice for FileDevice {
    fn geometry(&self) -> Result<DeviceGeometry, HalError> {
        Ok(self.geometry)
    }

    fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<usize, HalError> {
        if self.direct {
            let sector = u64::from(self.geometry.sector_size);
            let addr_aligned = (buf.as_ptr() as usize as u64) % sector == 0;
            if offset % sector != 0 || buf.len() as u64 % sector != 0 || !addr_aligned {
                return Err(HalError::Misaligned {
                    offset,
                    len: buf.len(),
                    sector: self.geometry.sector_size,
                });
            }
        }
        Ok(pread(&self.file, buf, offset)?)
    }
}

#[cfg(unix)]
fn pread(file: &File, buf: &mut [u8], offset: u64) -> io::Result<usize> {
    std::os::unix::fs::FileExt::read_at(file, buf, offset)
}

#[cfg(windows)]
fn pread(file: &File, buf: &mut [u8], offset: u64) -> io::Result<usize> {
    std::os::windows::fs::FileExt::seek_read(file, buf, offset)
}

/// Opens a regular image file read-only. Geometry uses 512-byte sectors.
pub fn open_image(path: &Path) -> Result<FileDevice, HalError> {
    let file = File::open(path)?;
    let len = file.metadata()?.len();
    let sector_size = 512;
    Ok(FileDevice {
        file,
        geometry: DeviceGeometry {
            sector_size,
            sector_count: len.div_ceil(u64::from(sector_size)),
        },
        direct: false,
    })
}

/// Opens a physical device (`/dev/sdX`, `\\.\PhysicalDriveN`) read-only with direct I/O.
pub fn open_device(path: &Path) -> Result<FileDevice, HalError> {
    #[cfg(target_os = "linux")]
    {
        let (file, geometry) = linux::open_direct(path)?;
        Ok(FileDevice {
            file,
            geometry,
            direct: true,
        })
    }
    #[cfg(target_os = "macos")]
    {
        let (file, geometry) = macos::open_direct(path)?;
        Ok(FileDevice {
            file,
            geometry,
            direct: true,
        })
    }
    #[cfg(windows)]
    {
        let (file, geometry) = windows::open_direct(path)?;
        Ok(FileDevice {
            file,
            geometry,
            direct: true,
        })
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        let _ = path;
        Err(HalError::Unsupported)
    }
}
