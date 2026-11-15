# Changelog

All notable changes to `tpt-usb-rescue-carver` are recorded here. The format follows Keep a
Changelog, and the crate uses semantic versioning once 1.0.0 is released.

## [Unreleased]

### Added
- Compiled-in signature table for 20 file types (JPEG, PNG, GIF, BMP, TIFF, WEBP, WAV, AVI, MP3,
  FLAC, OGG, PDF, ZIP/DOCX/XLSX, MP4/MOV, MKV, 7z, RAR, GZIP, SQLite, PSD), with structural end
  detection for the formats that have one.
- Built-in media validators that produce integrity scores.
- `index.csv` output via `extract::write_carved`, with a SHA-256 per carved file.
- BLAKE3 and SHA-256 hashing and output verification (`integrity`).
- A lazy `find_all`, so large images are not loaded into a list up front.

### Fixed
- ZIP walk picks the end record whose central directory is consistent, so ZIPs that contain stored ZIPs carve correctly.
- Files with no end marker are capped at 256 MiB.
- Hostile input is handled by checked arithmetic and bounded allocations, checked by fuzzing.

### Changed
- Exposes real types in place of the scaffold placeholders.

### Known limitations
- Carving is contiguous only: fragmented files are not reassembled.
- JPEG checks are segment-level, not Huffman-table decoding.
- Header-only formats (MP3, FLAC, TIFF, MKV, GZIP, PSD) are scored on their structure fields only.
- Not yet compared with a reference tool (PhotoRec, foremost) on real-world images.
