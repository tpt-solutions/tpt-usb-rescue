# tpt-usb-rescue TODO

## 0. Setup & project
- [x] Cargo workspace, edition 2024, MSRV 1.85
- [x] Dual license files (MIT OR Apache-2.0, TPT Solutions)
- [x] CLI skeleton (image, carve, analyze, verify, list-devices, mapfile)
- [x] CI workflow (Linux + Windows, fmt/clippy/test, MSRV)
- [ ] Confirm real repository URL in workspace Cargo.toml
- [ ] `git init`, first commit
- [ ] CONTRIBUTING.md, SECURITY.md, CHANGELOG.md

## 1. Milestone 1: HAL & basic imager (weeks 1-3)
- [ ] Aligned buffer allocator (std::alloc with Layout)
- [ ] Linux backend: O_RDONLY|O_DIRECT|O_SYNC, BLKSSZGET, BLKGETSIZE64
- [ ] Windows backend: \.\PhysicalDriveX, FILE_FLAG_NO_BUFFERING, geometry IOCTLs
- [ ] macOS backend stub (cfg-gated)
- [ ] `list-devices` implementation
- [ ] Read-only safety guard (never write to device; refuse output on source device; confirm system disk)
- [ ] Binary append-only mapfile: header, fixed-size records, per-record CRC, replay on startup, torn-write tolerance
- [ ] Sequential reader, sparse output image writer
- [ ] Deliverable: clone a healthy USB to .img at hardware speed

## 2. Milestone 2: Multi-pass rescue engine (weeks 4-6)
- [ ] I/O fault classification (Uncategorized / InputOutput / device removed)
- [ ] Pass 1: 1MB sweep, mark bad chunks
- [ ] Pass 2: retries at 4KB then 512B
- [ ] Pass 3: trimming bad-block edges (NonTrimmed -> exact boundary)
- [ ] Deterministic state machine; resume after kill (`--resume`)
- [ ] Zero-fill unrecoverable sectors, keep byte alignment
- [ ] 5s read timeout + USB controller reset (SCSI/ATA)
- [ ] Device-disconnect detection, clean handle close
- [ ] `mapfile` subcommand: inspect, export to TOML / ddrescue format
- [ ] Fault-injection block-device harness for tests
- [ ] Deliverable: image a drive with bad sectors without hanging the OS

## 3. Milestone 3: Signature carver (weeks 7-9)
- [ ] Data-driven signature DB (table compiled in at build time)
- [ ] memmap2 zero-copy scanner, SIMD/memchr-style header search
- [ ] Footer/size-based end detection
- [ ] Top 20 types: JPEG, PNG, PDF, DOCX, MP4, ...
- [ ] CarvedFile records + output writer
- [ ] Deliverable: extract files from an img with corrupted partition table

## 4. Milestone 4: Verification & integrity (weeks 10-12)
- [ ] Built-in structural validators (JPEG markers/SOF/Huffman, PNG chunk CRCs, PDF xref/EOF, ZIP/DOCX, MP4 boxes) implementing MediaValidator; flag corrupted vs intact, integrity_score
- [ ] BLAKE3/SHA-256 hashing (blake3 + sha2 crates) of image and carved files
- [ ] `verify` subcommand
- [ ] `analyze` subcommand
- [ ] Docs, examples, release notes
- [ ] Deliverable: v1.0.0

## 5. rescue-fs
- [ ] FAT32 parser (cluster chains, directory tree)
- [ ] exFAT parser
- [ ] NTFS parser (MFT)
- [ ] Rebuild tree in memory and extract despite bad partition table

## 6. Edge cases (spec section 9)
- [ ] Disconnect mid-read -> instant resume
- [ ] Firmware lockup -> timeout + reset
- [ ] False-positive signatures rejected by validation
- [ ] Fragmented files (contiguous only in v1)

## 7. Quality
- [ ] Unit tests per crate, property tests for mapfile replay
- [ ] Integration tests with synthetic images
- [ ] Fuzz carver and fs parsers (cargo-fuzz)
- [ ] Benchmarks
- [ ] cargo-deny / audit

## 8. Post-1.0
- [ ] macOS HAL
- [ ] Fragmented-file stitching using fs metadata

## Decisions
- No external `tpt-*` crates (tpt-media, tpt-math, tpt-archon). Validation and hashing are implemented in-repo / with generic crates (blake3, sha2, memmap2).
