//! Multi-pass imager and append-only binary mapfile.
//!
//! * [`mapfile`] — crash-safe record of which sectors are recovered, bad or untested.
//! * [`output`] — image writer that skips all-zero blocks so fresh images stay sparse.
//! * [`engine`] — the three-pass rescue state machine driving a [`RawDevice`].
//!
//! [`RawDevice`]: tpt_usb_rescue_hal::RawDevice

pub mod engine;
pub mod mapfile;
pub mod output;

/// A contiguous run of sectors that share a status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockRegion {
    pub start_sector: u64,
    pub length_sectors: u64,
    pub status: BlockStatus,
}

/// State of a run of sectors.
///
/// * `Untested` — never attempted.
/// * `Recovered` — read successfully and written to the image.
/// * `NonTrimmed` — a pass-1 chunk failed; not yet resolved to sector granularity.
/// * `BadSector` — failed at sector granularity (pass 2) and not recovered by trimming.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BlockStatus {
    Untested,
    Recovered,
    BadSector,
    /// Edge of a bad block.
    NonTrimmed,
}

impl BlockStatus {
    /// Single-character code used by GNU ddrescue mapfiles.
    pub fn ddrescue_char(self) -> char {
        match self {
            BlockStatus::Untested => '?',
            BlockStatus::Recovered => '+',
            BlockStatus::BadSector => '-',
            BlockStatus::NonTrimmed => '*',
        }
    }

    pub(crate) fn code(self) -> u8 {
        match self {
            BlockStatus::Untested => 0,
            BlockStatus::Recovered => 1,
            BlockStatus::BadSector => 2,
            BlockStatus::NonTrimmed => 3,
        }
    }

    pub(crate) fn from_code(code: u8) -> Option<Self> {
        Some(match code {
            0 => BlockStatus::Untested,
            1 => BlockStatus::Recovered,
            2 => BlockStatus::BadSector,
            3 => BlockStatus::NonTrimmed,
            _ => return None,
        })
    }
}
