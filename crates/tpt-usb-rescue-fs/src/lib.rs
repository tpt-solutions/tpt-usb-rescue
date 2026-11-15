//! Raw FAT32 / exFAT parsers operating on byte slices.
//!
//! The parsers read the image directly, so they work even when the OS refuses to mount the
//! volume ("drive needs formatting") or the partition table is damaged. [`detect`] finds volumes
//! by scanning for boot-sector signatures, which does not depend on the partition table at all.
//! NTFS is not implemented yet.

pub mod exfat;
pub mod fat32;
pub mod ntfs;

use std::fmt;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum FsError {
    #[error("not a {0} volume at this offset")]
    NotThisFilesystem(&'static str),
    #[error("volume metadata is inconsistent: {0}")]
    Corrupt(&'static str),
    #[error("offset {0} lies outside the image")]
    OutOfBounds(u64),
    #[error("entry is a directory")]
    IsDirectory,
}

/// One file or directory found on a volume.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    /// Slash-separated path from the root, e.g. `/DOCS/README.TXT`.
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub first_cluster: u32,
    /// Entry was marked deleted but its metadata survives.
    pub deleted: bool,
    /// Data is read as a contiguous cluster run instead of following the FAT. Used for deleted
    /// entries, whose FAT chain has been freed, and for exFAT entries flagged NoFatChain.
    pub contiguous: bool,
}

/// Common read interface for the supported filesystems.
pub trait Filesystem {
    fn kind(&self) -> VolumeKind;
    /// Every entry reachable from the root, including recoverable deleted entries.
    fn entries(&self) -> Vec<FileEntry>;
    /// File contents. The result may be shorter than `size` if clusters fall outside the image.
    fn read(&self, entry: &FileEntry) -> Result<Vec<u8>, FsError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VolumeKind {
    Fat32,
    ExFat,
    Ntfs,
}

impl fmt::Display for VolumeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            VolumeKind::Fat32 => "FAT32",
            VolumeKind::ExFat => "exFAT",
            VolumeKind::Ntfs => "NTFS",
        })
    }
}

/// A volume located by [`detect`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DetectedVolume {
    pub offset: u64,
    pub kind: VolumeKind,
}

/// Opens the volume at `offset`, choosing the parser from the boot sector.
pub fn open(image: &[u8], offset: u64) -> Result<Box<dyn Filesystem + '_>, FsError> {
    let base = usize::try_from(offset).map_err(|_| FsError::OutOfBounds(offset))?;
    match fat32::Fat32::open(image, base) {
        Ok(fs) => return Ok(Box::new(fs)),
        Err(FsError::NotThisFilesystem(_)) => {}
        Err(e) => return Err(e),
    }
    match exfat::ExFat::open(image, base) {
        Ok(fs) => return Ok(Box::new(fs)),
        Err(FsError::NotThisFilesystem(_)) => {}
        Err(e) => return Err(e),
    }
    ntfs::Ntfs::open(image, base).map(|fs| Box::new(fs) as Box<dyn Filesystem + '_>)
}

/// Scans every sector-aligned offset for FAT32, exFAT or NTFS boot sectors.
///
/// This works without a partition table, so it finds volumes behind a corrupted MBR. Hits are
/// returned in offset order. The scan is linear in the image size.
pub fn detect(image: &[u8]) -> Vec<DetectedVolume> {
    let mut found = Vec::new();
    // Offsets of backup boot sectors belonging to volumes already found; they are not volumes.
    let mut backups = std::collections::HashSet::new();
    let mut off = 0usize;
    while off + 512 <= image.len() {
        let sector = &image[off..off + 512];
        if sector[510..512] == [0x55, 0xAA] && !backups.contains(&off) {
            let kind = if &sector[3..11] == b"EXFAT   " {
                Some(VolumeKind::ExFat)
            } else if &sector[3..11] == b"NTFS    " {
                Some(VolumeKind::Ntfs)
            } else if &sector[82..90] == b"FAT32   " {
                Some(VolumeKind::Fat32)
            } else {
                None
            };
            if let Some(kind) = kind {
                // Confirm the full boot sector parses before reporting it.
                let ok = open(image, off as u64).is_ok();
                if ok {
                    if let Some(b) = backup_boot_offset(sector, kind, off) {
                        backups.insert(b);
                    }
                    found.push(DetectedVolume {
                        offset: off as u64,
                        kind,
                    });
                }
            }
        }
        off += 512;
    }
    found
}

/// Where the backup copy of a volume's boot sector lives, as an image offset.
fn backup_boot_offset(sector: &[u8], kind: VolumeKind, base: usize) -> Option<usize> {
    let u16_at = |o: usize| usize::from(u16::from_le_bytes([sector[o], sector[o + 1]]));
    match kind {
        // BPB_BkBootSec, in sectors from the start of the volume.
        VolumeKind::Fat32 => {
            let bps = u16_at(11);
            let bk = u16_at(50);
            (bk > 0).then(|| base + bk * bps)
        }
        // The backup boot region starts 12 sectors in.
        VolumeKind::ExFat => {
            let shift = u32::from(*sector.get(108)?);
            (9..=12).contains(&shift).then(|| base + (12usize << shift))
        }
        // NTFS keeps its copy in the last sector of the volume.
        VolumeKind::Ntfs => {
            let bps = u16_at(11);
            let total = u64::from_le_bytes(sector[40..48].try_into().ok()?);
            let off = usize::try_from(total).ok()?.checked_mul(bps)?;
            base.checked_add(off)
        }
    }
}

/// A partition table entry from the MBR.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MbrPartition {
    pub index: u8,
    pub partition_type: u8,
    pub start_lba: u32,
    pub sectors: u32,
}

/// Reads the four primary MBR entries. Returns an empty list if there is no 0x55AA signature.
pub fn mbr_partitions(image: &[u8]) -> Vec<MbrPartition> {
    let Some(sector) = image.get(..512) else {
        return Vec::new();
    };
    if sector[510..512] != [0x55, 0xAA] {
        return Vec::new();
    }
    (0..4u8)
        .filter_map(|i| {
            let e = &sector[446 + usize::from(i) * 16..446 + usize::from(i) * 16 + 16];
            let partition_type = e[4];
            let start_lba = u32::from_le_bytes([e[8], e[9], e[10], e[11]]);
            let sectors = u32::from_le_bytes([e[12], e[13], e[14], e[15]]);
            (partition_type != 0 && sectors != 0).then_some(MbrPartition {
                index: i,
                partition_type,
                start_lba,
                sectors,
            })
        })
        .collect()
}

/// Little-endian helpers that bounds-check instead of panicking on hostile input.
pub(crate) fn le16(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

pub(crate) fn le32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

pub(crate) fn le64(b: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(b.get(at..at + 8)?.try_into().ok()?))
}

/// Returns `image[start..start+len]` or `OutOfBounds`.
pub(crate) fn slice(image: &[u8], start: usize, len: usize) -> Result<&[u8], FsError> {
    start
        .checked_add(len)
        .and_then(|end| image.get(start..end))
        .ok_or(FsError::OutOfBounds(start as u64))
}

/// Writes `bytes` to `fuzz/seeds/<target>/valid.img`, a starting point for the fuzz targets.
/// Random input almost never forms a valid boot sector, so without seeds the fuzzers spend their
/// time on the "reject" path and never reach directory or run-list parsing.
#[cfg(test)]
pub(crate) fn write_fuzz_seed(target: &str, bytes: &[u8]) {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fuzz/seeds")
        .join(target);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("valid.img"), bytes).unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_and_extracts_volume_behind_corrupted_partition_table() {
        let volume = crate::fat32::test_image::build();
        // Garbage before the volume, with a 0x55AA-terminated sector that is not a valid MBR.
        let mut img = vec![0xA5u8; 40 * 512];
        img[446..510].fill(0xFF);
        img[510] = 0x55;
        img[511] = 0xAA;
        let offset = img.len() as u64;
        img.extend_from_slice(&volume);

        assert_eq!(
            detect(&img),
            vec![DetectedVolume {
                offset,
                kind: VolumeKind::Fat32
            }]
        );
        let fs = open(&img, offset).unwrap();
        let entries = fs.entries();
        let hello = entries.iter().find(|e| e.path == "/HELLO.TXT").unwrap();
        assert_eq!(fs.read(hello).unwrap(), b"hello world");
    }

    #[test]
    fn finds_ntfs_volume_behind_garbage() {
        let volume = crate::ntfs::test_image::build();
        let mut img = vec![0x5Au8; 16 * 512];
        let offset = img.len() as u64;
        img.extend_from_slice(&volume);
        assert_eq!(
            detect(&img),
            vec![DetectedVolume {
                offset,
                kind: VolumeKind::Ntfs
            }]
        );
        let fs = open(&img, offset).unwrap();
        let hello = fs
            .entries()
            .into_iter()
            .find(|e| e.path == "/HELLO.TXT")
            .unwrap();
        assert_eq!(fs.read(&hello).unwrap(), b"hello ntfs");
    }

    #[test]
    fn mbr_entries_are_decoded() {
        let mut img = vec![0u8; 512];
        img[446 + 4] = 0x0C; // FAT32 LBA
        img[446 + 8..446 + 12].copy_from_slice(&2048u32.to_le_bytes());
        img[446 + 12..446 + 16].copy_from_slice(&4096u32.to_le_bytes());
        img[510] = 0x55;
        img[511] = 0xAA;
        let parts = mbr_partitions(&img);
        assert_eq!(
            parts,
            vec![MbrPartition {
                index: 0,
                partition_type: 0x0C,
                start_lba: 2048,
                sectors: 4096
            }]
        );
    }
}
