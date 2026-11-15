# tpt-usb-rescue-carver

Zero-copy signature carver with media validation, part of the
[tpt-usb-rescue](https://github.com/tpt-solutions/tpt-usb-rescue) recovery engine.

Status: **release candidate 0.1.0-rc.1**. Tested on synthetic data and benchmarked (about 124 MiB/s
on a synthetic image). Not yet compared with a reference tool on real-world images.

## What it provides

- `SIGNATURES`, `Signature` and `FileType`: a compiled-in table of 20 file types: JPEG, PNG, GIF,
  BMP, TIFF, WEBP, WAV, AVI, MP3, FLAC, OGG, PDF, ZIP (including DOCX and XLSX), MP4/MOV, MKV, 7z,
  RAR, GZIP, SQLite and PSD.
- `find_end`: structural end-of-file detection. Several formats (JPEG, PNG, GIF, ZIP, PDF, BMP,
  RIFF, MP4, OGG, 7z, RAR, SQLite) are walked using their own layout; other types use a maximum size.
- `carve`: scans a byte slice (typically a memory-mapped image) without copying it, and returns
  `CarvedFile` candidates configured by `CarveOptions`.
- `MediaValidator` and `BuiltinValidator`: score each candidate with an integrity score, and reject
  candidates below the minimum score as false positives.
- `extract::write_carved`: writes carved files and an `index.csv` (with a SHA-256 per file).
- `integrity`: BLAKE3 and SHA-256 digests of images and carved files, plus `verify_output`.

Some formats (MP3, FLAC, TIFF, MKV, GZIP and PSD) are scored on their header structure only, so a
high score does not prove that the file's contents are intact.

## Usage

This crate is not yet published to crates.io. Depend on it by path or git until it is.

```rust
use tpt_usb_rescue_carver::{BuiltinValidator, CarveOptions, carve};

let image: &[u8] = &[]; // e.g. a memory-mapped image
let candidates = carve(image, &CarveOptions::default(), &BuiltinValidator);
```

## License

Licensed under either of [Apache License, Version 2.0](../../LICENSE-APACHE) or
[MIT license](../../LICENSE-MIT) at your option.
