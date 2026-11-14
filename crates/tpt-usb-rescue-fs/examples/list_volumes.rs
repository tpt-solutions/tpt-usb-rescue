//! Finds FAT32, exFAT and NTFS volumes in an image, without using the partition table, and lists
//! their files (deleted entries included).
//!
//! `cargo run -p tpt-usb-rescue-fs --example list_volumes -- path/to/image.img`

fn main() -> std::io::Result<()> {
    let path = std::env::args()
        .nth(1)
        .expect("usage: list_volumes <image>");
    let data = std::fs::read(&path)?;
    for vol in tpt_usb_rescue_fs::detect(&data) {
        println!("{} volume at byte {}", vol.kind, vol.offset);
        let Ok(fs) = tpt_usb_rescue_fs::open(&data, vol.offset) else {
            continue;
        };
        for e in fs.entries() {
            let tag = if e.deleted { " (deleted)" } else { "" };
            println!(
                "  {}{} {} bytes{}",
                e.path,
                if e.is_dir { "/" } else { "" },
                e.size,
                tag
            );
        }
    }
    Ok(())
}
