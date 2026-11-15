//! NTFS parser: boot sector, MFT records with update-sequence fixups, attributes, run lists.
//!
//! The directory tree is rebuilt from `$FILE_NAME` parent references across the whole MFT, rather
//! than by walking index blocks. That also finds files whose directory index is damaged. Deleted
//! records (MFT entries not marked in-use) are listed with `deleted = true`, and their data is read
//! through the run list that the record still carries.
//!
//! For NTFS entries, [`FileEntry::first_cluster`] holds the MFT record number, because NTFS data is
//! not a single cluster chain. `contiguous` is always `false`.

use crate::{FileEntry, Filesystem, FsError, VolumeKind, le16, le32, le64, slice};
use std::collections::HashMap;

/// Longest sparse hole that `read_runs` will expand into zeros.
const MAX_SPARSE_FILL: usize = 256 << 20;
const RECORD_SIGNATURE: &[u8; 4] = b"FILE";
const ATTR_FILE_NAME: u32 = 0x30;
const ATTR_DATA: u32 = 0x80;
const ATTR_END: u32 = 0xFFFF_FFFF;
const FIRST_USER_RECORD: u64 = 24;
const ROOT_RECORD: u64 = 5;
const MAX_PATH_DEPTH: usize = 64;

/// One run of an extent: `lcn == None` means a sparse (zero-filled) run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Run {
    lcn: Option<u64>,
    clusters: u64,
}

pub struct Ntfs<'a> {
    img: &'a [u8],
    base: usize,
    cluster_size: usize,
    record_size: usize,
    /// Runs of the $MFT file itself, which locate every other record.
    mft_runs: Vec<Run>,
    /// Records parsed once when the volume is opened: record number -> parsed info.
    records: HashMap<u64, Record>,
}

#[derive(Debug, Clone)]
struct Record {
    number: u64,
    in_use: bool,
    is_dir: bool,
    name: String,
    parent: u64,
    /// Data attribute: (real size, resident bytes or run list).
    data: Option<DataAttr>,
}

#[derive(Debug, Clone)]
enum DataAttr {
    Resident(Vec<u8>),
    NonResident { real_size: u64, runs: Vec<Run> },
}

impl<'a> Ntfs<'a> {
    /// Parses the boot sector at `base`, then every MFT record reachable from the $MFT runs.
    pub fn open(img: &'a [u8], base: usize) -> Result<Self, FsError> {
        let b = slice(img, base, 512)?;
        if &b[3..11] != b"NTFS    " || b[510..512] != [0x55, 0xAA] {
            return Err(FsError::NotThisFilesystem("NTFS"));
        }
        let corrupt = |what| FsError::Corrupt(what);
        let bytes_per_sector = le16(b, 11).ok_or(corrupt("boot sector"))? as usize;
        let sectors_per_cluster = b[13] as usize;
        if !matches!(bytes_per_sector, 512 | 1024 | 2048 | 4096) {
            return Err(corrupt("bytes per sector"));
        }
        if sectors_per_cluster == 0 || !sectors_per_cluster.is_power_of_two() {
            return Err(corrupt("sectors per cluster"));
        }
        let cluster_size = bytes_per_sector * sectors_per_cluster;
        let mft_cluster = le64(b, 48).ok_or(corrupt("MFT location"))?;
        let clusters_per_record = b[64] as i8;
        let record_size = if clusters_per_record > 0 {
            clusters_per_record as usize * cluster_size
        } else {
            1usize
                .checked_shl((-(clusters_per_record as i32)) as u32)
                .ok_or(corrupt("record size"))?
        };
        if !(512..=65536).contains(&record_size) || !record_size.is_power_of_two() {
            return Err(corrupt("record size"));
        }

        let mut fs = Ntfs {
            img,
            base,
            cluster_size,
            record_size,
            mft_runs: Vec::new(),
            records: HashMap::new(),
        };
        let mft_start = mft_cluster
            .checked_mul(cluster_size as u64)
            .ok_or(corrupt("MFT location"))? as usize;
        let mft0 = fs
            .read_record_at(base + mft_start)
            .ok_or(corrupt("$MFT record"))?;
        let mft_data = parse_record(0, &mft0)
            .and_then(|r| r.data)
            .ok_or(corrupt("$MFT data"))?;
        fs.mft_runs = match mft_data {
            DataAttr::NonResident { runs, .. } => runs,
            DataAttr::Resident(_) => return Err(corrupt("$MFT not in runs")),
        };
        fs.load_records()?;
        Ok(fs)
    }

    /// Reads and fixes up one record from a byte offset in the image.
    fn read_record_at(&self, offset: usize) -> Option<Vec<u8>> {
        let mut rec = self
            .img
            .get(offset..offset.checked_add(self.record_size)?)?
            .to_vec();
        apply_fixups(&mut rec)?;
        Some(rec)
    }

    /// Reads all MFT records through the $MFT run list.
    fn load_records(&mut self) -> Result<(), FsError> {
        // Corrupt run lists can claim absurd lengths: saturate, and never walk past the image.
        let total_clusters = self
            .mft_runs
            .iter()
            .fold(0u64, |acc, r| acc.saturating_add(r.clusters));
        let total = usize::try_from(total_clusters)
            .unwrap_or(usize::MAX)
            .saturating_mul(self.cluster_size)
            .min(self.img.len().saturating_sub(self.base));
        let per_record = self.record_size;
        let mut number = 0u64;
        let mut virt = 0usize;
        while virt + per_record <= total {
            let runs = self.mft_runs.clone();
            if let Some(phys) = self.virtual_to_physical(&runs, virt, per_record) {
                if let Some(rec) = self.read_record_at(self.base + phys) {
                    if let Some(parsed) = parse_record(number, &rec) {
                        self.records.insert(number, parsed);
                    }
                }
            }
            number += 1;
            virt += per_record;
        }
        Ok(())
    }

    /// Maps a virtual offset inside the $MFT to a physical offset within the image.
    fn virtual_to_physical(&self, runs: &[Run], virt: usize, len: usize) -> Option<usize> {
        let mut remaining = virt as u64;
        for run in runs {
            let run_bytes = run.clusters.saturating_mul(self.cluster_size as u64);
            if remaining < run_bytes {
                let lcn = run.lcn?;
                let phys = lcn
                    .checked_mul(self.cluster_size as u64)?
                    .checked_add(remaining)?;
                // The record must lie inside this run.
                if remaining.checked_add(len as u64)? > run_bytes {
                    return None;
                }
                return usize::try_from(phys).ok();
            }
            remaining -= run_bytes;
        }
        None
    }

    fn path_of(&self, number: u64) -> Option<String> {
        let mut parts = Vec::new();
        let mut cur = number;
        for _ in 0..MAX_PATH_DEPTH {
            if cur == ROOT_RECORD {
                parts.reverse();
                return Some(format!("/{}", parts.join("/")));
            }
            let rec = self.records.get(&cur)?;
            parts.push(rec.name.clone());
            if rec.parent == cur {
                return None; // self-parented: not a real tree
            }
            cur = rec.parent;
        }
        None // too deep, or a cycle
    }

    /// Reads the bytes described by a run list, limited to `size`.
    fn read_runs(&self, runs: &[Run], size: u64) -> Vec<u8> {
        let mut out = Vec::with_capacity((size as usize).min(self.img.len()));
        for run in runs {
            if out.len() as u64 >= size {
                break;
            }
            let want = (size - out.len() as u64)
                .min(run.clusters.saturating_mul(self.cluster_size as u64))
                as usize;
            match run.lcn {
                None => {
                    // Sparse hole: a corrupt size must not make us allocate gigabytes of zeros.
                    if want > MAX_SPARSE_FILL {
                        break;
                    }
                    out.resize(out.len() + want, 0);
                }
                Some(lcn) => {
                    let start = usize::try_from(lcn)
                        .ok()
                        .and_then(|l| l.checked_mul(self.cluster_size))
                        .and_then(|o| o.checked_add(self.base));
                    let Some(start) = start else { break };
                    match start
                        .checked_add(want)
                        .and_then(|end| self.img.get(start..end))
                    {
                        Some(bytes) => out.extend_from_slice(bytes),
                        None => break,
                    }
                }
            }
        }
        out
    }
}

impl Filesystem for Ntfs<'_> {
    fn kind(&self) -> VolumeKind {
        VolumeKind::Ntfs
    }

    fn entries(&self) -> Vec<FileEntry> {
        let mut out: Vec<FileEntry> = self
            .records
            .values()
            .filter(|r| r.number >= FIRST_USER_RECORD)
            .filter_map(|r| {
                let path = self.path_of(r.number)?;
                let size = match &r.data {
                    Some(DataAttr::Resident(bytes)) => bytes.len() as u64,
                    Some(DataAttr::NonResident { real_size, .. }) => *real_size,
                    None => 0,
                };
                Some(FileEntry {
                    path,
                    is_dir: r.is_dir,
                    size: if r.is_dir { 0 } else { size },
                    first_cluster: r.number as u32,
                    deleted: !r.in_use,
                    contiguous: false,
                })
            })
            .collect();
        out.sort_by(|a, b| a.path.cmp(&b.path));
        out
    }

    fn read(&self, e: &FileEntry) -> Result<Vec<u8>, FsError> {
        if e.is_dir {
            return Err(FsError::IsDirectory);
        }
        let rec = self
            .records
            .get(&u64::from(e.first_cluster))
            .ok_or(FsError::Corrupt("record vanished"))?;
        match &rec.data {
            None => Ok(Vec::new()),
            Some(DataAttr::Resident(bytes)) => Ok(bytes.clone()),
            Some(DataAttr::NonResident { real_size, runs }) => Ok(self.read_runs(runs, *real_size)),
        }
    }
}

/// Reverses the update-sequence protection: checks and restores the last two bytes of each sector.
fn apply_fixups(rec: &mut [u8]) -> Option<()> {
    let usa_off = le16(rec, 4)? as usize;
    let usa_count = le16(rec, 6)? as usize;
    if usa_count < 2 || usa_off + usa_count * 2 > rec.len() {
        return None;
    }
    let usn = [rec[usa_off], rec[usa_off + 1]];
    for i in 1..usa_count {
        let end = i * 512 - 2;
        if end + 2 > rec.len() || rec[end..end + 2] != usn {
            return None; // torn write or corruption
        }
        let saved = [rec[usa_off + 2 * i], rec[usa_off + 2 * i + 1]];
        rec[end..end + 2].copy_from_slice(&saved);
    }
    Some(())
}

fn parse_record(number: u64, rec: &[u8]) -> Option<Record> {
    if &rec[0..4] != RECORD_SIGNATURE {
        return None;
    }
    let flags = le16(rec, 22)?;
    let in_use = flags & 0x01 != 0;
    let is_dir = flags & 0x02 != 0;
    let mut p = le16(rec, 20)? as usize;

    let mut best_name: Option<(u8, String, u64)> = None; // (namespace rank, name, parent)
    let mut data = None;
    while p + 8 <= rec.len() {
        let kind = le32(rec, p)?;
        if kind == ATTR_END {
            break;
        }
        let len = le32(rec, p + 4)? as usize;
        if len < 16 || p + len > rec.len() {
            break;
        }
        let attr = &rec[p..p + len];
        let non_resident = attr[8] != 0;
        let name_len = attr[9];
        if kind == ATTR_FILE_NAME && !non_resident {
            if let Some((rank, name, parent)) = parse_file_name(attr) {
                if best_name.as_ref().is_none_or(|(r, _, _)| rank > *r) {
                    best_name = Some((rank, name, parent));
                }
            }
        } else if kind == ATTR_DATA && name_len == 0 && data.is_none() {
            data = parse_data(attr);
        }
        p += len;
    }

    // Records without a usable name (such as $MFT itself when its name is absent) are kept with an
    // empty name and no parent. `entries()` skips them because they cannot be placed in the tree.
    let (name, parent) = best_name.map_or((String::new(), 0), |(_, n, p)| (n, p));
    Some(Record {
        number,
        in_use,
        is_dir,
        name,
        parent,
        data,
    })
}

/// Namespace preference: Win32 (1) and Win32+DOS (3) beat POSIX (0); DOS-only (2) is skipped.
fn parse_file_name(attr: &[u8]) -> Option<(u8, String, u64)> {
    let content_off = le16(attr, 20)? as usize;
    let c = attr.get(content_off..)?;
    let parent = le64(c, 0)? & 0x0000_FFFF_FFFF_FFFF;
    let name_len = *c.get(64)? as usize;
    let namespace = *c.get(65)?;
    if namespace == 2 {
        return None;
    }
    let units: Vec<u16> = c
        .get(66..66 + name_len * 2)?
        .chunks_exact(2)
        .map(|p| u16::from_le_bytes([p[0], p[1]]))
        .collect();
    let rank = match namespace {
        3 => 3,
        1 => 2,
        _ => 1,
    };
    Some((rank, String::from_utf16_lossy(&units), parent))
}

fn parse_data(attr: &[u8]) -> Option<DataAttr> {
    if attr[8] == 0 {
        let len = le32(attr, 16)? as usize;
        let off = le16(attr, 20)? as usize;
        return attr
            .get(off..off + len)
            .map(|b| DataAttr::Resident(b.to_vec()));
    }
    let runs_off = le16(attr, 32)? as usize;
    let real_size = le64(attr, 48)?;
    let runs = decode_runs(attr.get(runs_off..)?)?;
    Some(DataAttr::NonResident { real_size, runs })
}

/// Decodes an NTFS run list. Each entry: a header nibble pair giving the byte widths of the length
/// and the (signed, relative) LCN delta, then the two values; a zero header ends the list.
fn decode_runs(mut rl: &[u8]) -> Option<Vec<Run>> {
    let mut runs = Vec::new();
    let mut lcn: i64 = 0;
    while let Some((&header, rest)) = rl.split_first() {
        if header == 0 {
            return Some(runs);
        }
        let len_bytes = (header & 0x0F) as usize;
        let off_bytes = (header >> 4) as usize;
        if len_bytes == 0 || len_bytes > 8 || off_bytes > 8 || rest.len() < len_bytes + off_bytes {
            return None;
        }
        let clusters = read_unsigned(&rest[..len_bytes]);
        let delta_bytes = &rest[len_bytes..len_bytes + off_bytes];
        rl = &rest[len_bytes + off_bytes..];
        if off_bytes == 0 {
            runs.push(Run {
                lcn: None,
                clusters,
            }); // sparse
        } else {
            lcn = lcn.checked_add(read_signed(delta_bytes))?;
            if lcn < 0 {
                return None;
            }
            runs.push(Run {
                lcn: Some(lcn as u64),
                clusters,
            });
        }
    }
    None // list not terminated: refuse rather than guess
}

fn read_unsigned(b: &[u8]) -> u64 {
    b.iter().rev().fold(0u64, |acc, &x| (acc << 8) | x as u64)
}

fn read_signed(b: &[u8]) -> i64 {
    let v = read_unsigned(b);
    let bits = b.len() * 8;
    if bits == 0 || bits >= 64 {
        return v as i64;
    }
    if v & (1u64 << (bits - 1)) != 0 {
        (v | (!0u64 << bits)) as i64
    } else {
        v as i64
    }
}

#[cfg(test)]
pub(crate) mod test_image {
    //! A 256 KiB NTFS volume: 512-byte sectors and clusters, 1 KiB records, MFT at cluster 8.
    //!
    //! Records: 0 $MFT, 5 root, 24 HELLO.TXT (resident), 25 DOCS/ (dir), 26 DOCS/NOTE.TXT
    //! (non-resident, clusters 100-101, real size 600), 27 OLD.BIN (deleted, resident).

    pub const CLUSTER: usize = 512;
    const RECORD: usize = 1024;
    const MFT_CLUSTER: usize = 8;
    const MFT_RECORDS: usize = 32;

    pub fn build() -> Vec<u8> {
        let mut img = vec![0u8; 512 * 512];
        // Boot sector.
        let b = &mut img[..512];
        b[3..11].copy_from_slice(b"NTFS    ");
        b[11..13].copy_from_slice(&512u16.to_le_bytes());
        b[13] = 1;
        b[40..48].copy_from_slice(&512u64.to_le_bytes());
        b[48..56].copy_from_slice(&(MFT_CLUSTER as u64).to_le_bytes());
        b[56..64].copy_from_slice(&(MFT_CLUSTER as u64 + 40).to_le_bytes());
        b[64] = (-10i8) as u8; // 2^10 = 1024-byte records
        b[510] = 0x55;
        b[511] = 0xAA;

        let mft_base = MFT_CLUSTER * CLUSTER;
        let mut recs: Vec<Vec<u8>> = (0..MFT_RECORDS).map(|_| empty_record(0, 0)).collect();

        // 0: $MFT with a run list covering the whole MFT.
        let mft_len = MFT_RECORDS * RECORD;
        recs[0] = record(
            0,
            0x01,
            &[non_resident_attr(
                0x80,
                &run_list(MFT_CLUSTER as u64, (mft_len / CLUSTER) as u64),
                mft_len as u64,
            )],
        );
        // 5: root directory.
        recs[5] = record(5, 0x03, &[file_name_attr(5, ".")]);
        // 24: HELLO.TXT, resident data.
        recs[24] = record(
            24,
            0x01,
            &[
                file_name_attr(5, "HELLO.TXT"),
                resident_attr(0x80, b"hello ntfs"),
            ],
        );
        // 25: DOCS directory under root.
        recs[25] = record(25, 0x03, &[file_name_attr(5, "DOCS")]);
        // 26: NOTE.TXT under DOCS, non-resident, two clusters starting at cluster 100.
        recs[26] = record(
            26,
            0x01,
            &[
                file_name_attr(25, "NOTE.TXT"),
                non_resident_attr(0x80, &run_list(100, 2), 600),
            ],
        );
        // 27: deleted OLD.BIN under root (not in use).
        recs[27] = record(
            27,
            0x00,
            &[file_name_attr(5, "OLD.BIN"), resident_attr(0x80, b"old")],
        );

        // Clusters 100..102 hold NOTE.TXT.
        let note = &mut img[100 * CLUSTER..100 * CLUSTER + 600];
        note.fill(b'n');

        for (i, rec) in recs.iter().enumerate() {
            let at = mft_base + i * RECORD;
            img[at..at + RECORD].copy_from_slice(rec);
        }
        img
    }

    /// Builds a record: header, attributes, end marker, then applies update-sequence protection.
    fn record(number: u32, flags: u16, attrs: &[Vec<u8>]) -> Vec<u8> {
        let mut r = vec![0u8; RECORD];
        r[0..4].copy_from_slice(b"FILE");
        r[4..6].copy_from_slice(&48u16.to_le_bytes()); // update sequence array offset
        r[6..8].copy_from_slice(&3u16.to_le_bytes()); // 1 USN + 2 sectors
        r[16..18].copy_from_slice(&1u16.to_le_bytes());
        r[20..22].copy_from_slice(&56u16.to_le_bytes()); // first attribute
        r[22..24].copy_from_slice(&flags.to_le_bytes());
        r[44..48].copy_from_slice(&number.to_le_bytes());
        let mut p = 56;
        for a in attrs {
            r[p..p + a.len()].copy_from_slice(a);
            p += a.len();
        }
        r[p..p + 4].copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
        r[24..28].copy_from_slice(&((p + 8) as u32).to_le_bytes());
        r[28..32].copy_from_slice(&(RECORD as u32).to_le_bytes());
        // Update sequence protection: save the last two bytes of each sector, write the USN there.
        let usn = [0x01u8, 0x00];
        r[48..50].copy_from_slice(&usn);
        for (i, end) in [510usize, 1022].iter().enumerate() {
            let saved = [r[*end], r[*end + 1]];
            r[50 + i * 2..52 + i * 2].copy_from_slice(&saved);
            r[*end..*end + 2].copy_from_slice(&usn);
        }
        r
    }

    fn empty_record(number: u32, flags: u16) -> Vec<u8> {
        record(number, flags, &[])
    }

    fn resident_attr(kind: u32, content: &[u8]) -> Vec<u8> {
        let len = 24 + content.len();
        let len = (len + 7) & !7;
        let mut a = vec![0u8; len];
        a[0..4].copy_from_slice(&kind.to_le_bytes());
        a[4..8].copy_from_slice(&(len as u32).to_le_bytes());
        a[8] = 0; // resident
        a[16..20].copy_from_slice(&(content.len() as u32).to_le_bytes());
        a[20..22].copy_from_slice(&24u16.to_le_bytes());
        a[24..24 + content.len()].copy_from_slice(content);
        a
    }

    fn file_name_attr(parent: u64, name: &str) -> Vec<u8> {
        let units: Vec<u16> = name.encode_utf16().collect();
        let mut content = vec![0u8; 66 + units.len() * 2];
        content[0..8].copy_from_slice(&parent.to_le_bytes());
        content[64] = units.len() as u8;
        content[65] = 1; // Win32 namespace
        for (i, u) in units.iter().enumerate() {
            content[66 + i * 2..68 + i * 2].copy_from_slice(&u.to_le_bytes());
        }
        resident_attr(0x30, &content)
    }

    fn non_resident_attr(kind: u32, runs: &[u8], real_size: u64) -> Vec<u8> {
        let mut a = vec![0u8; 64 + runs.len()];
        let len = a.len();
        a[0..4].copy_from_slice(&kind.to_le_bytes());
        a[4..8].copy_from_slice(&(len as u32).to_le_bytes());
        a[8] = 1; // non-resident
        a[24..32].copy_from_slice(&0u64.to_le_bytes()); // last VCN placeholder
        a[32..34].copy_from_slice(&64u16.to_le_bytes()); // run list offset
        a[40..48].copy_from_slice(&real_size.to_le_bytes()); // allocated size
        a[48..56].copy_from_slice(&real_size.to_le_bytes()); // real size
        a[56..64].copy_from_slice(&real_size.to_le_bytes()); // initialised size
        a[64..].copy_from_slice(runs);
        a
    }

    /// One run with a one-byte length and a two-byte signed LCN.
    fn run_list(lcn: u64, clusters: u64) -> Vec<u8> {
        vec![
            0x21,
            clusters as u8,
            (lcn & 0xFF) as u8,
            ((lcn >> 8) & 0xFF) as u8,
            0x00,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ntfs::test_image::build;

    fn fs_of(img: &[u8]) -> Ntfs<'_> {
        Ntfs::open(img, 0).expect("valid NTFS boot sector and MFT")
    }

    #[test]
    fn rebuilds_tree_from_parent_references() {
        let img = build();
        let fs = fs_of(&img);
        let paths: Vec<String> = fs.entries().into_iter().map(|e| e.path).collect();
        assert!(paths.contains(&"/HELLO.TXT".to_string()), "{paths:?}");
        assert!(paths.contains(&"/DOCS".to_string()));
        assert!(paths.contains(&"/DOCS/NOTE.TXT".to_string()), "{paths:?}");
    }

    #[test]
    fn reads_resident_and_run_list_data() {
        let img = build();
        let fs = fs_of(&img);
        let entries = fs.entries();
        let hello = entries.iter().find(|e| e.path == "/HELLO.TXT").unwrap();
        assert_eq!(fs.read(hello).unwrap(), b"hello ntfs");
        let note = entries.iter().find(|e| e.path == "/DOCS/NOTE.TXT").unwrap();
        let data = fs.read(note).unwrap();
        assert_eq!(data.len(), 600);
        assert!(data.iter().all(|&b| b == b'n'));
    }

    #[test]
    fn lists_deleted_records() {
        let img = build();
        let fs = fs_of(&img);
        let entries = fs.entries();
        let old = entries
            .iter()
            .find(|e| e.path == "/OLD.BIN")
            .expect("deleted record listed");
        assert!(old.deleted);
        assert_eq!(fs.read(old).unwrap(), b"old");
    }

    #[test]
    fn run_list_decoding_handles_sparse_and_negative_deltas() {
        // Run 1: 2 clusters at LCN 0x100. Run 2: sparse 3 clusters. Run 3: 1 cluster at LCN 0x100 - 0x40.
        let rl = [0x21, 0x02, 0x00, 0x01, 0x01, 0x03, 0x11, 0x01, 0xC0, 0x00];
        let runs = decode_runs(&rl).unwrap();
        assert_eq!(
            runs[0],
            Run {
                lcn: Some(0x100),
                clusters: 2
            }
        );
        assert_eq!(
            runs[1],
            Run {
                lcn: None,
                clusters: 3
            }
        );
        assert_eq!(
            runs[2],
            Run {
                lcn: Some(0xC0),
                clusters: 1
            }
        );
    }

    #[test]
    fn corrupt_fixup_drops_the_record() {
        let mut img = build();
        // Break the USN at the end of record 24's first sector.
        let at = 8 * 512 + 24 * 1024 + 510;
        img[at] ^= 0xFF;
        let fs = fs_of(&img);
        assert!(fs.entries().iter().all(|e| e.path != "/HELLO.TXT"));
    }
}
