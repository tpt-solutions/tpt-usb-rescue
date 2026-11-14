//! Zero-copy signature carver with pluggable media validation.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileType {
    Jpeg,
    Png,
    Pdf,
}

pub struct CarvedFile {
    pub id: u64,
    pub file_type: FileType,
    pub start_offset: u64,
    pub end_offset: u64,
    pub size_bytes: u64,
    /// 0.0..=1.0, derived from media validation.
    pub integrity_score: f32,
    pub is_fragmented: bool,
}

/// Validates carved bytes (built-in structural parsers).
pub trait MediaValidator {
    /// Returns an integrity score in 0.0..=1.0.
    fn validate(&self, file_type: FileType, data: &[u8]) -> f32;
}
