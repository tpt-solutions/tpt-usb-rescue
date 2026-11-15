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
//!
//! Reads run on a reader thread. A read that does not finish within
//! [`EngineConfig::read_timeout`] is treated as a media failure, and the reader is replaced, so a
//! hung device cannot stall the whole run. The stuck thread is left to finish on its own.

use crate::BlockStatus;
use crate::mapfile::{Mapfile, MapfileError, Totals};
use crate::output::OutputImage;
use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;
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
    /// A read that does not complete in this time counts as a media failure (default 5 s).
    pub read_timeout: Duration,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            chunk_bytes: 1 << 20,
            pass2_retries: 2,
            trim_retries: 4,
            read_timeout: Duration::from_secs(5),
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
    #[error("stopped by the user; progress saved")]
    Stopped,
}

enum Read {
    Ok { buf: AlignedBuf, bytes: usize },
    Media,
}

/// Callback invoked with the current totals after each checkpoint.
pub type ProgressHook = Box<dyn FnMut(&Totals)>;

/// Callback invoked when a read times out. Receives the first sector of the stuck read.
pub type ResetHook = Box<dyn FnMut(u64) -> Result<(), String>>;

struct ReadRequest {
    offset: u64,
    len: usize,
    align: usize,
}

type ReadResponse = Result<(AlignedBuf, usize), HalError>;

/// A thread that performs device reads on request, so the caller can time them out.
struct Reader {
    requests: mpsc::Sender<ReadRequest>,
    responses: mpsc::Receiver<ReadResponse>,
}

enum ReaderOutcome {
    Done(ReadResponse),
    TimedOut,
    Stopped,
}

impl Reader {
    fn spawn<D: RawDevice + Send + Sync + 'static>(dev: Arc<D>) -> Self {
        let (req_tx, req_rx) = mpsc::channel::<ReadRequest>();
        let (resp_tx, resp_rx) = mpsc::channel::<ReadResponse>();
        std::thread::spawn(move || {
            for req in req_rx {
                let response = fetch(dev.as_ref(), &req);
                if resp_tx.send(response).is_err() {
                    break; // the engine gave up on this reader
                }
            }
        });
        Reader {
            requests: req_tx,
            responses: resp_rx,
        }
    }

    fn read(&self, req: ReadRequest, timeout: Duration) -> ReaderOutcome {
        if self.requests.send(req).is_err() {
            return ReaderOutcome::Stopped;
        }
        match self.responses.recv_timeout(timeout) {
            Ok(resp) => ReaderOutcome::Done(resp),
            Err(RecvTimeoutError::Timeout) => ReaderOutcome::TimedOut,
            Err(RecvTimeoutError::Disconnected) => ReaderOutcome::Stopped,
        }
    }
}

fn fetch<D: RawDevice>(dev: &D, req: &ReadRequest) -> ReadResponse {
    let mut buf = AlignedBuf::new(req.len, req.align)?;
    let n = dev.read_at(req.offset, &mut buf[..req.len])?;
    Ok((buf, n))
}

pub struct Engine<D: RawDevice + Send + Sync + 'static> {
    dev: Arc<D>,
    reader: Reader,
    geo: DeviceGeometry,
    map: Mapfile,
    out: OutputImage,
    cfg: EngineConfig,
    progress: Option<ProgressHook>,
    reset_hook: Option<ResetHook>,
    last_reset_error: Option<String>,
    cancel: Option<Arc<AtomicBool>>,
}

impl<D: RawDevice + Send + Sync + 'static> Engine<D> {
    pub fn new(
        dev: Arc<D>,
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
        let reader = Reader::spawn(Arc::clone(&dev));
        Ok(Self {
            dev,
            reader,
            geo,
            map,
            out,
            cfg,
            progress: None,
            reset_hook: None,
            last_reset_error: None,
            cancel: None,
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
        if let Some(hook) = self.progress.as_mut() {
            hook(&self.map.totals());
        }
        if self.cancel_requested() {
            return Err(EngineError::Stopped);
        }
        Ok(())
    }

    /// Sets a flag that the user can raise to stop the run. The engine checks it at every
    /// checkpoint, so it stops after the current chunk with progress saved.
    pub fn set_cancel_flag(&mut self, flag: Arc<AtomicBool>) {
        self.cancel = Some(flag);
    }

    fn cancel_requested(&self) -> bool {
        self.cancel
            .as_ref()
            .is_some_and(|f| f.load(Ordering::SeqCst))
    }

    /// Registers a hook called when a read times out, before the reader is replaced. Used to reset
    /// the USB device. The hook receives the sector that timed out.
    pub fn set_reset_hook(&mut self, hook: ResetHook) {
        self.reset_hook = Some(hook);
    }

    /// The last error returned by the reset hook, if any.
    pub fn last_reset_error(&self) -> Option<&str> {
        self.last_reset_error.as_deref()
    }

    /// Registers a callback invoked after every checkpoint with the current totals.
    pub fn set_progress_hook(&mut self, hook: ProgressHook) {
        self.progress = Some(hook);
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
                    Read::Ok { buf, bytes } => self.commit(s, &buf, bytes, n)?,
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
                    Read::Ok { buf, bytes } => self.commit(s, &buf, bytes, n)?,
                    Read::Media if n > 1 => {
                        for sector in s..s + n {
                            match self.read_block(sector, 1, self.cfg.pass2_retries)? {
                                Read::Ok { buf, bytes } => self.commit(sector, &buf, bytes, 1)?,
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
                    Read::Ok { buf, bytes } => self.commit(lo, &buf, bytes, 1)?,
                    Read::Media => break,
                }
                self.checkpoint()?;
                lo += 1;
            }
            // Trim from the back the same way.
            while hi > lo {
                match self.read_block(hi - 1, 1, self.cfg.trim_retries)? {
                    Read::Ok { buf, bytes } => self.commit(hi - 1, &buf, bytes, 1)?,
                    Read::Media => break,
                }
                self.checkpoint()?;
                hi -= 1;
            }
        }
        Ok(())
    }

    /// Reads `n` sectors at `start`, retrying media faults and timeouts.
    fn read_block(&mut self, start: u64, n: u64, retries: u32) -> Result<Read, EngineError> {
        let ss = u64::from(self.geo.sector_size);
        let align = (self.geo.sector_size as usize).max(4096);
        let mut attempt = 0;
        loop {
            let req = ReadRequest {
                offset: start * ss,
                len: (n * ss) as usize,
                align,
            };
            let outcome = self.reader.read(req, self.cfg.read_timeout);
            let err = match outcome {
                ReaderOutcome::Done(Ok((_, 0))) => return Ok(Read::Media),
                ReaderOutcome::Done(Ok((buf, bytes))) => return Ok(Read::Ok { buf, bytes }),
                ReaderOutcome::Done(Err(err)) => err,
                ReaderOutcome::TimedOut => {
                    if let Some(hook) = self.reset_hook.as_mut() {
                        if let Err(e) = hook(start) {
                            self.last_reset_error = Some(e);
                        }
                    }
                    // The read is stuck inside the device. Leave that thread to finish on its
                    // own and continue with a fresh reader.
                    self.reader = Reader::spawn(Arc::clone(&self.dev));
                    HalError::Io(io::Error::new(io::ErrorKind::TimedOut, "read timed out"))
                }
                ReaderOutcome::Stopped => {
                    self.reader = Reader::spawn(Arc::clone(&self.dev));
                    HalError::Io(io::Error::other("reader thread stopped"))
                }
            };
            match classify(&err) {
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
            }
        }
    }

    /// Writes the first `bytes` of `buf` for sectors starting at `start`, and records the sectors
    /// they cover. `requested` is the sector count that was asked for.
    fn commit(
        &mut self,
        start: u64,
        buf: &AlignedBuf,
        bytes: usize,
        requested: u64,
    ) -> Result<(), EngineError> {
        let ss = u64::from(self.geo.sector_size);
        let covered = (bytes as u64).div_ceil(ss).min(requested);
        self.out
            .write_block(start * ss, &buf[..bytes])
            .map_err(EngineError::Output)?;
        self.map.set(start, covered, BlockStatus::Recovered)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::path::PathBuf;
    use std::sync::atomic::AtomicU64;
    use std::time::Instant;

    /// Thread-safe in-memory device with injectable bad sectors, hangs and a disconnect.
    struct MockDev {
        data: Vec<u8>,
        ss: u32,
        bad: HashSet<u64>,
        /// Sectors whose reads block for `hang_for` before failing.
        hang: HashSet<u64>,
        hang_for: Duration,
        reads: AtomicU64,
        disconnect_after_reads: Option<u64>,
    }

    impl MockDev {
        fn new(data: Vec<u8>, bad: HashSet<u64>) -> Self {
            Self {
                data,
                ss: 512,
                bad,
                hang: HashSet::new(),
                hang_for: Duration::from_millis(0),
                reads: AtomicU64::new(0),
                disconnect_after_reads: None,
            }
        }
    }

    impl RawDevice for MockDev {
        fn geometry(&self) -> Result<DeviceGeometry, HalError> {
            Ok(DeviceGeometry {
                sector_size: self.ss,
                sector_count: self.data.len() as u64 / u64::from(self.ss),
            })
        }

        fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<usize, HalError> {
            let n = self.reads.fetch_add(1, Ordering::SeqCst) + 1;
            if self.disconnect_after_reads.is_some_and(|limit| n > limit) {
                return Err(io::Error::from_raw_os_error(19).into()); // ENODEV
            }
            let ss = u64::from(self.ss);
            let first = offset / ss;
            let last = (offset + buf.len() as u64).div_ceil(ss);
            if (first..last).any(|s| self.hang.contains(&s)) {
                std::thread::sleep(self.hang_for);
                return Err(io::Error::other("hung then failed").into());
            }
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
            read_timeout: Duration::from_secs(5),
        }
    }

    #[test]
    fn clean_device_images_exactly() {
        let data = pattern(200, 512);
        let dev = Arc::new(MockDev::new(data.clone(), HashSet::new()));
        let map = Mapfile::open_or_create(&tmp("clean.map"), 512, 200).unwrap();
        let img_path = tmp("clean.img");
        let out = OutputImage::create(&img_path, 200 * 512).unwrap();
        let mut eng = Engine::new(dev, map, out, cfg_small()).unwrap();
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
        let dev = Arc::new(MockDev::new(data.clone(), bad.clone()));
        let map = Mapfile::open_or_create(&tmp("bad.map"), 512, 200).unwrap();
        let img_path = tmp("bad.img");
        let out = OutputImage::create(&img_path, 200 * 512).unwrap();
        let mut eng = Engine::new(dev, map, out, cfg_small()).unwrap();
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
    fn hung_read_times_out_and_the_rest_of_the_disk_is_imaged() {
        let data = pattern(40, 512);
        let mut dev = MockDev::new(data.clone(), HashSet::new());
        dev.hang = [7u64].into_iter().collect();
        dev.hang_for = Duration::from_millis(1500);
        let map = Mapfile::open_or_create(&tmp("hang.map"), 512, 40).unwrap();
        let img_path = tmp("hang.img");
        let out = OutputImage::create(&img_path, 40 * 512).unwrap();
        let cfg = EngineConfig {
            read_timeout: Duration::from_millis(150),
            ..cfg_small()
        };
        let mut eng = Engine::new(Arc::new(dev), map, out, cfg).unwrap();
        let started = Instant::now();
        for p in 1..=3 {
            eng.run_pass(p).unwrap();
        }
        // Each timed-out read costs one timeout, not the device's full hang.
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "{:?}",
            started.elapsed()
        );
        let t = eng.totals();
        assert_eq!(t.bad, 512, "only the hung sector is lost");
        assert_eq!(t.recovered, 39 * 512);
        eng.finish().unwrap();
        let img = std::fs::read(&img_path).unwrap();
        assert!(img[7 * 512..8 * 512].iter().all(|&b| b == 0));
        assert_eq!(&img[..7 * 512], &data[..7 * 512]);
        assert_eq!(&img[8 * 512..], &data[8 * 512..]);
    }

    #[test]
    fn disconnect_then_resume_completes_image() {
        let data = pattern(120, 512);
        let bad: HashSet<u64> = [40u64].into_iter().collect();
        let map_path = tmp("resume.map");
        let img_path = tmp("resume.img");

        // First run: the device vanishes partway through pass 1.
        {
            let mut dev = MockDev::new(data.clone(), bad.clone());
            dev.disconnect_after_reads = Some(5);
            let map = Mapfile::open_or_create(&map_path, 512, 120).unwrap();
            let out = OutputImage::create(&img_path, 120 * 512).unwrap();
            let mut eng = Engine::new(Arc::new(dev), map, out, cfg_small()).unwrap();
            let err = eng.run_pass(1).unwrap_err();
            assert!(matches!(err, EngineError::DeviceRemoved { .. }), "{err}");
        }

        // Resume with a healthy device: map and image carry on from where they stopped.
        let dev = MockDev::new(data.clone(), bad);
        let map = Mapfile::open_or_create(&map_path, 512, 120).unwrap();
        assert!(
            map.totals().recovered > 0,
            "progress before the disconnect was saved"
        );
        let out = OutputImage::open_resume(&img_path, 120 * 512).unwrap();
        let mut eng = Engine::new(Arc::new(dev), map, out, cfg_small()).unwrap();
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
    fn timed_out_read_calls_the_reset_hook() {
        let data = pattern(16, 512);
        let mut dev = MockDev::new(data, HashSet::new());
        dev.hang = [3u64].into_iter().collect();
        dev.hang_for = Duration::from_millis(600);
        let map = Mapfile::open_or_create(&tmp("reset.map"), 512, 16).unwrap();
        let out = OutputImage::create(&tmp("reset.img"), 16 * 512).unwrap();
        let cfg = EngineConfig {
            read_timeout: Duration::from_millis(100),
            ..cfg_small()
        };
        let mut eng = Engine::new(Arc::new(dev), map, out, cfg).unwrap();
        let calls = Arc::new(AtomicU64::new(0));
        let seen = Arc::clone(&calls);
        eng.set_reset_hook(Box::new(move |sector| {
            assert_eq!(sector, 0);
            seen.fetch_add(1, Ordering::SeqCst);
            Err("no root".to_string())
        }));
        eng.run_pass(1).unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(eng.last_reset_error(), Some("no root"));
    }

    #[test]
    fn cancel_flag_stops_the_run_with_progress_saved() {
        let data = pattern(40, 512);
        let dev = Arc::new(MockDev::new(data, HashSet::new()));
        let map = Mapfile::open_or_create(&tmp("cancel.map"), 512, 40).unwrap();
        let out = OutputImage::create(&tmp("cancel.img"), 40 * 512).unwrap();
        let mut eng = Engine::new(dev, map, out, cfg_small()).unwrap();
        let flag = Arc::new(AtomicBool::new(true));
        eng.set_cancel_flag(flag);
        let err = eng.run_pass(1).unwrap_err();
        assert!(matches!(err, EngineError::Stopped), "{err}");
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
