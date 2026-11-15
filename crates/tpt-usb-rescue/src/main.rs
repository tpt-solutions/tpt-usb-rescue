mod json;
mod progress;

use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::Shell;
use json::J;
use progress::Progress;
use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tpt_usb_rescue::ops::{self, Export, ImageRequest, Interrupted, Tuning, VerifyFailed};

type CliResult = Result<i32, Box<dyn Error>>;

/// Exit codes. Scripts can rely on these; clap itself exits with 2 for usage errors.
mod exit {
    pub const OK: i32 = 0;
    pub const ERROR: i32 = 1;
    /// Bad option combination (clap uses the same code for its own usage errors).
    pub const USAGE: i32 = 2;
    /// Stopped early with progress saved (Ctrl-C, device removed or unresponsive): resumable.
    pub const INTERRUPTED: i32 = 3;
    /// `verify` found carved files that do not match the image.
    pub const VERIFY_FAILED: i32 = 4;
    /// The image finished but some sectors could not be recovered.
    pub const UNRECOVERED_DATA: i32 = 5;
}

#[derive(Parser)]
#[command(
    name = "tpt-usb-rescue",
    version,
    about = "Forensic data recovery for failing USB devices"
)]
struct Cli {
    /// Do not show live progress on stderr.
    #[arg(short, long, global = true)]
    quiet: bool,
    /// Print the result as one JSON object on stdout; messages go to stderr instead.
    /// Supported by image, carve, verify, list-devices and mapfile (without --export).
    #[arg(long, global = true)]
    json: bool,
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
    /// Copy a failing drive to an image file, retrying and mapping bad sectors
    Image {
        /// Physical device (/dev/sdX, \\.\PhysicalDriveN) or a regular file to copy from.
        device: PathBuf,
        /// Output image file. Must not be on the source device.
        output: PathBuf,
        /// Progress file that makes the copy resumable. Default: the output name plus `.map`.
        #[arg(long)]
        mapfile: Option<PathBuf>,
        /// Run only this pass (1 sweep, 2 sector retries, 3 trimming). Default: all three.
        #[arg(long, value_parser = clap::value_parser!(u8).range(1..=3))]
        pass: Option<u8>,
        /// Continue from an existing mapfile and image.
        #[arg(long)]
        resume: bool,
        /// Allow imaging the disk the operating system runs from.
        #[arg(long)]
        allow_system_disk: bool,
        /// Seconds before a read counts as hung (default 5).
        #[arg(long, value_name = "SECS", value_parser = clap::value_parser!(u64).range(1..=600))]
        timeout: Option<u64>,
        /// Size of one pass-1 read in KiB (default 1024). Smaller isolates bad areas sooner.
        #[arg(long, value_name = "KIB", value_parser = clap::value_parser!(u64).range(4..=65536))]
        chunk_kib: Option<u64>,
        /// Do not re-enumerate the USB device after a hung read (Linux only; needs root).
        #[arg(long)]
        no_reset: bool,
        /// After copying, hash the image and write `<output>.sha256`; `verify` checks it later.
        #[arg(long)]
        hash: bool,
    },
    /// Recover files from an image by file signature, ignoring the filesystem
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
        /// Also extract files from every detected FAT32, exFAT or NTFS volume into this directory.
        #[arg(long)]
        extract: Option<PathBuf>,
    },
    /// Verify carved files against the raw image
    Verify { output_dir: PathBuf, image: PathBuf },
    /// List candidate block devices
    ListDevices,
    /// Print a shell completion script (bash, zsh, fish, elvish or powershell)
    Completions { shell: Shell },
    /// Print the manual page (roff) to stdout, or write one page per command to a folder
    Man {
        /// Write tpt-usb-rescue.1 and tpt-usb-rescue-<command>.1 here instead of printing.
        #[arg(long, value_name = "DIR")]
        out_dir: Option<PathBuf>,
    },
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
    let json = cli.json;
    let code = match run(cli.command, cli.quiet, json) {
        Ok(code) => code,
        Err(e) => {
            let interrupted = e.is::<Interrupted>();
            let code = if interrupted {
                exit::INTERRUPTED
            } else if e.is::<VerifyFailed>() {
                exit::VERIFY_FAILED
            } else {
                exit::ERROR
            };
            if interrupted {
                eprintln!("{e}");
                eprintln!("Run the same command again with --resume to continue.");
            } else {
                eprintln!("error: {e}");
            }
            if json {
                println!(
                    "{}",
                    J::Obj(vec![
                        ("ok", J::Bool(false)),
                        ("error", J::from(e.to_string())),
                        ("interrupted", J::Bool(interrupted)),
                        ("exit_code", J::from(code as u64)),
                    ])
                );
            }
            code
        }
    };
    std::process::exit(code);
}

/// `<output>.map`, next to the image.
fn default_mapfile(output: &Path) -> PathBuf {
    let mut name = output.as_os_str().to_os_string();
    name.push(".map");
    PathBuf::from(name)
}

/// Raises the returned flag on the first Ctrl-C so the copy stops at its next checkpoint with
/// progress saved. A second Ctrl-C quits at once.
fn stop_on_ctrl_c() -> Arc<AtomicBool> {
    let flag = Arc::new(AtomicBool::new(false));
    let handler_flag = Arc::clone(&flag);
    let _ = ctrlc::set_handler(move || {
        if handler_flag.swap(true, Ordering::SeqCst) {
            std::process::exit(130);
        }
        eprintln!(
            "\nStopping after the current block; progress is saved (Ctrl-C again to quit now)..."
        );
    });
    flag
}

/// Prints the `--json` result for a command.
fn emit(value: J) {
    println!("{value}");
}

fn run(cmd: Command, quiet: bool, json: bool) -> CliResult {
    let progress = Progress::new();
    // With --json, stdout carries only the result; everything else goes to stderr.
    let mut out = progress.writer(json);
    match cmd {
        Command::Image {
            device,
            output,
            mapfile,
            pass,
            resume,
            allow_system_disk,
            timeout,
            chunk_kib,
            no_reset,
            hash,
        } => {
            let req = ImageRequest {
                mapfile: mapfile.unwrap_or_else(|| default_mapfile(&output)),
                device,
                output,
                pass,
                resume,
                allow_system_disk,
                tuning: Tuning {
                    chunk_kib,
                    timeout_secs: timeout,
                    no_reset,
                    hash,
                },
                cancel: Some(stop_on_ctrl_c()),
            };
            let hook = (!quiet).then(|| progress.hook());
            let result = ops::image(&req, &mut out, hook);
            progress.clear();
            let totals = result?;
            // A single pass leaves work for the next one by design; only a full run is judged.
            let complete = matches!(pass, None | Some(3));
            let lost = totals.bad + totals.non_trimmed + totals.untested;
            let code = if complete && lost > 0 {
                eprintln!(
                    "finished, but {} could not be recovered (see: tpt-usb-rescue mapfile {})",
                    ops::human_bytes(lost),
                    req.mapfile.display()
                );
                exit::UNRECOVERED_DATA
            } else {
                exit::OK
            };
            if json {
                emit(J::Obj(vec![
                    ("ok", J::Bool(true)),
                    ("command", J::from("image")),
                    ("output", J::from(req.output.as_path())),
                    ("mapfile", J::from(req.mapfile.as_path())),
                    ("recovered_bytes", J::from(totals.recovered)),
                    ("bad_bytes", J::from(totals.bad)),
                    ("non_trimmed_bytes", J::from(totals.non_trimmed)),
                    ("untested_bytes", J::from(totals.untested)),
                    ("unrecovered_bytes", J::from(lost)),
                    ("hash_recorded", J::Bool(hash)),
                    ("exit_code", J::from(code as u64)),
                ]));
            }
            Ok(code)
        }
        Command::Carve {
            image,
            output_dir,
            validate_media,
        } => {
            let summary = ops::carve_files(&image, &output_dir, validate_media, &mut out)?;
            if json {
                let intact = summary
                    .files
                    .iter()
                    .filter(|f| f.integrity_score >= 1.0)
                    .count();
                let files = summary
                    .files
                    .iter()
                    .map(|f| {
                        J::Obj(vec![
                            ("id", J::from(f.id)),
                            (
                                "file",
                                J::from(tpt_usb_rescue_carver::extract::file_name(f)),
                            ),
                            ("type", J::from(f.file_type.name())),
                            ("start_offset", J::from(f.start_offset)),
                            ("end_offset", J::from(f.end_offset)),
                            ("size_bytes", J::from(f.size_bytes)),
                            (
                                "integrity_score",
                                J::Float((f64::from(f.integrity_score) * 1000.0).round() / 1000.0),
                            ),
                        ])
                    })
                    .collect();
                emit(J::Obj(vec![
                    ("ok", J::Bool(true)),
                    ("command", J::from("carve")),
                    ("index", J::from(summary.index.as_path())),
                    ("count", J::from(summary.files.len())),
                    ("intact", J::from(intact)),
                    ("files", J::Arr(files)),
                ]));
            }
            Ok(exit::OK)
        }
        Command::Analyze { image, extract } => {
            let report = ops::analyze_report(&image, extract.as_deref(), &mut out)?;
            if json {
                emit(analyze_json(&image, &report));
            }
            Ok(exit::OK)
        }
        Command::Verify { output_dir, image } => {
            let report = ops::verify_report(&output_dir, &image, &mut out)?;
            let code = if report.failed > 0 {
                exit::VERIFY_FAILED
            } else {
                exit::OK
            };
            if json {
                use tpt_usb_rescue_carver::integrity::Outcome;
                let check = match &report.image_hash {
                    ops::ImageHashCheck::NotRecorded => "not_recorded",
                    ops::ImageHashCheck::Matches => "matches",
                    ops::ImageHashCheck::Changed { .. } => "changed",
                };
                let files = report
                    .rows
                    .iter()
                    .map(|r| {
                        let result = match r.outcome {
                            Outcome::Match => "ok",
                            Outcome::Mismatch => "mismatch",
                            Outcome::Missing => "missing",
                        };
                        J::Obj(vec![
                            ("id", J::from(r.id)),
                            ("file", J::from(r.file.as_str())),
                            ("result", J::from(result)),
                        ])
                    })
                    .collect();
                emit(J::Obj(vec![
                    ("ok", J::Bool(report.failed == 0)),
                    ("command", J::from("verify")),
                    ("image_blake3", J::from(report.blake3.as_str())),
                    ("image_sha256", J::from(report.sha256.as_str())),
                    ("image_hash", J::from(check)),
                    ("checked", J::from(report.rows.len())),
                    ("failed", J::from(report.failed)),
                    ("files", J::Arr(files)),
                    ("exit_code", J::from(code as u64)),
                ]));
            } else if report.failed > 0 {
                eprintln!("error: {} carved files failed verification", report.failed);
            }
            Ok(code)
        }
        Command::Completions { shell } => {
            let mut cmd = Cli::command();
            clap_complete::generate(shell, &mut cmd, "tpt-usb-rescue", &mut std::io::stdout());
            Ok(exit::OK)
        }
        Command::Man { out_dir } => {
            let mut cmd = Cli::command();
            cmd.build(); // propagates the global options (--quiet, --json) to every command
            match out_dir {
                None => clap_mangen::Man::new(cmd).render(&mut std::io::stdout())?,
                Some(dir) => write_man_pages(&cmd, &dir)?,
            }
            Ok(exit::OK)
        }
        Command::ListDevices => {
            let devices = ops::list(&mut out)?;
            if json {
                let list = devices
                    .iter()
                    .map(|d| {
                        J::Obj(vec![
                            ("path", J::from(d.path.as_path())),
                            ("size_bytes", J::from(d.size_bytes)),
                            ("removable", d.removable.map_or(J::Null, J::Bool)),
                        ])
                    })
                    .collect();
                emit(J::Obj(vec![
                    ("ok", J::Bool(true)),
                    ("command", J::from("list-devices")),
                    ("devices", J::Arr(list)),
                ]));
            }
            Ok(exit::OK)
        }
        Command::Mapfile {
            mapfile,
            export,
            out: dest,
        } => {
            if json {
                if export.is_some() {
                    return unsupported_json("mapfile --export");
                }
                let m = ops::mapfile_summary(&mapfile)?;
                let runs = m
                    .bad_runs
                    .iter()
                    .map(|(start, len)| {
                        J::Obj(vec![
                            ("start_sector", J::from(*start)),
                            ("sectors", J::from(*len)),
                        ])
                    })
                    .collect();
                emit(J::Obj(vec![
                    ("ok", J::Bool(true)),
                    ("command", J::from("mapfile")),
                    ("path", J::from(mapfile.as_path())),
                    ("sector_size", J::from(m.sector_size)),
                    ("sector_count", J::from(m.sector_count)),
                    ("replayed_records", J::from(m.replayed_records)),
                    ("skipped_records", J::from(m.skipped_records)),
                    ("recovered_bytes", J::from(m.totals.recovered)),
                    ("bad_bytes", J::from(m.totals.bad)),
                    ("non_trimmed_bytes", J::from(m.totals.non_trimmed)),
                    ("untested_bytes", J::from(m.totals.untested)),
                    ("bad_runs", J::Arr(runs)),
                ]));
                return Ok(exit::OK);
            }
            let export = export.map(|e| match e {
                MapExport::Ddrescue => Export::Ddrescue,
                MapExport::Toml => Export::Toml,
            });
            ops::mapfile_report(&mapfile, export, dest.as_deref(), &mut out).map(|()| exit::OK)
        }
    }
}

/// Writes `tpt-usb-rescue.1` and one `tpt-usb-rescue-<command>.1` per command into `dir`.
fn write_man_pages(cmd: &clap::Command, dir: &Path) -> Result<(), Box<dyn Error>> {
    std::fs::create_dir_all(dir)?;
    let mut main = Vec::new();
    clap_mangen::Man::new(cmd.clone()).render(&mut main)?;
    std::fs::write(dir.join("tpt-usb-rescue.1"), main)?;
    for sub in cmd
        .get_subcommands()
        .filter(|s| !s.is_hide_set() && s.get_name() != "help")
    {
        // The page title is the command name, so give it the full "tpt-usb-rescue-image" form.
        // Leaking one short string per command is fine in a run that exits right after.
        let name: &'static str =
            Box::leak(format!("tpt-usb-rescue-{}", sub.get_name()).into_boxed_str());
        let mut page = Vec::new();
        clap_mangen::Man::new(sub.clone().name(name).version(env!("CARGO_PKG_VERSION")))
            .render(&mut page)?;
        std::fs::write(dir.join(format!("{name}.1")), page)?;
    }
    Ok(())
}

/// The `analyze --json` object: partitions, then each volume with its entries.
fn analyze_json(image: &std::path::Path, report: &ops::AnalyzeReport) -> J {
    let partitions = report
        .partitions
        .iter()
        .map(|p| {
            J::Obj(vec![
                ("index", J::from(u32::from(p.index))),
                ("type", J::from(u32::from(p.partition_type))),
                ("start_lba", J::from(p.start_lba)),
                ("sectors", J::from(p.sectors)),
            ])
        })
        .collect();
    let volumes = report
        .volumes
        .iter()
        .map(|v| {
            let entries = v
                .entries
                .iter()
                .map(|e| {
                    J::Obj(vec![
                        ("path", J::from(e.path.as_str())),
                        ("is_dir", J::from(e.is_dir)),
                        ("deleted", J::from(e.deleted)),
                        ("size", J::from(e.size)),
                    ])
                })
                .collect();
            J::Obj(vec![
                ("kind", J::from(v.kind.as_str())),
                ("offset", J::from(v.offset)),
                ("error", v.error.clone().map_or(J::Null, J::from)),
                ("extracted", v.extracted.map_or(J::Null, J::from)),
                ("entries", J::Arr(entries)),
            ])
        })
        .collect();
    J::Obj(vec![
        ("ok", J::Bool(true)),
        ("command", J::from("analyze")),
        ("image", J::from(image)),
        ("partitions", J::Arr(partitions)),
        ("volumes", J::Arr(volumes)),
    ])
}

/// `--json` was given to a command that has no JSON form. Reported as a usage error.
fn unsupported_json(command: &str) -> CliResult {
    eprintln!("error: --json is not supported for `{command}`");
    Ok(exit::USAGE)
}
