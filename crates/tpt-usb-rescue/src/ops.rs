//! Operations shared by the command-line tool and the GUI.
//!
//! Every operation writes its report to a [`Write`] sink, so each front end decides where the
//! text goes. Safety checks (output on the source disk, system-disk refusal) live here, so both
//! front ends get them.

use std::error::Error;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use tpt_usb_rescue_carver::extract::{file_name, write_carved};
use tpt_usb_rescue_carver::integrity::{Outcome, VerifyRow, digest, verify_output};
use tpt_usb_rescue_carver::{BuiltinValidator, CarveOptions, CarvedFile, carve};
use tpt_usb_rescue_fs::{Filesystem, FsError};
use tpt_usb_rescue_hal::{
    DeviceInfo, RawDevice, check_not_system_disk, check_output, list_devices, open_device,
    open_image,
};
use tpt_usb_rescue_imager::BlockStatus;
use tpt_usb_rescue_imager::engine::{Engine, EngineConfig, EngineError, ProgressHook};
use tpt_usb_rescue_imager::mapfile::{Mapfile, Totals};
use tpt_usb_rescue_imager::output::OutputImage;

pub type OpResult<T = ()> = Result<T, Box<dyn Error>>;

/// Optional engine settings. The defaults suit most drives; slow or fragile ones may need more.
#[derive(Debug, Clone, Default)]
pub struct Tuning {
    /// Size of one pass-1 read in KiB (default 1024). Smaller reads isolate bad areas sooner.
    pub chunk_kib: Option<u64>,
    /// Seconds before a read counts as hung (default 5).
    pub timeout_secs: Option<u64>,
    /// Do not re-enumerate the USB device after a hung read.
    pub no_reset: bool,
    /// Hash the finished image and write `<output>.sha256` (the `sha256sum` format).
    pub hash: bool,
}

/// Parameters for imaging a device or file.
#[derive(Debug, Clone)]
pub struct ImageRequest {
    pub device: PathBuf,
    pub output: PathBuf,
    pub mapfile: PathBuf,
    /// Run only this pass (1..=3). `None` runs all three.
    pub pass: Option<u8>,
    pub resume: bool,
    pub allow_system_disk: bool,
    pub tuning: Tuning,
    /// Raise to stop the copy. Progress is saved before the run stops.
    pub cancel: Option<Arc<AtomicBool>>,
}

/// Message returned when the user stops a copy.
pub const STOPPED_MESSAGE: &str = "Copy stopped. Your progress is saved.";

/// A run that stopped early with its progress saved: stopped by the user, device removed, or
/// device unresponsive. Resuming with `--resume` continues it. The command line maps this to its
/// own exit code.
#[derive(Debug)]
pub struct Interrupted(pub String);

impl std::fmt::Display for Interrupted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl Error for Interrupted {}

/// Carved files failed verification against the image.
#[derive(Debug)]
pub struct VerifyFailed(pub usize);

impl std::fmt::Display for VerifyFailed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} carved files failed verification", self.0)
    }
}

impl Error for VerifyFailed {}

/// Mapfile export formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Export {
    Ddrescue,
    Toml,
}

/// Images a device or regular file.
///
/// `progress` is called after every checkpoint with the current totals. Returns the final totals
/// so callers can tell a clean copy from one that left unrecovered data.
pub fn image(
    req: &ImageRequest,
    out: &mut dyn Write,
    progress: Option<ProgressHook>,
) -> OpResult<Totals> {
    let totals = copy(req, out, progress)?;
    if req.tuning.hash {
        write_hash_sidecar(&req.output, out)?;
    }
    Ok(totals)
}

/// The sidecar that holds an image's SHA-256: the image path with `.sha256` appended.
pub fn sidecar_path(image: &Path) -> PathBuf {
    let mut name = image.as_os_str().to_os_string();
    name.push(".sha256");
    PathBuf::from(name)
}

/// Hashes the finished image and records it next to it, so later `verify` runs can detect
/// changes. The image is written out of order and may be resumed, so it is hashed once, whole.
fn write_hash_sidecar(image: &Path, out: &mut dyn Write) -> OpResult {
    writeln!(out, "hashing image ...")?;
    let map = map_file(image)?;
    let d = digest(&map);
    let name = image.file_name().map_or_else(
        || image.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    let sidecar = sidecar_path(image);
    fs::write(&sidecar, format!("{}  {name}\n", d.sha256))?;
    writeln!(out, "image sha256 {}", d.sha256)?;
    writeln!(out, "image blake3 {}", d.blake3)?;
    writeln!(out, "hash recorded in {}", sidecar.display())?;
    Ok(())
}

fn copy(
    req: &ImageRequest,
    out: &mut dyn Write,
    progress: Option<ProgressHook>,
) -> OpResult<Totals> {
    let is_file = fs::metadata(&req.device)?.is_file();
    check_output(&req.device, &req.output)?;
    if is_file {
        let dev = Arc::new(open_image(&req.device)?);
        let totals = run_image(dev, req, out, progress, None)?;
        // Regular files are not sector multiples in general; keep the exact source length.
        let len = fs::metadata(&req.device)?.len();
        OpenOptions::new()
            .write(true)
            .open(&req.output)?
            .set_len(len)?;
        Ok(totals)
    } else {
        check_not_system_disk(&req.device, req.allow_system_disk)?;
        let dev = Arc::new(open_device(&req.device)?);
        run_image(dev, req, out, progress, Some(req.device.clone()))
    }
}

fn run_image<D: RawDevice + Send + Sync + 'static>(
    dev: Arc<D>,
    req: &ImageRequest,
    out: &mut dyn Write,
    progress: Option<ProgressHook>,
    reset_device: Option<PathBuf>,
) -> OpResult<Totals> {
    let geo = dev.geometry()?;
    let total = geo.sector_count * u64::from(geo.sector_size);
    writeln!(
        out,
        "device: {} sectors x {} B = {}",
        geo.sector_count,
        geo.sector_size,
        human_bytes(total)
    )?;

    let map = Mapfile::open_or_create(&req.mapfile, geo.sector_size, geo.sector_count)?;
    if map.skipped_records() > 0 {
        writeln!(
            out,
            "warning: {} damaged mapfile record(s) were skipped; the affected sectors will be read again",
            map.skipped_records()
        )?;
    }
    let already = map.totals();
    let has_progress = already.untested != total;
    let image = if req.resume {
        if has_progress {
            // The mapfile claims data is in the image; refuse if the image can't hold it,
            // rather than silently recreating it as zeros that the map calls recovered.
            let len = fs::metadata(&req.output).map(|m| m.len()).map_err(|_| {
                format!(
                    "cannot resume: image {} is missing but mapfile {} records progress",
                    req.output.display(),
                    req.mapfile.display()
                )
            })?;
            if len + u64::from(geo.sector_size) <= total {
                return Err(format!(
                    "cannot resume: image {} is {len} bytes but the mapfile expects {total}; \
                     the image was truncated or belongs to a different run",
                    req.output.display()
                )
                .into());
            }
        }
        OutputImage::open_resume(&req.output, total)?
    } else {
        if has_progress {
            return Err(format!(
                "mapfile {} already records progress; pass --resume to continue (GUI: tick Resume), or delete the mapfile to start over",
                req.mapfile.display()
            )
            .into());
        }
        OutputImage::create(&req.output, total)?
    };

    let mut cfg = EngineConfig::default();
    if let Some(kib) = req.tuning.chunk_kib {
        cfg.chunk_bytes = kib.saturating_mul(1024);
    }
    if let Some(secs) = req.tuning.timeout_secs {
        cfg.read_timeout = std::time::Duration::from_secs(secs);
    }
    let mut engine = Engine::new(dev, map, image, cfg)?;
    if let Some(flag) = req.cancel.clone() {
        engine.set_cancel_flag(flag);
    }
    if let Some(device) = reset_device.filter(|_| !req.tuning.no_reset) {
        engine.set_reset_hook(Box::new(move |_sector| {
            tpt_usb_rescue_hal::reset_device(&device).map_err(|e| e.to_string())
        }));
    }
    if let Some(hook) = progress {
        engine.set_progress_hook(hook);
    }
    let passes: Vec<u8> = req.pass.map_or_else(|| vec![1, 2, 3], |p| vec![p]);
    for p in passes {
        writeln!(out, "pass {p} ...")?;
        match engine.run_pass(p) {
            Ok(t) => writeln!(
                out,
                "  recovered {}  bad {}  non-trimmed {}  untested {}",
                human_bytes(t.recovered),
                human_bytes(t.bad),
                human_bytes(t.non_trimmed),
                human_bytes(t.untested)
            )?,
            Err(EngineError::DeviceRemoved { .. }) => {
                writeln!(out, "device removed; progress saved")?;
                return Err(Box::new(Interrupted("device removed".into())));
            }
            Err(EngineError::Stopped) => {
                writeln!(out, "{STOPPED_MESSAGE}")?;
                return Err(Box::new(Interrupted(STOPPED_MESSAGE.into())));
            }
            Err(e @ EngineError::Unresponsive { .. }) => {
                return Err(Box::new(Interrupted(e.to_string())));
            }
            Err(e) => return Err(e.into()),
        }
    }
    if let Some(e) = engine.last_reset_error() {
        writeln!(out, "USB reset failed: {e}")?;
    }
    let totals = engine.totals();
    engine.finish()?;
    writeln!(out, "image written to {}", req.output.display())?;
    Ok(totals)
}

/// What a carve run produced.
#[derive(Debug, Clone)]
pub struct CarveSummary {
    pub files: Vec<CarvedFile>,
    /// The `index.csv` that lists them.
    pub index: PathBuf,
}

/// Carves files from `image` into `output_dir`. Returns the number of files written.
pub fn carve_image(
    image: &Path,
    output_dir: &Path,
    validate_media: bool,
    out: &mut dyn Write,
) -> OpResult<usize> {
    Ok(carve_files(image, output_dir, validate_media, out)?
        .files
        .len())
}

/// Like [`carve_image`], but returns the carved files so callers can report them.
pub fn carve_files(
    image: &Path,
    output_dir: &Path,
    validate_media: bool,
    out: &mut dyn Write,
) -> OpResult<CarveSummary> {
    let map = map_file(image)?;
    let opts = CarveOptions {
        validate: validate_media,
        min_score: 0.5,
    };
    let files = carve(&map, &opts, &BuiltinValidator);
    let index = write_carved(&map, &files, output_dir)?;
    let intact = files.iter().filter(|f| f.integrity_score >= 1.0).count();
    writeln!(
        out,
        "carved {} files ({intact} structurally intact)",
        files.len()
    )?;
    for f in files.iter().take(20) {
        writeln!(
            out,
            "  {:<24} {:<5} {:>12} B  score {:.2}  @ {}",
            file_name(f),
            f.file_type.name(),
            f.size_bytes,
            f.integrity_score,
            f.start_offset
        )?;
    }
    if files.len() > 20 {
        writeln!(
            out,
            "  ... and {} more (all are listed in the index)",
            files.len() - 20
        )?;
    }
    writeln!(out, "index: {}", index.display())?;
    Ok(CarveSummary { files, index })
}

/// What `analyze` found in an image.
#[derive(Debug, Clone, Default)]
pub struct AnalyzeReport {
    pub partitions: Vec<tpt_usb_rescue_fs::MbrPartition>,
    pub volumes: Vec<VolumeReport>,
}

/// One volume found by the boot-sector scan.
#[derive(Debug, Clone)]
pub struct VolumeReport {
    /// The filesystem, such as `FAT32`.
    pub kind: String,
    pub offset: u64,
    /// Why the volume could not be opened, if it could not.
    pub error: Option<String>,
    pub entries: Vec<tpt_usb_rescue_fs::FileEntry>,
    /// Files written by `--extract`; `None` when nothing was extracted.
    pub extracted: Option<usize>,
}

/// Prints partition table, detected volumes and their files. Optionally extracts them.
pub fn analyze(image: &Path, extract: Option<&Path>, out: &mut dyn Write) -> OpResult {
    analyze_report(image, extract, out).map(|_| ())
}

/// Like [`analyze`], and also returns what it found (for `--json`).
pub fn analyze_report(
    image: &Path,
    extract: Option<&Path>,
    out: &mut dyn Write,
) -> OpResult<AnalyzeReport> {
    let map = map_file(image)?;
    writeln!(
        out,
        "image: {} ({})",
        image.display(),
        human_bytes(map.len() as u64)
    )?;

    let partitions = tpt_usb_rescue_fs::mbr_partitions(&map);
    if partitions.is_empty() {
        writeln!(out, "partition table: none found (MBR signature missing)")?;
    } else {
        writeln!(out, "partition table:")?;
        for p in &partitions {
            writeln!(
                out,
                "  #{} type 0x{:02X} start LBA {} ({} sectors)",
                p.index, p.partition_type, p.start_lba, p.sectors
            )?;
        }
    }

    let mut report = AnalyzeReport {
        partitions,
        volumes: Vec::new(),
    };
    let volumes = tpt_usb_rescue_fs::detect(&map);
    if volumes.is_empty() {
        writeln!(out, "volumes: none detected by boot-sector scan")?;
    }
    for v in &volumes {
        writeln!(out, "volume: {} at byte offset {}", v.kind, v.offset)?;
        let mut vol = VolumeReport {
            kind: v.kind.to_string(),
            offset: v.offset,
            error: None,
            entries: Vec::new(),
            extracted: None,
        };
        let fs = match tpt_usb_rescue_fs::open(&map, v.offset) {
            Ok(fs) => fs,
            Err(e) => {
                writeln!(out, "  cannot open: {e}")?;
                vol.error = Some(e.to_string());
                report.volumes.push(vol);
                continue;
            }
        };
        let entries = fs.entries();
        let deleted = entries.iter().filter(|e| e.deleted).count();
        writeln!(out, "  {} entries ({deleted} deleted)", entries.len())?;
        for e in entries.iter().take(50) {
            writeln!(
                out,
                "  {}{}  {} B",
                printable(&e.path),
                if e.is_dir { "/" } else { "" },
                e.size
            )?;
        }
        if entries.len() > 50 {
            writeln!(
                out,
                "  ... and {} more (use --extract to write them all out)",
                entries.len() - 50
            )?;
        }
        if let Some(dir) = extract {
            vol.extracted = Some(extract_volume(fs.as_ref(), v.offset, dir, out)?);
        }
        vol.entries = entries;
        report.volumes.push(vol);
    }
    Ok(report)
}

/// Writes every file on the volume under `dir/volume_<offset>`; returns how many were written.
fn extract_volume(
    fs: &dyn Filesystem,
    offset: u64,
    dir: &Path,
    out: &mut dyn Write,
) -> OpResult<usize> {
    let base = dir.join(format!("volume_{offset}"));
    let mut written = 0usize;
    for e in fs.entries().into_iter().filter(|e| !e.is_dir) {
        let data = match fs.read(&e) {
            Ok(d) => d,
            Err(FsError::IsDirectory) => continue,
            Err(err) => {
                writeln!(out, "skip {}: {err}", printable(&e.path))?;
                continue;
            }
        };
        // Names come from an untrusted image: never let them escape `base`.
        let rel = sanitize_rel_path(&e.path);
        if rel.as_os_str().is_empty() {
            writeln!(out, "skip {}: unusable name", printable(&e.path))?;
            continue;
        }
        let mut target = base.join(&rel);
        if e.deleted {
            let name = target
                .file_name()
                .map(|n| format!("DELETED_{}", n.to_string_lossy()))
                .unwrap_or_default();
            target.set_file_name(name);
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&target, &data)?;
        written += 1;
    }
    writeln!(out, "  extracted {written} files to {}", base.display())?;
    Ok(written)
}

/// A name from an untrusted image made safe to print: control characters (which could carry
/// terminal escape sequences) become `?`.
fn printable(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_control() { '?' } else { c })
        .collect()
}

/// Turns a filesystem path from an untrusted image into a safe relative path.
///
/// Splits on both `/` and `\`, drops empty, `.` and `..` components, replaces characters that are
/// invalid in Windows file names or are control characters, and strips trailing dots and spaces.
fn sanitize_rel_path(raw: &str) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in raw.split(['/', '\\']) {
        let clean: String = comp
            .chars()
            .map(|c| {
                if c.is_control() || matches!(c, ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                    '_'
                } else {
                    c
                }
            })
            .collect();
        let clean = clean.trim_end_matches(['.', ' ']);
        if clean.is_empty() {
            continue;
        }
        out.push(clean);
    }
    out
}

/// How the image compares with the hash recorded when it was made (`image --hash`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageHashCheck {
    /// There is no `<image>.sha256` next to the image.
    NotRecorded,
    Matches,
    Changed {
        recorded: String,
    },
}

/// Everything `verify` found.
#[derive(Debug, Clone)]
pub struct VerifyReport {
    pub blake3: String,
    pub sha256: String,
    pub image_hash: ImageHashCheck,
    pub rows: Vec<VerifyRow>,
    /// Carved files that differ or are missing, plus one if the image itself changed.
    pub failed: usize,
}

/// Checks carved files in `output_dir` against `image`. Errors if any file fails.
pub fn verify(output_dir: &Path, image: &Path, out: &mut dyn Write) -> OpResult {
    let report = verify_report(output_dir, image, out)?;
    if report.failed > 0 {
        return Err(Box::new(VerifyFailed(report.failed)));
    }
    Ok(())
}

/// Like [`verify`], but returns the findings instead of failing, so callers can report them.
pub fn verify_report(
    output_dir: &Path,
    image: &Path,
    out: &mut dyn Write,
) -> OpResult<VerifyReport> {
    let map = map_file(image)?;
    let d = digest(&map);
    writeln!(out, "image blake3 {}", d.blake3)?;
    writeln!(out, "image sha256 {}", d.sha256)?;
    let mut bad = 0usize;
    // If the image was hashed when it was made, check it still matches.
    let image_hash = match fs::read_to_string(sidecar_path(image)) {
        Ok(text) => {
            let recorded = text.split_whitespace().next().unwrap_or("");
            if recorded.eq_ignore_ascii_case(&d.sha256) {
                writeln!(out, "image matches its recorded hash")?;
                ImageHashCheck::Matches
            } else {
                writeln!(out, "IMAGE CHANGED: recorded sha256 is {recorded}")?;
                bad += 1;
                ImageHashCheck::Changed {
                    recorded: recorded.to_string(),
                }
            }
        }
        Err(_) => ImageHashCheck::NotRecorded,
    };
    let rows = verify_output(&map, output_dir).map_err(|e| {
        format!(
            "cannot read the carve index in {}: {e}",
            output_dir.display()
        )
    })?;
    for r in &rows {
        let label = match r.outcome {
            Outcome::Match => "OK",
            Outcome::Mismatch => {
                bad += 1;
                "MISMATCH"
            }
            Outcome::Missing => {
                bad += 1;
                "MISSING"
            }
        };
        writeln!(out, "{label:<9} {}", printable(&r.file))?;
    }
    writeln!(out, "{} checked, {bad} failed", rows.len())?;
    Ok(VerifyReport {
        blake3: d.blake3,
        sha256: d.sha256,
        image_hash,
        rows,
        failed: bad,
    })
}

/// Lists candidate devices and writes one line per device.
pub fn list(out: &mut dyn Write) -> OpResult<Vec<DeviceInfo>> {
    let devices = list_devices()?;
    if devices.is_empty() {
        writeln!(
            out,
            "no devices found (physical access may need administrator or root)"
        )?;
    }
    for d in &devices {
        let removable = match d.removable {
            Some(true) => "removable",
            Some(false) => "fixed",
            None => "unknown",
        };
        writeln!(
            out,
            "{:<28} {:>12}  {}",
            d.path.display(),
            human_bytes(d.size_bytes),
            removable
        )?;
    }
    Ok(devices)
}

/// The contents of a mapfile in numbers.
#[derive(Debug, Clone)]
pub struct MapSummary {
    pub sector_size: u32,
    pub sector_count: u64,
    pub replayed_records: u64,
    pub skipped_records: u64,
    pub totals: Totals,
    /// Unreadable runs as `(first sector, sector count)`.
    pub bad_runs: Vec<(u64, u64)>,
}

/// Reads a mapfile and summarises it.
pub fn mapfile_summary(path: &Path) -> OpResult<MapSummary> {
    let map = Mapfile::open_existing(path)?;
    Ok(MapSummary {
        sector_size: map.sector_size(),
        sector_count: map.sector_count(),
        replayed_records: map.replayed_records(),
        skipped_records: map.skipped_records(),
        totals: map.totals(),
        bad_runs: map
            .regions_with(BlockStatus::BadSector)
            .iter()
            .map(|r| (r.start_sector, r.length_sectors))
            .collect(),
    })
}

/// Summarises a mapfile, or exports it when `export` is set.
pub fn mapfile_report(
    path: &Path,
    export: Option<Export>,
    dest: Option<&Path>,
    out: &mut dyn Write,
) -> OpResult {
    let map = Mapfile::open_existing(path)?;
    let text = match export {
        Some(Export::Ddrescue) => map.to_ddrescue(),
        Some(Export::Toml) => map.to_toml(),
        None => {
            let t = map.totals();
            let mut s = format!(
                "mapfile {}\n  sector size {} B, {} sectors\n  replayed records {} ({} damaged, skipped)\n",
                path.display(),
                map.sector_size(),
                map.sector_count(),
                map.replayed_records(),
                map.skipped_records()
            );
            s.push_str(&format!(
                "  recovered {}\n  bad {}\n  non-trimmed {}\n  untested {}\n",
                human_bytes(t.recovered),
                human_bytes(t.bad),
                human_bytes(t.non_trimmed),
                human_bytes(t.untested)
            ));
            for r in map.regions_with(BlockStatus::BadSector) {
                s.push_str(&format!(
                    "  bad run: sector {} + {}\n",
                    r.start_sector, r.length_sectors
                ));
            }
            s
        }
    };
    match dest {
        Some(p) => {
            // Never overwrite: the destination could be the mapfile itself or another run's map.
            let mut f = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(p)
                .map_err(|e| {
                    if e.kind() == std::io::ErrorKind::AlreadyExists {
                        format!("{} already exists; choose a new --out file", p.display())
                    } else {
                        e.to_string()
                    }
                })?;
            f.write_all(text.as_bytes())?;
        }
        None => write!(out, "{text}")?,
    }
    Ok(())
}

/// Memory-maps an image read-only.
fn map_file(image: &Path) -> OpResult<memmap2::Mmap> {
    let file = File::open(image)?;
    // SAFETY: the mapping is read-only and this tool never writes to the image it maps. Another
    // process truncating or rewriting the file while it is mapped is undefined behaviour, so an
    // image must not be carved or verified while it is still being written.
    let map = unsafe { memmap2::Mmap::map(&file)? };
    Ok(map)
}

/// Formats a byte count with binary units.
pub fn human_bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{n} B")
    } else {
        format!("{v:.2} {}", UNITS[i])
    }
}

#[cfg(test)]
mod sanitize_tests {
    use super::sanitize_rel_path;
    use std::path::PathBuf;

    #[test]
    fn strips_traversal_and_drive_prefixes() {
        assert_eq!(
            sanitize_rel_path("/a/b.txt"),
            PathBuf::from("a").join("b.txt")
        );
        assert_eq!(
            sanitize_rel_path("../../etc/passwd"),
            PathBuf::from("etc").join("passwd")
        );
        assert_eq!(
            sanitize_rel_path(r"..\..\Windows\x.dll"),
            PathBuf::from("Windows").join("x.dll")
        );
        assert_eq!(
            sanitize_rel_path("C:/evil.exe"),
            PathBuf::from("C_").join("evil.exe")
        );
        assert_eq!(sanitize_rel_path("../.."), PathBuf::new());
        assert_eq!(sanitize_rel_path("a\u{0}b"), PathBuf::from("a_b"));
    }

    #[test]
    fn printable_hides_terminal_escapes() {
        assert_eq!(super::printable("a\x1b[31mb\n"), "a?[31mb?");
    }
}
