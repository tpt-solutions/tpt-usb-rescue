# tpt-usb-rescue TODO

## 0. Setup & project
- [x] Cargo workspace, edition 2024, MSRV 1.85
- [x] Dual license files (MIT OR Apache-2.0, TPT Solutions)
- [x] CLI skeleton (image, carve, analyze, verify, list-devices, mapfile)
- [x] CI workflow (Linux + Windows, fmt/clippy/test, MSRV)
- [ ] Confirm real repository URL in workspace Cargo.toml (URL is `github.com/tpt-solutions/tpt-usb-rescue`; needs owner confirmation)
- [x] `git init`, first commit
- [x] CONTRIBUTING.md, SECURITY.md, CHANGELOG.md

## 1. Milestone 1: HAL & basic imager (weeks 1-3)
- [x] Aligned buffer allocator (std::alloc with Layout)
- [x] Linux backend: O_RDONLY|O_DIRECT|O_SYNC, BLKSSZGET, BLKGETSIZE64 (type-checked for Linux; not run on a device)
- [x] Windows backend: \.\PhysicalDriveX, FILE_FLAG_NO_BUFFERING, geometry IOCTLs (compiled; not run on a device)
- [x] macOS backend stub (cfg-gated; returns `Unsupported`)
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
- [ ] 5s read timeout + USB controller reset (SCSI/ATA) (not implemented; a hung read still blocks)
- [x] Device-disconnect detection, clean handle close
- [x] `mapfile` subcommand: inspect, export to TOML / ddrescue format
- [x] Fault-injection block-device harness for tests (in-memory device with injected bad sectors and disconnects)
- [ ] Deliverable: image a drive with bad sectors without hanging the OS (blocked on the read timeout above)

## 3. Milestone 3: Signature carver (weeks 7-9)
- [x] Data-driven signature DB (table compiled in at build time)
- [x] memmap2 zero-copy scanner, memchr-style header search (first-byte scan; not SIMD yet)
- [x] Footer/size-based end detection (structure walks for JPEG, PNG, GIF, ZIP, PDF; max size as a bound)
- [ ] Top 20 types: JPEG, PNG, PDF, DOCX, MP4, ... (5 so far: JPEG, PNG, PDF, GIF, ZIP incl. DOCX/XLSX)
- [x] CarvedFile records + output writer
- [x] Deliverable: extract files from an img with corrupted partition table (carving ignores the partition table; tested with a garbage MBR)

## 4. Milestone 4: Verification & integrity (weeks 10-12)
- [ ] Built-in structural validators (JPEG markers/SOF/Huffman, PNG chunk CRCs, PDF xref/EOF, ZIP/DOCX, MP4 boxes) implementing MediaValidator; flag corrupted vs intact, integrity_score (JPEG, PNG, PDF, GIF, ZIP done; MP4 boxes missing; JPEG checks are structural, not Huffman-table decoding)
- [x] BLAKE3/SHA-256 hashing (blake3 + sha2 crates) of image and carved files
- [x] `verify` subcommand
- [x] `analyze` subcommand
- [ ] Docs, examples, release notes (CHANGELOG and README usage added; examples and 1.0 release notes still to write)
- [ ] Deliverable: v1.0.0

## 5. tpt-usb-rescue-fs
- [x] FAT32 parser (cluster chains, directory tree, LFN, deleted entries)
- [x] exFAT parser (file sets, NoFatChain runs, deleted entries)
- [ ] NTFS parser (MFT)
- [x] Rebuild tree in memory and extract despite bad partition table (boot-sector scan finds volumes without the MBR)

## 6. Edge cases (spec section 9)
- [x] Disconnect mid-read -> instant resume (tested: device removed mid-pass 1, resumed from the mapfile)
- [ ] Firmware lockup -> timeout + reset
- [x] False-positive signatures rejected by validation
- [x] Fragmented files (contiguous only in v1; `is_fragmented` is always false)

## 7. Quality
- [x] Unit tests per crate, property tests for mapfile replay
- [x] Integration tests with synthetic images (CLI end-to-end: image, carve, verify, resume guard, tamper detection)
- [ ] Fuzz carver and fs parsers (cargo-fuzz) (proptest no-panic tests on mutated volumes and arbitrary carver tails exist; no cargo-fuzz target yet)
- [ ] Benchmarks
- [ ] cargo-deny / audit

## 8. Post-1.0
- [ ] macOS HAL
- [ ] Fragmented-file stitching using fs metadata

## Decisions
- No external `tpt-*` crates (tpt-media, tpt-math, tpt-archon). Validation and hashing are implemented in-repo / with generic crates (blake3, sha2, memmap2).
