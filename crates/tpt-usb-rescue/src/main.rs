use clap::{Parser, Subcommand, ValueEnum};
use tpt_usb_rescue_carver::extract::{file_name, write_carved};
use tpt_usb_rescue_carver::integrity::{Outcome, digest, verify_output};
use tpt_usb_rescue_carver::{BuiltinValidator, CarveOptions, carve};
use tpt_usb_rescue_fs::{Filesystem, FsError};
use tpt_usb_rescue_imager::BlockStatus;
use tpt_usb_rescue_imager::engine::{Engine, EngineConfig, EngineError};
use tpt_usb_rescue_imager::mapfile::Mapfile;
use tpt_usb_rescue_imager::output::OutputImage;
use std::error::Error;
use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use tpt_usb_rescue_hal::{
    RawDevice, check_not_system_disk, check_output, list_devices, open_device, open_image,
};

type CliResult = Result<(), Box<dyn Error>>;

#[derive(Parser)]
#[command(
    name = "tpt-usb-rescue",
    version,
    about = "Forensic data recovery for failing USB devices"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Clone, Copy, ValueEnum)]
enum MapExport {
    Ddrescue,
    Toml,
}

#[derive(Subcommand)]
enum Command {
    /// Image a failing drive (Phase 1)
    Image {
        /// Physical device (/dev/sdX, \\.\PhysicalDriveN) or a regular file to copy from.
        device: PathBuf,
        /// Output image file. Must not be on the source device.
        output: PathBuf,
        #[arg(long)]
        mapfile: PathBuf,
        /// Run only this pass (1 sweep, 2 sector retries, 3 trimming). Default: all three.
        #[arg(long, value_parser = clap::value_parser!(u8).range(1..=3))]
        pass: Option<u8>,
        /// Continue from an existing mapfile and image.
        #[arg(long)]
        resume: bool,
        /// Allow imaging the disk the operating system runs from.
        #[arg(long)]
        allow_system_disk: bool,
    },
    /// Carve files from an image (Phase 2)
    Carve {
        image: PathBuf,
        output_dir: PathBuf,
        /// Reject candidates whose structure fails validation (score below 0.5).
        /// Without this flag every candidate is written and its score is recorded in index.csv.
        #[arg(long)]
        validate_media: bool,
    },
    /// Analyze an image without extracting: partition table, volumes and files
    Analyze {
        image: PathBuf,
        /// Also extract files from every detected FAT32/exFAT volume into this directory.
        #[arg(long)]
        extract: Option<PathBuf>,
    },
    /// Verify carved files against the raw image
    Verify { output_dir: PathBuf, image: PathBuf },
    /// List candidate block devices
    ListDevices,
    /// Inspect or export a mapfile
    Mapfile {
        mapfile: PathBuf,
        /// Export format; prints to stdout unless --out is given.
        #[arg(long, value_enum)]
        export: Option<MapExport>,
        #[arg(long)]
        out: Option<PathBuf>,
    },
}

fn main() {
    let cli = Cli::parse();
    if let Err(e) = run(cli.command) {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run(cmd: Command) -> CliResult {
    match cmd {
        Command::Image {
            device,
            output,
            mapfile,
            pass,
            resume,
            allow_system_disk,
        } => cmd_image(&device, &output, &mapfile, pass, resume, allow_system_disk),
        Command::Carve {
            image,
            output_dir,
            validate_media,
        } => cmd_carve(&image, &output_dir, validate_media),
        Command::Analyze { image, extract } => cmd_analyze(&image, extract.as_deref()),
        Command::Verify { output_dir, image } => cmd_verify(&output_dir, &image),
        Command::ListDevices => cmd_list(),
        Command::Mapfile {
            mapfile,
            export,
            out,
        } => cmd_mapfile(&mapfile, export, out.as_deref()),
    }
}

fn cmd_image(
    device: &Path,
    output: &Path,
    mapfile: &Path,
    pass: Option<u8>,
    resume: bool,
    allow_system_disk: bool,
) -> CliResult {
    let is_file = fs::metadata(device)?.is_file();
    check_output(device, output)?;
    if !is_file {
        check_not_system_disk(device, allow_system_disk)?;
        let dev = open_device(device)?;
        run_image(&dev, output, mapfile, pass, resume)
    } else {
        let dev = open_image(device)?;
        run_image(&dev, output, mapfile, pass, resume)?;
        // Regular files are not sector-multiples in general; keep the exact source length.
        let len = fs::metadata(device)?.len();
        OpenOptions::new().write(true).open(output)?.set_len(len)?;
        Ok(())
    }
}

fn run_image<D: RawDevice>(
    dev: &D,
    output: &Path,
    mapfile: &Path,
    pass: Option<u8>,
    resume: bool,
) -> CliResult {
    let geo = dev.geometry()?;
    let total = geo.sector_count * u64::from(geo.sector_size);
    println!(
        "device: {} sectors x {} B = {}",
        geo.sector_count,
        geo.sector_size,
        human_bytes(total)
    );

    let map = Mapfile::open_or_create(mapfile, geo.sector_size, geo.sector_count)?;
    let already = map.totals();
    let out = if resume {
        OutputImage::open_resume(output, total)?
    } else {
        if already.recovered > 0 || already.bad > 0 {
            return Err(format!(
                "mapfile {} already records progress; pass --resume to continue or delete it to start over",
                mapfile.display()
            )
            .into());
        }
        OutputImage::create(output, total)?
    };

    let mut engine = Engine::new(dev, map, out, EngineConfig::default())?;
    let passes: Vec<u8> = pass.map_or_else(|| vec![1, 2, 3], |p| vec![p]);
    for p in passes {
        println!("pass {p} ...");
        match engine.run_pass(p) {
            Ok(t) => println!(
                "  recovered {}  bad {}  non-trimmed {}  untested {}",
                human_bytes(t.recovered),
                human_bytes(t.bad),
                human_bytes(t.non_trimmed),
                human_bytes(t.untested)
            ),
            Err(EngineError::DeviceRemoved { .. }) => {
                println!("device removed; progress saved");
                return Err("device removed".into());
            }
            Err(e) => return Err(e.into()),
        }
    }
    engine.finish()?;
    println!("image written to {}", output.display());
    Ok(())
}

fn cmd_carve(image: &Path, output_dir: &Path, validate_media: bool) -> CliResult {
    let file = File::open(image)?;
    // SAFETY: the image is opened read-only and is not modified while the map is alive. Another
    // process truncating the file underneath us would be undefined behaviour, so callers must
    // not carve an image that is still being written.
    let map = unsafe { memmap2::Mmap::map(&file)? };
    let opts = CarveOptions {
        validate: validate_media,
        min_score: 0.5,
    };
    let files = carve(&map, &opts, &BuiltinValidator);
    let index = write_carved(&map, &files, output_dir)?;
    let intact = files.iter().filter(|f| f.integrity_score >= 1.0).count();
    println!(
        "carved {} files ({intact} structurally intact)",
        files.len()
    );
    for f in files.iter().take(20) {
        println!(
            "  {:<24} {:<5} {:>12} B  score {:.2}  @ {}",
            file_name(f),
            f.file_type.name(),
            f.size_bytes,
            f.integrity_score,
            f.start_offset
        );
    }
    println!("index: {}", index.display());
    Ok(())
}

fn cmd_analyze(image: &Path, extract: Option<&Path>) -> CliResult {
    let file = File::open(image)?;
    // SAFETY: read-only mapping; see cmd_carve.
    let map = unsafe { memmap2::Mmap::map(&file)? };
    println!(
        "image: {} ({})",
        image.display(),
        human_bytes(map.len() as u64)
    );

    let parts = tpt_usb_rescue_fs::mbr_partitions(&map);
    if parts.is_empty() {
        println!("partition table: none found (MBR signature missing)");
    } else {
        println!("partition table:");
        for p in parts {
            println!(
                "  #{} type 0x{:02X} start LBA {} ({} sectors)",
                p.index, p.partition_type, p.start_lba, p.sectors
            );
        }
    }

    let volumes = tpt_usb_rescue_fs::detect(&map);
    if volumes.is_empty() {
        println!("volumes: none detected by boot-sector scan");
    }
    for v in &volumes {
        println!("volume: {} at byte offset {}", v.kind, v.offset);
        let fs = match tpt_usb_rescue_fs::open(&map, v.offset) {
            Ok(fs) => fs,
            Err(e) => {
                println!("  cannot open: {e}");
                continue;
            }
        };
        let entries = fs.entries();
        let deleted = entries.iter().filter(|e| e.deleted).count();
        println!("  {} entries ({deleted} deleted)", entries.len());
        for e in entries.iter().take(50) {
            println!(
                "  {}{}  {} B",
                e.path,
                if e.is_dir { "/" } else { "" },
                e.size
            );
        }
        if let Some(dir) = extract {
            extract_volume(fs.as_ref(), v.offset, dir)?;
        }
    }
    Ok(())
}

fn extract_volume(fs: &dyn Filesystem, offset: u64, dir: &Path) -> CliResult {
    let base = dir.join(format!("volume_{offset}"));
    let mut written = 0usize;
    for e in fs.entries().into_iter().filter(|e| !e.is_dir) {
        let data = match fs.read(&e) {
            Ok(d) => d,
            Err(FsError::IsDirectory) => continue,
            Err(err) => {
                eprintln!("skip {}: {err}", e.path);
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
    println!("  extracted {written} files to {}", base.display());
    Ok(())
}

fn cmd_verify(output_dir: &Path, image: &Path) -> CliResult {
    let file = File::open(image)?;
    // SAFETY: read-only mapping; see cmd_carve.
    let map = unsafe { memmap2::Mmap::map(&file)? };
    let d = digest(&map);
    println!("image blake3 {}", d.blake3);
    println!("image sha256 {}", d.sha256);
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
        println!("{label:<9} {}", r.file);
    }
    println!("{} checked, {bad} failed", rows.len());
    if bad > 0 {
        return Err(format!("{bad} carved files failed verification").into());
    }
    Ok(())
}

fn cmd_list() -> CliResult {
    let devices = list_devices()?;
    if devices.is_empty() {
        println!("no devices found (physical access may need administrator or root)");
    }
    for d in devices {
        let removable = match d.removable {
            Some(true) => "removable",
            Some(false) => "fixed",
            None => "unknown",
        };
        println!(
            "{:<28} {:>12}  {}",
            d.path.display(),
            human_bytes(d.size_bytes),
            removable
        );
    }
    Ok(())
}

fn cmd_mapfile(path: &Path, export: Option<MapExport>, out: Option<&Path>) -> CliResult {
    let map = Mapfile::open_existing(path)?;
    let text = match export {
        Some(MapExport::Ddrescue) => map.to_ddrescue(),
        Some(MapExport::Toml) => map.to_toml(),
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
    match out {
        Some(p) => fs::write(p, text)?,
        None => print!("{text}"),
    }
    Ok(())
}

fn human_bytes(n: u64) -> String {
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
