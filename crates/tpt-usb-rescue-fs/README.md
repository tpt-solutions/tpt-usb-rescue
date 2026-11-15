# tpt-usb-rescue-fs

Raw FAT32, exFAT and NTFS parsers that operate on byte slices, part of the
[tpt-usb-rescue](https://github.com/tpt-solutions/tpt-usb-rescue) recovery engine.

Status: **release candidate 0.1.0-rc.1**. Tested on synthetic volumes, the practice images, and
fuzzed (the parsers are covered by libFuzzer targets in `fuzz/`).

## What it provides

- `detect`: finds volumes by scanning for boot-sector signatures. This works without a partition
  table, so it still finds volumes when the table is damaged. Backup boot sectors are ignored.
- `mbr_partitions`: lists MBR partition entries.
- `open`: returns a `Box<dyn Filesystem>` for a volume at a given offset.
- `Filesystem`: a trait for walking directory trees, including LFN entries, deleted entries and
  FAT chains (or contiguous runs).
- `fat32`, `exfat` and `ntfs`: the format-specific parsers, with `FsError` for failures. NTFS
  applies MFT fixups, reads resident and run-list data, and builds the tree from parent references.

The parsers read the image directly, so they work even when the OS refuses to mount the volume.
Not read: ext4, APFS and other filesystems.

## Usage

This crate is not yet published to crates.io. Depend on it by path or git until it is.

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
