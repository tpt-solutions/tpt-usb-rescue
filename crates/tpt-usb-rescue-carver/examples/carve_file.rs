//! Carves an image in memory and prints what it found.
//!
//! `cargo run -p tpt-usb-rescue-carver --example carve_file -- path/to/image.img`

use tpt_usb_rescue_carver::{BuiltinValidator, CarveOptions, carve};

fn main() -> std::io::Result<()> {
    let path = std::env::args().nth(1).expect("usage: carve_file <image>");
    let data = std::fs::read(&path)?;
    let files = carve(&data, &CarveOptions::default(), &BuiltinValidator);
    for f in &files {
        println!(
            "{:<5} {:>12} bytes at {:>12}  score {:.2}",
            f.file_type.name(),
            f.size_bytes,
            f.start_offset,
            f.integrity_score
        );
    }
    println!("{} files", files.len());
    Ok(())
}
