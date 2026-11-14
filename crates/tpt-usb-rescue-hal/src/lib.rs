//! Hardware abstraction layer: raw, cache-bypassing block device access.
//!
//! Platform backends: Linux (`O_DIRECT`), Windows (`FILE_FLAG_NO_BUFFERING`),
//! macOS (raw `/dev/rdiskX`, post-1.0).

/// Errors returned by the HAL.
#[derive(Debug, thiserror::Error)]
pub enum HalError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("unsupported platform")]
    Unsupported,
}

/// Geometry of a block device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceGeometry {
    pub sector_size: u32,
    pub sector_count: u64,
}

/// A read-only raw block device (TODO: implement per platform).
pub trait RawDevice {
    fn geometry(&self) -> Result<DeviceGeometry, HalError>;
    /// Read `buf.len()` bytes at `offset`. `buf` and `offset` must be sector-aligned.
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<usize, HalError>;
}
