# tpt-usb-rescue-imager

Multi-pass forensic imager with a crash-safe, append-only mapfile, part of the
[tpt-usb-rescue](https://github.com/tpt-solutions/tpt-usb-rescue) recovery engine.

Status: **pre-alpha**. Implemented and tested on synthetic data. Not yet validated on failing
physical media.

## What it provides

- `engine`: a three-pass rescue state machine (sweep, 4 KiB / 512 B retries, edge trimming)
  that drives a `RawDevice`. It checkpoints progress and stops cleanly if the device is removed.
- `mapfile`: a binary append-only record of which regions are recovered, bad or untested.
  Each record carries a CRC32, and a torn tail is recovered on open. Export to ddrescue and
  TOML formats is supported.
- `output`: an image writer that skips all-zero blocks so fresh images stay sparse.
- `BlockRegion` and `BlockStatus`: the shared region and status types.

## Usage

```toml
[dependencies]
tpt-usb-rescue-imager = "0.1"
tpt-usb-rescue-hal = "0.1"
```

Sources are opened read-only through `tpt-usb-rescue-hal`. Pair the engine with a mapfile so an
interrupted run can resume without re-reading recovered sectors.

## License

Licensed under either of [Apache License, Version 2.0](../../LICENSE-APACHE) or
[MIT license](../../LICENSE-MIT) at your option.
