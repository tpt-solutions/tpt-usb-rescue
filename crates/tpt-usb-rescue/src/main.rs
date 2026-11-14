use clap::{Parser, Subcommand, ValueEnum};
use std::error::Error;
use std::io;
use std::path::PathBuf;
use tpt_usb_rescue::ops::{self, Export, ImageRequest};

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
    let mut out = io::stdout();
    match cmd {
        Command::Image {
            device,
            output,
            mapfile,
            pass,
            resume,
            allow_system_disk,
        } => {
            let req = ImageRequest {
                device,
                output,
                mapfile,
                pass,
                resume,
                allow_system_disk,
            };
            ops::image(&req, &mut out, None)
        }
        Command::Carve {
            image,
            output_dir,
            validate_media,
        } => ops::carve_image(&image, &output_dir, validate_media, &mut out).map(|_| ()),
        Command::Analyze { image, extract } => ops::analyze(&image, extract.as_deref(), &mut out),
        Command::Verify { output_dir, image } => ops::verify(&output_dir, &image, &mut out),
        Command::ListDevices => ops::list(&mut out).map(|_| ()),
        Command::Mapfile {
            mapfile,
            export,
            out: dest,
        } => {
            let export = export.map(|e| match e {
                MapExport::Ddrescue => Export::Ddrescue,
                MapExport::Toml => Export::Toml,
            });
            ops::mapfile_report(&mapfile, export, dest.as_deref(), &mut out)
        }
    }
}
