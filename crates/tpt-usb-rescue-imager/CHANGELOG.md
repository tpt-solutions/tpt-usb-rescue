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
- 5 s read timeout: reads run on a reader thread, and a timed-out read is treated as a media failure.
- USB device reset on Linux, called from the engine when a read times out (`--no-reset` turns it off).
- Hung-area handling: the reset fires once per hang, and after 5 timeouts in a row the engine
  re-reads a recovered sector. A hung area is survived; a dead drive stops the run.

### Changed
- Exposes real types in place of the scaffold placeholders. `BlockRegion::length_sectors` is
  now `u64`.

### Fixed
- The image is synced before the mapfile records data as recovered; short reads queue the unread
  tail for the next pass.
- A damaged record in the middle of a mapfile no longer discards every later record.
- A failed mapfile write rolls back. Device-removal error codes are per-OS (EIO stays a media error).
- A fatal read error leaves the map untouched.

### Known limitations
- Not yet validated on failing physical USB media.
- Passes 2 and 3 fsync twice per sector; batching them is not done yet.
