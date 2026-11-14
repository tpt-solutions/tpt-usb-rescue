//! Multi-pass imager and append-only binary mapfile.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockRegion {
    pub start_sector: u64,
    pub length_sectors: u32,
    pub status: BlockStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockStatus {
    Untested,
    Recovered,
    BadSector,
    /// Edge of a bad block.
    NonTrimmed,
}
