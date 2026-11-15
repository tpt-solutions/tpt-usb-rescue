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

use tpt_usb_rescue_carver::extract::{file_name, write_carved};
use tpt_usb_rescue_carver::integrity::{Outcome, digest, verify_output};
use tpt_usb_rescue_carver::{BuiltinValidator, CarveOptions, carve};
use tpt_usb_rescue_fs::{Filesystem, FsError};
use tpt_usb_rescue_hal::{
    DeviceInfo, RawDevice, check_not_system_disk, check_output, list_devices, open_device,
    open_image,
};
use tpt_usb_rescue_imager::BlockStatus;
use tpt_usb_rescue_imager::engine::{Engine, EngineConfig, EngineError, ProgressHook};
use tpt_usb_rescue_imager::mapfile::Mapfile;
use tpt_usb_rescue_imager::output::OutputImage;

pub type OpResult<T = ()> = Result<T, Box<dyn Error>>;

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
}

/// Mapfile export formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Export {
    Ddrescue,
    Toml,
}

/// Images a device or regular file.
///
/// `progress` is called after every checkpoint with the current totals.
pub fn image(req: &ImageRequest, out: &mut dyn Write, progress: Option<ProgressHook>) -> OpResult {
    let is_file = fs::metadata(&req.device)?.is_file();
    check_output(&req.device, &req.output)?;
    if is_file {
        let dev = Arc::new(open_image(&req.device)?);
        run_image(dev, req, out, progress, None)?;
        // Regular files are not sector multiples in general; keep the exact source length.
        let len = fs::metadata(&req.device)?.len();
        OpenOptions::new()
            .write(true)
            .open(&req.output)?
            .set_len(len)?;
        Ok(())
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
) -> OpResult {
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
    let already = map.totals();
    let image = if req.resume {
        OutputImage::open_resume(&req.output, total)?
    } else {
        if already.recovered > 0 || already.bad > 0 {
            return Err(format!(
                "mapfile {} already records progress; pass --resume to continue (GUI: tick Resume), or delete the mapfile to start over",
                req.mapfile.display()
            )
            .into());
        }
        OutputImage::create(&req.output, total)?
    };

    let mut engine = Engine::new(dev, map, image, EngineConfig::default())?;
    if let Some(device) = reset_device {
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
                return Err("device removed".into());
            }
            Err(e) => return Err(e.into()),
        }
    }
    if let Some(e) = engine.last_reset_error() {
        writeln!(out, "USB reset failed: {e}")?;
    }
    engine.finish()?;
    writeln!(out, "image written to {}", req.output.display())?;
    Ok(())
}

/// Carves files from `image` into `output_dir`. Returns the number of files written.
pub fn carve_image(
    image: &Path,
    output_dir: &Path,
    validate_media: bool,
    out: &mut dyn Write,
) -> OpResult<usize> {
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
    writeln!(out, "index: {}", index.display())?;
    Ok(files.len())
}

/// Prints partition table, detected volumes and their files. Optionally extracts them.
pub fn analyze(image: &Path, extract: Option<&Path>, out: &mut dyn Write) -> OpResult {
    let map = map_file(image)?;
    writeln!(
        out,
        "image: {} ({})",
        image.display(),
        human_bytes(map.len() as u64)
    )?;

    let parts = tpt_usb_rescue_fs::mbr_partitions(&map);
    if parts.is_empty() {
        writeln!(out, "partition table: none found (MBR signature missing)")?;
    } else {
        writeln!(out, "partition table:")?;
        for p in parts {
            writeln!(
                out,
                "  #{} type 0x{:02X} start LBA {} ({} sectors)",
                p.index, p.partition_type, p.start_lba, p.sectors
            )?;
        }
    }

    let volumes = tpt_usb_rescue_fs::detect(&map);
    if volumes.is_empty() {
        writeln!(out, "volumes: none detected by boot-sector scan")?;
    }
    for v in &volumes {
        writeln!(out, "volume: {} at byte offset {}", v.kind, v.offset)?;
        let fs = match tpt_usb_rescue_fs::open(&map, v.offset) {
            Ok(fs) => fs,
            Err(e) => {
                writeln!(out, "  cannot open: {e}")?;
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
                e.path,
                if e.is_dir { "/" } else { "" },
                e.size
            )?;
        }
        if let Some(dir) = extract {
            extract_volume(fs.as_ref(), v.offset, dir, out)?;
        }
    }
    Ok(())
}

fn extract_volume(fs: &dyn Filesystem, offset: u64, dir: &Path, out: &mut dyn Write) -> OpResult {
    let base = dir.join(format!("volume_{offset}"));
    let mut written = 0usize;
    for e in fs.entries().into_iter().filter(|e| !e.is_dir) {
        let data = match fs.read(&e) {
            Ok(d) => d,
            Err(FsError::IsDirectory) => continue,
            Err(err) => {
                writeln!(out, "skip {}: {err}", e.path)?;
                continue;
            }
        };
        let rel = e.path.trim_start_matches('/');
        let target = base.join(if e.deleted {
            format!("DELETED_{rel}")
        } else {
            rel.to_string()
        });
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&target, &data)?;
        written += 1;
    }
    writeln!(out, "  extracted {written} files to {}", base.display())?;
    Ok(())
}

/// Checks carved files in `output_dir` against `image`. Errors if any file fails.
pub fn verify(output_dir: &Path, image: &Path, out: &mut dyn Write) -> OpResult {
    let map = map_file(image)?;
    let d = digest(&map);
    writeln!(out, "image blake3 {}", d.blake3)?;
    writeln!(out, "image sha256 {}", d.sha256)?;
    let rows = verify_output(&map, output_dir)?;
    let mut bad = 0usize;
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
        writeln!(out, "{label:<9} {}", r.file)?;
    }
    writeln!(out, "{} checked, {bad} failed", rows.len())?;
    if bad > 0 {
        return Err(format!("{bad} carved files failed verification").into());
    }
    Ok(())
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
                "mapfile {}\n  sector size {} B, {} sectors\n  replayed records {}\n",
                path.display(),
                map.sector_size(),
                map.sector_count(),
                map.replayed_records()
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
        Some(p) => fs::write(p, text)?,
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
