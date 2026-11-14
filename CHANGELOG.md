# Changelog

All notable changes are recorded here. The format follows Keep a Changelog, and the project
uses semantic versioning once 1.0.0 is released.

## [Unreleased]

### Added
- `tpt-usb-rescue-hal`: aligned buffers, read-only file/image and device handles, Linux
  (`O_DIRECT`, `BLKSSZGET`, `BLKGETSIZE64`) and Windows (`FILE_FLAG_NO_BUFFERING`,
  `DeviceIoControl`) backends, device listing, and output / system-disk safety guards.
- `tpt-usb-rescue-imager`: binary append-only mapfile with per-record CRC32, torn-tail recovery,
  ddrescue and TOML export; sparse-aware image writer; three-pass engine (sweep, 4 KiB / 512 B
  retries, edge trimming) with checkpointing and clean stop on device removal.
- `tpt-usb-rescue-carver`: compiled-in signature table (JPEG, PNG, PDF, GIF, ZIP), structural end
  detection, built-in validators with integrity scores, index.csv output, BLAKE3 and SHA-256
  verification.
- `tpt-usb-rescue-fs`: FAT32 and exFAT parsers (directory trees, LFN, deleted entries, FAT chains and
  contiguous runs), boot-sector volume detection that works without a partition table, MBR
  partition listing.
- CLI: `image` (with `--pass`, `--resume`, `--allow-system-disk`), `carve --validate-media`,
  `analyze --extract`, `verify`, `list-devices`, `mapfile --export ddrescue|toml`.
- Tests: unit tests, property tests for mapfile replay, no-panic property tests for parsers and
  the carver, and CLI end-to-end tests on synthetic images.

### Changed
- `tpt-usb-rescue-imager` and `tpt-usb-rescue-carver` expose real types instead of the scaffold placeholders.
  `BlockRegion::length_sectors` is now `u64`.

### Known limitations
- Hardware paths (Linux and Windows device reads, device listing) are compiled and reviewed but
  have not been run against physical USB media yet.
- No read timeout or controller reset yet; a hung device read blocks the run.
- NTFS is not parsed. Carving covers five file types.

## [0.1.0] - 2026-11-14

### Added
- Cargo workspace (edition 2024, MSRV 1.85) with crates `tpt-usb-rescue-hal`, `tpt-usb-rescue-imager`,
  `tpt-usb-rescue-carver`, `tpt-usb-rescue-fs` and the `tpt-usb-rescue` CLI.
- Dual MIT / Apache-2.0 license files.
- CLI skeleton with `image`, `carve`, `analyze`, `verify`, `list-devices` and `mapfile` subcommands.
- CI workflow for Linux and Windows (fmt, clippy, tests) and an MSRV check.
