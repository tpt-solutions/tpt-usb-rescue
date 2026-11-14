# Changelog

All notable changes to `tpt-usb-rescue-carver` are recorded here. The format follows Keep a
Changelog, and the crate uses semantic versioning once 1.0.0 is released.

## [Unreleased]

### Added
- Compiled-in signature table (JPEG, PNG, PDF, GIF, ZIP) with structural end detection.
- Built-in media validators that produce integrity scores.
- `index.csv` output via `extract::write_carved`.
- BLAKE3 and SHA-256 hashing and output verification (`integrity`).

### Changed
- Exposes real types in place of the scaffold placeholders.

### Known limitations
- Carving covers five file types only.
