# Changelog

All notable changes to the `tpt-usb-rescue` CLI are recorded here. The format follows Keep a
Changelog, and the crate uses semantic versioning once 1.0.0 is released.

## [Unreleased]

### Added
- `image` command with `--pass`, `--resume` and `--allow-system-disk`.
- `carve --validate-media`.
- `analyze --extract`.
- `verify`, `list-devices`, and `mapfile --export ddrescue|toml`.
- End-to-end tests on synthetic images.

### Known limitations
- Hardware paths (device reads and device listing) have not been run against physical USB media.
- No read timeout or controller reset yet; a hung device read blocks the run.
- NTFS is not parsed. Carving covers five file types.

## [0.1.0] - 2026-11-14

### Added
- CLI skeleton with `image`, `carve`, `analyze`, `verify`, `list-devices` and `mapfile`
  subcommands.
