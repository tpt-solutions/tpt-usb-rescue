# tpt-usb-rescue-fs

Raw FAT32 and exFAT parsers that operate on byte slices, part of the
[tpt-usb-rescue](https://github.com/tpt-solutions/tpt-usb-rescue) recovery engine.

Status: **pre-alpha**. Implemented and tested on synthetic data. NTFS is not implemented yet.

## What it provides

- `detect`: finds volumes by scanning for boot-sector signatures. This works without a partition
  table, so it still finds volumes when the table is damaged.
- `mbr_partitions`: lists MBR partition entries.
- `open`: returns a `Box<dyn Filesystem>` for a volume at a given offset.
- `Filesystem`: a trait for walking directory trees, including LFN entries, deleted entries and
  FAT chains (or contiguous runs).
- `fat32` and `exfat`: the format-specific parsers, with `FsError` for failures.

The parsers read the image directly, so they work even when the OS refuses to mount the volume.

## Usage

```toml
[dependencies]
tpt-usb-rescue-fs = "0.1"
```

```rust
use tpt_usb_rescue_fs::{detect, open};

let image: &[u8] = &[]; // e.g. a memory-mapped image
for volume in detect(image) {
    let fs = open(image, volume.offset)?;
    // walk the directory tree through the Filesystem trait
}
```

## License

Licensed under either of [Apache License, Version 2.0](../../LICENSE-APACHE) or
[MIT license](../../LICENSE-MIT) at your option.
