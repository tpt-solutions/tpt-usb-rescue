# Changelog

All notable changes to `tpt-usb-rescue-imager` are recorded here. The format follows Keep a
Changelog, and the crate uses semantic versioning once 1.0.0 is released.

## [Unreleased]

### Added
- Binary append-only mapfile with per-record CRC32, torn-tail recovery, and ddrescue and TOML
  export.
- Sparse-aware image writer (`output`).
- Three-pass engine (sweep, 4 KiB / 512 B retries, edge trimming) with checkpointing and a clean
  stop on device removal.

### Changed
- Exposes real types in place of the scaffold placeholders. `BlockRegion::length_sectors` is
  now `u64`.

### Known limitations
- Not yet validated on failing physical USB media.
- No read timeout or controller reset yet; a hung device read blocks the run.
