//! Three-pass rescue engine.
//!
//! | Pass | Works on            | Action                                                          |
//! |------|---------------------|-----------------------------------------------------------------|
//! | 1    | `Untested`          | Sequential sweep in large chunks. Failed chunks → `NonTrimmed`. |
//! | 2    | `NonTrimmed`        | Retry at 4 KiB, then per sector. Failed sectors → `BadSector`.  |
//! | 3    | `BadSector` edges   | Trim inward from each end of a bad run to the exact boundary.   |
//!
//! Every state change is checkpointed: image data is synced before the mapfile records it as
//! recovered, so a kill at any point leaves a map that never claims data that is not on disk.
//! Device removal stops the run cleanly with the progress saved.

use crate::BlockStatus;
use crate::mapfile::{Mapfile, MapfileError, Totals};
use crate::output::OutputImage;
use std::io;
use tpt_usb_rescue_hal::{AlignedBuf, DeviceGeometry, HalError, RawDevice};

/// Windows and Linux error codes meaning the device has gone away.
/// Linux: ENXIO (6), ENODEV (19). Windows: ERROR_NO_SUCH_DEVICE (433), ERROR_DEVICE_NOT_CONNECTED (1167).
const REMOVED_CODES: &[i32] = &[6, 19, 433, 1167];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    /// Read failed because of the medium; keep going.
    Media,
    /// The device disappeared; stop and keep progress.
    Removed,
    /// Programming or permission error; stop without touching the map.
    Fatal,
}

/// Decides how a failed HAL call should be handled.
pub fn classify(err: &HalError) -> Fault {
    match err {
        HalError::Io(e) => {
            if e.raw_os_error().is_some_and(|c| REMOVED_CODES.contains(&c)) {
                Fault::Removed
            } else if matches!(
                e.kind(),
                io::ErrorKind::PermissionDenied
                    | io::ErrorKind::InvalidInput
                    | io::ErrorKind::NotFound
            ) {
                Fault::Fatal
            } else {
                Fault::Media
            }
        }
        HalError::Misaligned { .. }
        | HalError::InvalidArgument(_)
        | HalError::OutOfMemory
        | HalError::Refused(_)
        | HalError::Unsupported => Fault::Fatal,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EngineConfig {
    /// Pass-1 chunk size in bytes (default 1 MiB).
    pub chunk_bytes: u64,
    /// Extra attempts per read in pass 2 (default 2).
    pub pass2_retries: u32,
    /// Extra attempts per edge sector in pass 3 (default 4).
    pub trim_retries: u32,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            chunk_bytes: 1 << 20,
            pass2_retries: 2,
            trim_retries: 4,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error(transparent)]
    Hal(#[from] HalError),
    #[error(transparent)]
    Mapfile(#[from] MapfileError),
    #[error("image write failed: {0}")]
    Output(io::Error),
    #[error("device removed at sector {sector}; progress saved, re-run with --resume to continue")]
    DeviceRemoved { sector: u64 },
    #[error("fatal error at sector {sector}: {source}")]
    Fatal { sector: u64, source: HalError },
}

enum Read {
    Ok { bytes: usize },
    Media,
}

pub struct Engine<'a, D: RawDevice> {
    dev: &'a D,
    geo: DeviceGeometry,
    map: Mapfile,
    out: OutputImage,
    buf: AlignedBuf,
    cfg: EngineConfig,
}

impl<'a, D: RawDevice> Engine<'a, D> {
    pub fn new(
        dev: &'a D,
        map: Mapfile,
        out: OutputImage,
        cfg: EngineConfig,
    ) -> Result<Self, EngineError> {
        let geo = dev.geometry()?;
        if geo.sector_size == 0
            || geo.sector_size != map.sector_size()
            || geo.sector_count != map.sector_count()
        {
            return Err(EngineError::Fatal {
                sector: 0,
                source: HalError::InvalidArgument(format!(
                    "mapfile geometry ({}/{}) does not match device ({}/{})",
                    map.sector_size(),
                    map.sector_count(),
                    geo.sector_size,
                    geo.sector_count
                )),
            });
        }
        let align = (geo.sector_size as usize).max(4096);
        let sector = u64::from(geo.sector_size);
        let len = (cfg.chunk_bytes.max(sector) / sector * sector).max(sector) as usize;
        let buf = AlignedBuf::new(len.max(align), align)?;
        Ok(Self {
            dev,
            geo,
            map,
            out,
            buf,
            cfg,
        })
    }

    pub fn mapfile(&self) -> &Mapfile {
        &self.map
    }

    pub fn totals(&self) -> Totals {
        self.map.totals()
    }

    /// Runs one pass (1, 2 or 3). On device removal or a fatal error the state is checkpointed first.
    pub fn run_pass(&mut self, pass: u8) -> Result<Totals, EngineError> {
        let result = match pass {
            1 => self.pass1(),
            2 => self.pass2(),
            3 => self.pass3(),
            other => Err(EngineError::Hal(HalError::InvalidArgument(format!(
                "no pass {other}"
            )))),
        };
        if let Err(e) = result {
            let _ = self.checkpoint();
            return Err(e);
        }
        self.checkpoint()?;
        Ok(self.map.totals())
    }

    /// Syncs image data before mapfile records, so the map never runs ahead of the disk.
    pub fn checkpoint(&mut self) -> Result<(), EngineError> {
        self.out.sync().map_err(EngineError::Output)?;
        self.map.sync()?;
        Ok(())
    }

    /// Finalises the image length and syncs everything. Consumes the engine.
    pub fn finish(self) -> Result<Mapfile, EngineError> {
        self.out.finish().map_err(EngineError::Output)?;
        let mut map = self.map;
        map.sync()?;
        Ok(map)
    }

    fn pass1(&mut self) -> Result<(), EngineError> {
        let chunk = (self.cfg.chunk_bytes / u64::from(self.geo.sector_size)).max(1);
        for region in self.map.regions_with(BlockStatus::Untested) {
            let end = region.start_sector + region.length_sectors;
            let mut s = region.start_sector;
            while s < end {
                let n = chunk.min(end - s);
                match self.read_block(s, n, 0)? {
                    Read::Ok { bytes } => self.commit(s, bytes, n)?,
                    Read::Media => self.map.set(s, n, BlockStatus::NonTrimmed)?,
                }
                self.checkpoint()?;
                s += n;
            }
        }
        Ok(())
    }

    fn pass2(&mut self) -> Result<(), EngineError> {
        let ss = u64::from(self.geo.sector_size);
        // 4 KiB granularity, but never coarser than one sector.
        let big = (4096 / ss).max(1);
        for region in self.map.regions_with(BlockStatus::NonTrimmed) {
            let end = region.start_sector + region.length_sectors;
            let mut s = region.start_sector;
            while s < end {
                let n = big.min(end - s);
                match self.read_block(s, n, self.cfg.pass2_retries)? {
                    Read::Ok { bytes } => self.commit(s, bytes, n)?,
                    Read::Media if n > 1 => {
                        for sector in s..s + n {
                            match self.read_block(sector, 1, self.cfg.pass2_retries)? {
                                Read::Ok { bytes } => self.commit(sector, bytes, 1)?,
                                Read::Media => self.map.set(sector, 1, BlockStatus::BadSector)?,
                            }
                            self.checkpoint()?;
                        }
                        s += n;
                        continue;
                    }
                    Read::Media => self.map.set(s, n, BlockStatus::BadSector)?,
                }
                self.checkpoint()?;
                s += n;
            }
        }
        Ok(())
    }

    fn pass3(&mut self) -> Result<(), EngineError> {
        for region in self.map.regions_with(BlockStatus::BadSector) {
            let mut lo = region.start_sector;
            let mut hi = region.start_sector + region.length_sectors;
            // Trim from the front until the first sector that stays unreadable.
            while lo < hi {
                match self.read_block(lo, 1, self.cfg.trim_retries)? {
                    Read::Ok { bytes } => self.commit(lo, bytes, 1)?,
                    Read::Media => break,
                }
                self.checkpoint()?;
                lo += 1;
            }
            // Trim from the back the same way.
            while hi > lo {
                match self.read_block(hi - 1, 1, self.cfg.trim_retries)? {
                    Read::Ok { bytes } => self.commit(hi - 1, bytes, 1)?,
                    Read::Media => break,
                }
                self.checkpoint()?;
                hi -= 1;
            }
        }
        Ok(())
    }

    /// Reads `n` sectors at `start` into the working buffer, retrying media faults.
    fn read_block(&mut self, start: u64, n: u64, retries: u32) -> Result<Read, EngineError> {
        let ss = u64::from(self.geo.sector_size);
        let offset = start * ss;
        let want = (n * ss) as usize;
        let mut attempt = 0;
        loop {
            match self.dev.read_at(offset, &mut self.buf[..want]) {
                Ok(0) => return Ok(Read::Media),
                Ok(bytes) => return Ok(Read::Ok { bytes }),
                Err(err) => match classify(&err) {
                    Fault::Removed => return Err(EngineError::DeviceRemoved { sector: start }),
                    Fault::Fatal => {
                        return Err(EngineError::Fatal {
                            sector: start,
                            source: err,
                        });
                    }
                    Fault::Media => {
                        if attempt >= retries {
                            return Ok(Read::Media);
                        }
                        attempt += 1;
                    }
                },
            }
        }
    }

    /// Writes the first `bytes` of the working buffer for sectors starting at `start`, and records
    /// the sectors they cover. `requested` is the sector count that was asked for.
    fn commit(&mut self, start: u64, bytes: usize, requested: u64) -> Result<(), EngineError> {
        let ss = u64::from(self.geo.sector_size);
        let covered = (bytes as u64).div_ceil(ss).min(requested);
        self.out
            .write_block(start * ss, &self.buf[..bytes])
            .map_err(EngineError::Output)?;
        self.map.set(start, covered, BlockStatus::Recovered)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::collections::HashSet;
    use std::path::PathBuf;

    /// In-memory device with injectable bad sectors and an optional disconnect.
    struct MockDev {
        data: Vec<u8>,
        ss: u32,
        bad: HashSet<u64>,
        reads: Cell<u64>,
        disconnect_after_reads: Option<u64>,
    }

    impl RawDevice for MockDev {
        fn geometry(&self) -> Result<DeviceGeometry, HalError> {
            Ok(DeviceGeometry {
                sector_size: self.ss,
                sector_count: self.data.len() as u64 / u64::from(self.ss),
            })
        }

        fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<usize, HalError> {
            let n = self.reads.get() + 1;
            self.reads.set(n);
            if self.disconnect_after_reads.is_some_and(|limit| n > limit) {
                return Err(io::Error::from_raw_os_error(19).into()); // ENODEV
            }
            let ss = u64::from(self.ss);
            let first = offset / ss;
            let last = (offset + buf.len() as u64).div_ceil(ss);
            if (first..last).any(|s| self.bad.contains(&s)) {
                return Err(io::Error::other("simulated media error").into());
            }
            let start = offset as usize;
            if start >= self.data.len() {
                return Ok(0);
            }
            let end = (start + buf.len()).min(self.data.len());
            buf[..end - start].copy_from_slice(&self.data[start..end]);
            Ok(end - start)
        }
    }

    fn tmp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join("tpt-engine-tests");
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join(name);
        let _ = std::fs::remove_file(&p);
        p
    }

    fn pattern(sectors: usize, ss: usize) -> Vec<u8> {
        (0..sectors * ss)
            .map(|i| ((i * 31 + 7) % 251) as u8 + 1)
            .collect()
    }

    fn cfg_small() -> EngineConfig {
        EngineConfig {
            chunk_bytes: 8 * 512,
            pass2_retries: 1,
            trim_retries: 2,
        }
    }

    #[test]
    fn clean_device_images_exactly() {
        let data = pattern(200, 512);
        let dev = MockDev {
            data: data.clone(),
            ss: 512,
            bad: HashSet::new(),
            reads: Cell::new(0),
            disconnect_after_reads: None,
        };
        let map = Mapfile::open_or_create(&tmp("clean.map"), 512, 200).unwrap();
        let img_path = tmp("clean.img");
        let out = OutputImage::create(&img_path, 200 * 512).unwrap();
        let mut eng = Engine::new(&dev, map, out, cfg_small()).unwrap();
        for p in 1..=3 {
            eng.run_pass(p).unwrap();
        }
        let t = eng.totals();
        assert_eq!(t.recovered, 200 * 512);
        assert_eq!(t.bad + t.non_trimmed + t.untested, 0);
        eng.finish().unwrap();
        assert_eq!(std::fs::read(&img_path).unwrap(), data);
    }

    #[test]
    fn bad_sectors_are_isolated_and_zero_filled() {
        let data = pattern(200, 512);
        let bad: HashSet<u64> = [10u64, 11, 150].into_iter().collect();
        let dev = MockDev {
            data: data.clone(),
            ss: 512,
            bad: bad.clone(),
            reads: Cell::new(0),
            disconnect_after_reads: None,
        };
        let map = Mapfile::open_or_create(&tmp("bad.map"), 512, 200).unwrap();
        let img_path = tmp("bad.img");
        let out = OutputImage::create(&img_path, 200 * 512).unwrap();
        let mut eng = Engine::new(&dev, map, out, cfg_small()).unwrap();
        for p in 1..=3 {
            eng.run_pass(p).unwrap();
        }
        let t = eng.totals();
        assert_eq!(t.bad, 3 * 512, "exactly the three bad sectors remain bad");
        assert_eq!(t.recovered, 197 * 512);
        assert_eq!(t.untested, 0);
        eng.finish().unwrap();

        let img = std::fs::read(&img_path).unwrap();
        assert_eq!(img.len(), data.len(), "byte alignment preserved");
        for s in 0..200usize {
            let got = &img[s * 512..(s + 1) * 512];
            if bad.contains(&(s as u64)) {
                assert!(
                    got.iter().all(|&b| b == 0),
                    "bad sector {s} must be zero-filled"
                );
            } else {
                assert_eq!(got, &data[s * 512..(s + 1) * 512], "sector {s}");
            }
        }
    }

    #[test]
    fn disconnect_then_resume_completes_image() {
        let data = pattern(120, 512);
        let bad: HashSet<u64> = [40u64].into_iter().collect();
        let map_path = tmp("resume.map");
        let img_path = tmp("resume.img");

        // First run: the device vanishes partway through pass 1.
        {
            let dev = MockDev {
                data: data.clone(),
                ss: 512,
                bad: bad.clone(),
                reads: Cell::new(0),
                disconnect_after_reads: Some(5),
            };
            let map = Mapfile::open_or_create(&map_path, 512, 120).unwrap();
            let out = OutputImage::create(&img_path, 120 * 512).unwrap();
            let mut eng = Engine::new(&dev, map, out, cfg_small()).unwrap();
            let err = eng.run_pass(1).unwrap_err();
            assert!(matches!(err, EngineError::DeviceRemoved { .. }), "{err}");
        }

        // Resume with a healthy device: map and image carry on from where they stopped.
        let dev = MockDev {
            data: data.clone(),
            ss: 512,
            bad,
            reads: Cell::new(0),
            disconnect_after_reads: None,
        };
        let map = Mapfile::open_or_create(&map_path, 512, 120).unwrap();
        assert!(
            map.totals().recovered > 0,
            "progress before the disconnect was saved"
        );
        let out = OutputImage::open_resume(&img_path, 120 * 512).unwrap();
        let mut eng = Engine::new(&dev, map, out, cfg_small()).unwrap();
        for p in 1..=3 {
            eng.run_pass(p).unwrap();
        }
        assert_eq!(eng.totals().bad, 512);
        eng.finish().unwrap();
        let img = std::fs::read(&img_path).unwrap();
        assert_eq!(&img[..40 * 512], &data[..40 * 512]);
        assert!(img[40 * 512..41 * 512].iter().all(|&b| b == 0));
        assert_eq!(&img[41 * 512..], &data[41 * 512..]);
    }

    #[test]
    fn classify_maps_codes_as_expected() {
        assert_eq!(classify(&io::Error::other("media").into()), Fault::Media);
        assert_eq!(
            classify(&io::Error::from_raw_os_error(19).into()),
            Fault::Removed
        );
        assert_eq!(
            classify(&io::Error::from_raw_os_error(433).into()),
            Fault::Removed
        );
        assert_eq!(
            classify(&io::Error::from(io::ErrorKind::PermissionDenied).into()),
            Fault::Fatal
        );
    }
}
