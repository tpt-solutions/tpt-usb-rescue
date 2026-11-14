use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "tpt-usb-rescue", version, about = "Forensic data recovery for failing USB devices")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Image a failing drive (Phase 1)
    Image {
        device: PathBuf,
        output: PathBuf,
        #[arg(long)]
        mapfile: PathBuf,
        #[arg(long)]
        pass: Option<u8>,
        #[arg(long)]
        resume: bool,
    },
    /// Carve files from an image (Phase 2)
    Carve {
        image: PathBuf,
        output_dir: PathBuf,
        #[arg(long)]
        validate_media: bool,
    },
    /// Analyze an image without extracting
    Analyze { image: PathBuf },
    /// Verify carved files against the raw image
    Verify { output_dir: PathBuf, image: PathBuf },
    /// List candidate block devices
    ListDevices,
    /// Inspect or export a mapfile
    Mapfile { mapfile: PathBuf },
}

fn main() {
    let _cli = Cli::parse();
    eprintln!("not implemented yet");
    std::process::exit(2);
}
