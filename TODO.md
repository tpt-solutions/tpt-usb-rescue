# tpt-usb-rescue TODO

## 0. Setup & project
- [x] Cargo workspace, edition 2024, MSRV 1.85
- [x] Dual license files (MIT OR Apache-2.0, TPT Solutions)
- [x] CLI skeleton (image, carve, analyze, verify, list-devices, mapfile)
- [x] CI workflow (Linux + Windows, fmt/clippy/test, MSRV)
- [ ] Confirm real repository URL in workspace Cargo.toml (URL is `github.com/tpt-solutions/tpt-usb-rescue`; needs owner confirmation)
- [x] `git init`, first commit
- [x] CONTRIBUTING.md, SECURITY.md, CHANGELOG.md
- [x] Windows GUI (`tpt-usb-rescue-gui`): Win32 window, no third-party dependencies

## 1. Milestone 1: HAL & basic imager (weeks 1-3)
- [x] Aligned buffer allocator (std::alloc with Layout)
- [x] Linux backend: O_RDONLY|O_DIRECT|O_SYNC, BLKSSZGET, BLKGETSIZE64 (type-checked for Linux; not run on a device)
- [x] Windows backend: \.\PhysicalDriveX, FILE_FLAG_NO_BUFFERING, geometry IOCTLs (compiled; not run on a device)
- [x] macOS backend (`/dev/rdiskN` with F_NOCACHE, DKIOC geometry, /dev/diskN lookups; type-checked and clippy-clean for macOS, not run on a Mac)
- [x] `list-devices` implementation (Windows needs administrator rights to open physical drives)
- [x] Read-only safety guard (never write to device; refuse output on source device; confirm system disk)
- [x] Binary append-only mapfile: header, fixed-size records, per-record CRC, replay on startup, torn-write tolerance
- [x] Sequential reader, sparse output image writer
- [ ] Deliverable: clone a healthy USB to .img at hardware speed (file-source copy is byte-identical; not yet measured on hardware)

## 2. Milestone 2: Multi-pass rescue engine (weeks 4-6)
- [x] I/O fault classification (Uncategorized / InputOutput / device removed)
- [x] Pass 1: 1MB sweep, mark bad chunks
- [x] Pass 2: retries at 4KB then 512B
- [x] Pass 3: trimming bad-block edges (NonTrimmed -> exact boundary)
- [x] Deterministic state machine; resume after kill (`--resume`)
- [x] Zero-fill unrecoverable sectors, keep byte alignment
- [ ] 5s read timeout + USB controller reset (SCSI/ATA) (5 s timeout done: reads run on a reader thread and a timed-out read counts as a media failure; the controller reset is not implemented)
- [x] Device-disconnect detection, clean handle close
- [x] `mapfile` subcommand: inspect, export to TOML / ddrescue format
- [x] Fault-injection block-device harness for tests (in-memory device with injected bad sectors, hangs and disconnects)
- [ ] Deliverable: image a drive with bad sectors without hanging the OS (timeout done; needs a real failing drive to confirm, and the reset is still missing)

## 3. Milestone 3: Signature carver (weeks 7-9)
- [x] Data-driven signature DB (table compiled in at build time)
- [x] memmap2 zero-copy scanner, memchr-style header search (first-byte scan; not SIMD yet)
- [x] Footer/size-based end detection (structure walks for JPEG, PNG, GIF, ZIP, PDF, BMP, RIFF, MP4, OGG, 7z, RAR, SQLite; max size as a bound for the rest)
- [x] Top 20 types: JPEG, PNG, GIF, BMP, TIFF, WEBP, WAV, AVI, MP3, FLAC, OGG, PDF, ZIP (DOCX/XLSX), MP4/MOV, MKV, 7Z, RAR, GZIP, SQLite, PSD
- [x] CarvedFile records + output writer
- [x] Deliverable: extract files from an img with corrupted partition table (carving ignores the partition table; tested with a garbage MBR)

## 4. Milestone 4: Verification & integrity (weeks 10-12)
- [x] Built-in structural validators (JPEG markers/SOF/DHT/DQT, PNG chunk CRCs, PDF xref/EOF, ZIP/DOCX, MP4 boxes) implementing MediaValidator; flag corrupted vs intact, integrity_score (JPEG checks are segment-level, not Huffman-table decoding; header-only formats like MP3, FLAC, TIFF, MKV, GZIP and PSD are scored on their structure fields only)
- [x] BLAKE3/SHA-256 hashing (blake3 + sha2 crates) of image and carved files
- [x] `verify` subcommand
- [x] `analyze` subcommand
- [ ] Docs, examples, release notes (README, CHANGELOG, two library examples and benchmarks are done; the 1.0 release notes are written at release time)
- [ ] Deliverable: v1.0.0

## 5. tpt-usb-rescue-fs
- [x] FAT32 parser (cluster chains, directory tree, LFN, deleted entries)
- [x] exFAT parser (file sets, NoFatChain runs, deleted entries)
- [x] NTFS parser (MFT records with fixups, resident and run-list data, tree from parent references, deleted records)
- [x] Rebuild tree in memory and extract despite bad partition table (boot-sector scan finds FAT32, exFAT and NTFS volumes without the MBR)

## 6. Edge cases (spec section 9)
- [x] Disconnect mid-read -> instant resume (tested: device removed mid-pass 1, resumed from the mapfile)
- [ ] Firmware lockup -> timeout + reset (timeout done and tested; the reset is not implemented)
- [x] False-positive signatures rejected by validation
- [x] Fragmented files (contiguous only in v1; `is_fragmented` is always false)

## 7. Quality
- [x] Unit tests per crate, property tests for mapfile replay
- [x] Integration tests with synthetic images (CLI end-to-end: image, carve, verify, resume guard, tamper detection; multi-format carve)
- [x] Fuzz carver and fs parsers (cargo-fuzz targets in `fuzz/`; each target ran 60 s under libFuzzer in WSL with no crashes)
- [x] Benchmarks (`cargo bench -p tpt-usb-rescue-carver`: about 124 MiB/s carving on a synthetic image)
- [x] cargo-deny / audit (`deny.toml`: licenses, bans and sources pass, and advisories pass against the RustSec database)

## 8. Post-1.0
- [x] macOS HAL (type-checked for macOS; not run on a Mac)
- [ ] Fragmented-file stitching using fs metadata (FAT and exFAT readers already follow surviving FAT chains; stitching carved fragments of deleted files without metadata is not started)

## Decisions
- No external `tpt-*` crates (tpt-media, tpt-math, tpt-archon). Validation and hashing are implemented in-repo / with generic crates (blake3, sha2, memmap2).
- Dependency licenses: a crate is accepted when one of its licenses is permissive and listed in `deny.toml`. Crates offered only under Apache-2.0 are rejected.
