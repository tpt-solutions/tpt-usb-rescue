//! Heap buffers with caller-chosen alignment, required for Direct I/O.

use crate::HalError;
use std::alloc::{Layout, alloc_zeroed, dealloc};
use std::ops::{Deref, DerefMut};
use std::ptr::NonNull;

/// A zero-initialised heap buffer aligned to `align` bytes.
///
/// `O_DIRECT` on Linux and `FILE_FLAG_NO_BUFFERING` on Windows both require the
/// user buffer to be aligned to the device's sector size.
pub struct AlignedBuf {
    ptr: NonNull<u8>,
    layout: Layout,
}

impl AlignedBuf {
    /// Allocates `len` zeroed bytes aligned to `align` (must be a power of two).
    pub fn new(len: usize, align: usize) -> Result<Self, HalError> {
        if len == 0 {
            return Err(HalError::InvalidArgument(
                "buffer length must be non-zero".into(),
            ));
        }
        let layout = Layout::from_size_align(len, align)
            .map_err(|e| HalError::InvalidArgument(e.to_string()))?;
        // SAFETY: `layout` has a non-zero size, checked above.
        let raw = unsafe { alloc_zeroed(layout) };
        let ptr = NonNull::new(raw).ok_or(HalError::OutOfMemory)?;
        Ok(Self { ptr, layout })
    }

    /// Alignment in bytes.
    pub fn alignment(&self) -> usize {
        self.layout.align()
    }

    /// Always false; kept so the type satisfies the usual `len`/`is_empty` pairing.
    pub fn is_empty(&self) -> bool {
        self.layout.size() == 0
    }
}

impl Deref for AlignedBuf {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        // SAFETY: `ptr` points to `layout.size()` zero-initialised bytes owned by self.
        unsafe { std::slice::from_raw_parts(self.ptr.as_ptr(), self.layout.size()) }
    }
}

impl DerefMut for AlignedBuf {
    fn deref_mut(&mut self) -> &mut [u8] {
        // SAFETY: `ptr` points to `layout.size()` bytes owned exclusively by self, and
        // `&mut self` guarantees no other reference to them exists.
        unsafe { std::slice::from_raw_parts_mut(self.ptr.as_ptr(), self.layout.size()) }
    }
}

impl Drop for AlignedBuf {
    fn drop(&mut self) {
        // SAFETY: `ptr` was allocated with exactly `self.layout` in `new` and is freed once.
        unsafe { dealloc(self.ptr.as_ptr(), self.layout) }
    }
}

// SAFETY: AlignedBuf uniquely owns its allocation and holds no thread-affine state.
unsafe impl Send for AlignedBuf {}
// SAFETY: Shared access only hands out `&[u8]`, which is safe to share across threads.
unsafe impl Sync for AlignedBuf {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alignment_is_honoured() {
        for align in [512usize, 4096] {
            let buf = AlignedBuf::new(8192, align).unwrap();
            assert_eq!(buf.as_ptr() as usize % align, 0);
            assert_eq!(buf.len(), 8192);
            assert!(buf.iter().all(|&b| b == 0));
        }
    }

    #[test]
    fn rejects_zero_length_and_bad_alignment() {
        assert!(AlignedBuf::new(0, 512).is_err());
        assert!(AlignedBuf::new(512, 3).is_err());
    }
}
