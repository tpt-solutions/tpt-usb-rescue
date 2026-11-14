//! Binary, append-only mapfile.
//!
//! Layout (little-endian):
//!
//! ```text
//! header (24 bytes): magic "TPTMAP01" | sector_size u32 | sector_count u64 | crc32 u32
//! record (20 bytes): start_sector u64 | length_sectors u32 | status u8 | reserved [u8;3] | crc32 u32
//! ```
//!
//! Every record carries its own CRC over the preceding 16 bytes. Records are only ever appended,
//! and later records override earlier ones for the sectors they cover. On open the log is replayed;
//! a torn or corrupt tail (for example after a power cut mid-write) is detected by CRC or length and
//! truncated away, so the next append starts from a consistent state.

use crate::{BlockRegion, BlockStatus};
use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

pub const MAGIC: &[u8; 8] = b"TPTMAP01";
pub const HEADER_LEN: usize = 24;
pub const RECORD_LEN: usize = 20;

#[derive(Debug, thiserror::Error)]
pub enum MapfileError {
    #[error("I/O error on mapfile: {0}")]
    Io(#[from] std::io::Error),
    #[error("{path} is not a tpt-usb-rescue mapfile (bad header)")]
    BadHeader { path: PathBuf },
    #[error(
        "mapfile was created for a different device: sector size {found_size}/count {found_count}, expected {expected_size}/{expected_count}"
    )]
    DeviceMismatch {
        found_size: u32,
        found_count: u64,
        expected_size: u32,
        expected_count: u64,
    },
    #[error("sector range {start}+{len} is outside the device ({count} sectors)")]
    OutOfRange { start: u64, len: u64, count: u64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Run {
    len: u64,
    status: BlockStatus,
}

/// In-memory view of a mapfile backed by its append-only log.
pub struct Mapfile {
    path: PathBuf,
    file: File,
    sector_size: u32,
    sector_count: u64,
    /// Non-overlapping runs keyed by start sector. Sectors not covered are `Untested`.
    runs: BTreeMap<u64, Run>,
    /// Count of records replayed on open; useful for diagnostics.
    replayed_records: u64,
}

impl Mapfile {
    /// Opens the mapfile at `path`, creating it if absent.
    ///
    /// An existing file must match `sector_size` and `sector_count`, otherwise the map would
    /// describe a different disk.
    pub fn open_or_create(
        path: &Path,
        sector_size: u32,
        sector_count: u64,
    ) -> Result<Self, MapfileError> {
        let exists = path.exists();
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;

        if !exists || file.metadata()?.len() == 0 {
            let mut header = Vec::with_capacity(HEADER_LEN);
            header.extend_from_slice(MAGIC);
            header.extend_from_slice(&sector_size.to_le_bytes());
            header.extend_from_slice(&sector_count.to_le_bytes());
            let crc = crc32fast::hash(&header);
            header.extend_from_slice(&crc.to_le_bytes());
            file.seek(SeekFrom::Start(0))?;
            file.set_len(0)?;
            file.write_all(&header)?;
            file.sync_all()?;
            return Ok(Self {
                path: path.to_path_buf(),
                file,
                sector_size,
                sector_count,
                runs: BTreeMap::new(),
                replayed_records: 0,
            });
        }

        let mut bytes = Vec::new();
        file.seek(SeekFrom::Start(0))?;
        file.read_to_end(&mut bytes)?;
        if bytes.len() < HEADER_LEN
            || &bytes[..8] != MAGIC
            || crc32fast::hash(&bytes[..20]) != le_u32(&bytes[20..24])
        {
            return Err(MapfileError::BadHeader {
                path: path.to_path_buf(),
            });
        }
        let found_size = le_u32(&bytes[8..12]);
        let found_count = le_u64(&bytes[12..20]);
        if found_size != sector_size || found_count != sector_count {
            return Err(MapfileError::DeviceMismatch {
                found_size,
                found_count,
                expected_size: sector_size,
                expected_count: sector_count,
            });
        }

        let mut runs = BTreeMap::new();
        let mut replayed = 0u64;
        let mut good_len = HEADER_LEN;
        for rec in bytes[HEADER_LEN..].chunks(RECORD_LEN) {
            if rec.len() < RECORD_LEN {
                break; // torn tail
            }
            let Some((start, len, status)) = decode_record(rec) else {
                break; // corrupt record: everything after it is untrusted
            };
            apply(&mut runs, start, len, status);
            replayed += 1;
            good_len += RECORD_LEN;
        }
        if good_len != bytes.len() {
            // Drop the torn/corrupt tail so new appends follow a valid record.
            file.set_len(good_len as u64)?;
            file.sync_all()?;
        }
        file.seek(SeekFrom::End(0))?;

        Ok(Self {
            path: path.to_path_buf(),
            file,
            sector_size,
            sector_count,
            runs,
            replayed_records: replayed,
        })
    }

    /// Opens an existing mapfile, taking the device geometry from its header.
    pub fn open_existing(path: &Path) -> Result<Self, MapfileError> {
        let mut head = [0u8; HEADER_LEN];
        let mut f = File::open(path)?;
        if f.read(&mut head)? < HEADER_LEN
            || &head[..8] != MAGIC
            || crc32fast::hash(&head[..20]) != le_u32(&head[20..24])
        {
            return Err(MapfileError::BadHeader {
                path: path.to_path_buf(),
            });
        }
        Self::open_or_create(path, le_u32(&head[8..12]), le_u64(&head[12..20]))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn sector_size(&self) -> u32 {
        self.sector_size
    }

    pub fn sector_count(&self) -> u64 {
        self.sector_count
    }

    pub fn replayed_records(&self) -> u64 {
        self.replayed_records
    }

    /// Marks `len` sectors starting at `start` with `status`, appending a durable record.
    ///
    /// The record is written but not synced; call [`Mapfile::sync`] at checkpoints.
    pub fn set(&mut self, start: u64, len: u64, status: BlockStatus) -> Result<(), MapfileError> {
        if len == 0 {
            return Ok(());
        }
        let end = start.checked_add(len).filter(|e| *e <= self.sector_count);
        let Some(_) = end else {
            return Err(MapfileError::OutOfRange {
                start,
                len,
                count: self.sector_count,
            });
        };
        // Split the in-memory map in the same way replay will, so both stay identical.
        let len_u32 = len.min(u64::from(u32::MAX)) as u32;
        let mut remaining = u64::from(len_u32);
        let mut cursor = start;
        while remaining > 0 {
            let chunk = remaining.min(u64::from(u32::MAX)) as u32;
            let rec = encode_record(cursor, chunk, status);
            self.file.write_all(&rec)?;
            apply(&mut self.runs, cursor, u64::from(chunk), status);
            cursor += u64::from(chunk);
            remaining -= u64::from(chunk);
        }
        Ok(())
    }

    /// Forces appended records to stable storage.
    pub fn sync(&mut self) -> Result<(), MapfileError> {
        self.file.sync_data()?;
        Ok(())
    }

    /// Status of a single sector.
    pub fn status_of(&self, sector: u64) -> BlockStatus {
        match self.runs.range(..=sector).next_back() {
            Some((&s, run)) if sector < s + run.len => run.status,
            _ => BlockStatus::Untested,
        }
    }

    /// Full coverage of the device as regions in ascending order, gaps reported as `Untested`.
    pub fn regions(&self) -> Vec<BlockRegion> {
        let mut out: Vec<BlockRegion> = Vec::new();
        let mut cursor = 0u64;
        for (&start, run) in &self.runs {
            if start > cursor {
                out.push(BlockRegion {
                    start_sector: cursor,
                    length_sectors: start - cursor,
                    status: BlockStatus::Untested,
                });
            }
            push_merged(
                &mut out,
                BlockRegion {
                    start_sector: start,
                    length_sectors: run.len,
                    status: run.status,
                },
            );
            cursor = start + run.len;
        }
        if cursor < self.sector_count {
            out.push(BlockRegion {
                start_sector: cursor,
                length_sectors: self.sector_count - cursor,
                status: BlockStatus::Untested,
            });
        }
        out
    }

    /// Regions with the given status, merged where contiguous.
    pub fn regions_with(&self, status: BlockStatus) -> Vec<BlockRegion> {
        self.regions()
            .into_iter()
            .filter(|r| r.status == status)
            .collect()
    }

    /// Total sectors in each status.
    pub fn totals(&self) -> Totals {
        let mut t = Totals::default();
        for r in self.regions() {
            let bytes = r.length_sectors * u64::from(self.sector_size);
            match r.status {
                BlockStatus::Untested => t.untested += bytes,
                BlockStatus::Recovered => t.recovered += bytes,
                BlockStatus::BadSector => t.bad += bytes,
                BlockStatus::NonTrimmed => t.non_trimmed += bytes,
            }
        }
        t
    }

    /// Renders the map in GNU ddrescue format (byte offsets, hex).
    pub fn to_ddrescue(&self) -> String {
        let ss = u64::from(self.sector_size);
        let mut s = String::from(
            "# Mapfile. Created by tpt-usb-rescue\n# current_pos  current_status  current_pass\n",
        );
        s.push_str("0x00000000     ?               1\n");
        s.push_str("#      pos        size  status\n");
        for r in self.regions() {
            s.push_str(&format!(
                "0x{:08X}  0x{:08X}  {}\n",
                r.start_sector * ss,
                r.length_sectors * ss,
                r.status.ddrescue_char()
            ));
        }
        s
    }

    /// Renders the map as TOML `[[region]]` tables.
    pub fn to_toml(&self) -> String {
        let mut s = format!(
            "# tpt-usb-rescue mapfile export\nsector_size = {}\nsector_count = {}\n",
            self.sector_size, self.sector_count
        );
        for r in self.regions() {
            let name = match r.status {
                BlockStatus::Untested => "untested",
                BlockStatus::Recovered => "recovered",
                BlockStatus::BadSector => "bad",
                BlockStatus::NonTrimmed => "non_trimmed",
            };
            s.push_str(&format!(
                "\n[[region]]\nstart_sector = {}\nlength_sectors = {}\nstatus = \"{}\"\n",
                r.start_sector, r.length_sectors, name
            ));
        }
        s
    }
}

/// Byte totals per status.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Totals {
    pub recovered: u64,
    pub bad: u64,
    pub non_trimmed: u64,
    pub untested: u64,
}

fn apply(runs: &mut BTreeMap<u64, Run>, start: u64, len: u64, status: BlockStatus) {
    let end = start + len;
    let mut found: Vec<(u64, Run)> = Vec::new();
    if let Some((&s, run)) = runs.range(..start).next_back() {
        if s + run.len > start {
            found.push((s, *run));
        }
    }
    for (&s, run) in runs.range(start..end) {
        found.push((s, *run));
    }
    for (s, run) in found {
        runs.remove(&s);
        if s < start {
            runs.insert(
                s,
                Run {
                    len: start - s,
                    status: run.status,
                },
            );
        }
        let run_end = s + run.len;
        if run_end > end {
            runs.insert(
                end,
                Run {
                    len: run_end - end,
                    status: run.status,
                },
            );
        }
    }
    if status != BlockStatus::Untested {
        runs.insert(start, Run { len, status });
    }
}

fn push_merged(out: &mut Vec<BlockRegion>, r: BlockRegion) {
    if let Some(last) = out.last_mut() {
        if last.status == r.status && last.start_sector + last.length_sectors == r.start_sector {
            last.length_sectors += r.length_sectors;
            return;
        }
    }
    out.push(r);
}

fn encode_record(start: u64, len: u32, status: BlockStatus) -> [u8; RECORD_LEN] {
    let mut rec = [0u8; RECORD_LEN];
    rec[0..8].copy_from_slice(&start.to_le_bytes());
    rec[8..12].copy_from_slice(&len.to_le_bytes());
    rec[12] = status.code();
    let crc = crc32fast::hash(&rec[..16]);
    rec[16..20].copy_from_slice(&crc.to_le_bytes());
    rec
}

fn decode_record(rec: &[u8]) -> Option<(u64, u64, BlockStatus)> {
    if rec.len() != RECORD_LEN || crc32fast::hash(&rec[..16]) != le_u32(&rec[16..20]) {
        return None;
    }
    let start = le_u64(&rec[0..8]);
    let len = u64::from(le_u32(&rec[8..12]));
    let status = BlockStatus::from_code(rec[12])?;
    Some((start, len, status))
}

fn le_u32(b: &[u8]) -> u32 {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

fn le_u64(b: &[u8]) -> u64 {
    let mut a = [0u8; 8];
    a.copy_from_slice(&b[..8]);
    u64::from_le_bytes(a)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join("tpt-mapfile-tests");
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join(name);
        let _ = std::fs::remove_file(&p);
        p
    }

    #[test]
    fn round_trip_and_replay() {
        let p = temp("roundtrip.map");
        {
            let mut m = Mapfile::open_or_create(&p, 512, 100).unwrap();
            m.set(0, 10, BlockStatus::Recovered).unwrap();
            m.set(10, 5, BlockStatus::NonTrimmed).unwrap();
            m.set(12, 2, BlockStatus::BadSector).unwrap();
            m.sync().unwrap();
        }
        let m = Mapfile::open_or_create(&p, 512, 100).unwrap();
        assert_eq!(m.replayed_records(), 3);
        assert_eq!(m.status_of(0), BlockStatus::Recovered);
        assert_eq!(m.status_of(11), BlockStatus::NonTrimmed);
        assert_eq!(m.status_of(12), BlockStatus::BadSector);
        assert_eq!(m.status_of(14), BlockStatus::NonTrimmed);
        assert_eq!(m.status_of(15), BlockStatus::Untested);
    }

    #[test]
    fn regions_cover_whole_device() {
        let p = temp("cover.map");
        let mut m = Mapfile::open_or_create(&p, 512, 50).unwrap();
        m.set(5, 5, BlockStatus::Recovered).unwrap();
        m.set(20, 10, BlockStatus::BadSector).unwrap();
        let regions = m.regions();
        let total: u64 = regions.iter().map(|r| r.length_sectors).sum();
        assert_eq!(total, 50);
        assert_eq!(
            regions[0],
            BlockRegion {
                start_sector: 0,
                length_sectors: 5,
                status: BlockStatus::Untested
            }
        );
        assert_eq!(m.totals().recovered, 5 * 512);
        assert_eq!(m.totals().bad, 10 * 512);
    }

    #[test]
    fn torn_tail_is_truncated() {
        let p = temp("torn.map");
        {
            let mut m = Mapfile::open_or_create(&p, 512, 100).unwrap();
            m.set(0, 4, BlockStatus::Recovered).unwrap();
            m.sync().unwrap();
        }
        // Simulate a crash mid-append: half a record of garbage.
        {
            let mut f = OpenOptions::new().append(true).open(&p).unwrap();
            f.write_all(&[0xAB; 11]).unwrap();
        }
        let mut m = Mapfile::open_or_create(&p, 512, 100).unwrap();
        assert_eq!(m.replayed_records(), 1);
        assert_eq!(m.status_of(0), BlockStatus::Recovered);
        m.set(4, 4, BlockStatus::BadSector).unwrap();
        m.sync().unwrap();
        drop(m);
        let m = Mapfile::open_or_create(&p, 512, 100).unwrap();
        assert_eq!(m.replayed_records(), 2);
        assert_eq!(m.status_of(5), BlockStatus::BadSector);
    }

    #[test]
    fn corrupt_record_stops_replay() {
        let p = temp("corrupt.map");
        {
            let mut m = Mapfile::open_or_create(&p, 512, 100).unwrap();
            m.set(0, 4, BlockStatus::Recovered).unwrap();
            m.set(4, 4, BlockStatus::Recovered).unwrap();
            m.sync().unwrap();
        }
        let mut bytes = std::fs::read(&p).unwrap();
        bytes[HEADER_LEN + RECORD_LEN + 2] ^= 0xFF; // flip a bit in the second record
        std::fs::write(&p, &bytes).unwrap();
        let m = Mapfile::open_or_create(&p, 512, 100).unwrap();
        assert_eq!(m.replayed_records(), 1);
        assert_eq!(m.status_of(4), BlockStatus::Untested);
    }

    #[test]
    fn rejects_mismatched_device() {
        let p = temp("mismatch.map");
        Mapfile::open_or_create(&p, 512, 100).unwrap();
        assert!(matches!(
            Mapfile::open_or_create(&p, 512, 200),
            Err(MapfileError::DeviceMismatch { .. })
        ));
    }

    #[test]
    fn ddrescue_export_uses_byte_offsets() {
        let p = temp("ddr.map");
        let mut m = Mapfile::open_or_create(&p, 512, 4).unwrap();
        m.set(1, 2, BlockStatus::BadSector).unwrap();
        let text = m.to_ddrescue();
        assert!(text.contains("0x00000200  0x00000400  -"), "{text}");
    }

    #[test]
    fn overwriting_splits_runs_correctly() {
        let mut runs = BTreeMap::new();
        apply(&mut runs, 0, 10, BlockStatus::Recovered);
        apply(&mut runs, 3, 2, BlockStatus::BadSector);
        apply(&mut runs, 9, 5, BlockStatus::NonTrimmed);
        let mut seen = Vec::new();
        for (&s, r) in &runs {
            seen.push((s, r.len, r.status));
        }
        assert_eq!(
            seen,
            vec![
                (0, 3, BlockStatus::Recovered),
                (3, 2, BlockStatus::BadSector),
                (5, 4, BlockStatus::Recovered),
                (9, 5, BlockStatus::NonTrimmed),
            ]
        );
    }
}

#[cfg(test)]
mod property_tests {
    use super::*;
    use proptest::prelude::*;

    const COUNT: u64 = 64;

    fn status_strategy() -> impl Strategy<Value = BlockStatus> {
        prop_oneof![
            Just(BlockStatus::Untested),
            Just(BlockStatus::Recovered),
            Just(BlockStatus::BadSector),
            Just(BlockStatus::NonTrimmed),
        ]
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(128))]

        /// Any sequence of writes must match a naive per-sector model, both live and after replay.
        #[test]
        fn replay_matches_naive_model(
            ops in proptest::collection::vec((0..COUNT, 1..16u64, status_strategy()), 0..40)
        ) {
            let dir = std::env::temp_dir().join("tpt-mapfile-proptest");
            std::fs::create_dir_all(&dir).unwrap();
            let path = dir.join(format!("p{}.map", nanos_suffix()));
            let _ = std::fs::remove_file(&path);

            let mut model = vec![BlockStatus::Untested; COUNT as usize];
            {
                let mut m = Mapfile::open_or_create(&path, 512, COUNT).unwrap();
                for (start, len, status) in &ops {
                    let len = (*len).min(COUNT - start);
                    m.set(*start, len, *status).unwrap();
                    for s in *start..start + len {
                        model[s as usize] = *status;
                    }
                }
                m.sync().unwrap();
                for s in 0..COUNT {
                    prop_assert_eq!(m.status_of(s), model[s as usize]);
                }
            }
            let m = Mapfile::open_or_create(&path, 512, COUNT).unwrap();
            for s in 0..COUNT {
                prop_assert_eq!(m.status_of(s), model[s as usize]);
            }
            let covered: u64 = m.regions().iter().map(|r| r.length_sectors).sum();
            prop_assert_eq!(covered, COUNT);
        }
    }

    fn nanos_suffix() -> u64 {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0)
    }
}
