# tpt-usb-rescue-carver

Zero-copy signature carver with media validation, part of the
[tpt-usb-rescue](https://github.com/tpt-solutions/tpt-usb-rescue) recovery engine.

Status: **pre-alpha**. Implemented and tested on synthetic data.

## What it provides

- `SIGNATURES` and `Signature`: a compiled-in signature table covering JPEG, PNG, PDF, GIF and ZIP.
- `find_end`: structural end-of-file detection, using each format's own layout rather than a
  fixed size guess.
- `carve`: scans a byte slice (typically a memory-mapped image) without copying it, and returns
  `CarvedFile` candidates configured by `CarveOptions`.
- `MediaValidator` and `BuiltinValidator`: score each candidate, and reject candidates below the
  minimum score as false positives.
- `extract::write_carved`: writes carved files and an `index.csv`.
- `integrity`: BLAKE3 and SHA-256 digests of images and carved files, plus `verify_output`.

## Usage

```toml
[dependencies]
tpt-usb-rescue-carver = "0.1"
```

```rust
use tpt_usb_rescue_carver::{BuiltinValidator, CarveOptions, carve};

let image: &[u8] = &[]; // e.g. a memory-mapped image
let candidates = carve(image, &CarveOptions::default(), &BuiltinValidator);
```

## License

Licensed under either of [Apache License, Version 2.0](../../LICENSE-APACHE) or
[MIT license](../../LICENSE-MIT) at your option.
