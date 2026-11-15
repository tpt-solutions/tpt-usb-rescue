# tpt-usb-rescue-imager

Multi-pass forensic imager with a crash-safe, append-only mapfile, part of the
[tpt-usb-rescue](https://github.com/tpt-solutions/tpt-usb-rescue) recovery engine.

Status: **release candidate 0.1.0-rc.1**. Tested on synthetic data and on real Linux block devices
(loop and device-mapper, including hung reads and Ctrl-C with resume). Not yet validated on USB
hardware.

## What it provides

- `engine`: a three-pass rescue state machine (sweep, 4 KiB / 512 B retries, edge trimming)
  that drives a `RawDevice`. Reads time out after 5 s and count as media failures. A hung area is
  survived when the drive can still read a sector it already recovered; a dead drive stops the run
  with progress saved. It checkpoints progress and stops cleanly if the device is removed.
- `mapfile`: a binary append-only record of which regions are recovered, bad or untested.
  Each record carries a CRC32, a torn tail is recovered on open, and a damaged record in the
  middle is skipped. Export to ddrescue and TOML formats is supported.
- `output`: an image writer that skips all-zero blocks so fresh images stay sparse. The image is
  synced before the mapfile records data as recovered.
- `BlockRegion` and `BlockStatus`: the shared region and status types.

## Usage

This crate is not yet published to crates.io. Depend on it by path or git until it is.

```toml
[dependencies]
tpt-usb-rescue-imager = { path = "../tpt-usb-rescue-imager" }
tpt-usb-rescue-hal = { path = "../tpt-usb-rescue-hal" }
```

Sources are opened read-only through `tpt-usb-rescue-hal`. Pair the engine with a mapfile so an
interrupted run can resume without re-reading recovered sectors.

## License

Licensed under either of [Apache License, Version 2.0](../../LICENSE-APACHE) or
[MIT license](../../LICENSE-MIT) at your option.
