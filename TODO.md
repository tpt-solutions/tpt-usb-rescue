# tpt-usb-rescue TODO

## 0. Setup & project
- [x] Cargo workspace, edition 2024, MSRV 1.85
- [x] Dual license files (MIT OR Apache-2.0, TPT Solutions)
- [x] CLI skeleton (image, carve, analyze, verify, list-devices, mapfile)
- [x] CI workflow (Linux + Windows, fmt/clippy/test, MSRV)
- [x] Confirm real repository URL in workspace Cargo.toml (`https://github.com/tpt-solutions/tpt-usb-rescue`, confirmed by the owner)
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
- [x] 5s read timeout + USB reset (5 s timeout: reads run on a reader thread and a timed-out read counts as a media failure. Reset, Linux only: the USB device behind the disk is re-enumerated through sysfs `authorized`, which needs root. This is device re-enumeration, not a controller reset, and it has not been run on hardware)
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
- [x] Docs, examples, release notes (docs/USAGE.md, docs/ARCHITECTURE.md, docs/FORMATS.md; two library examples; benchmarks; 1.0.0 release-notes draft in docs/release/)
- [ ] Deliverable: v1.0.0 (release notes drafted, with the release gate and publishing steps in docs/release/1.0.0-DRAFT.md; blocked on hardware checks and on publishing, which is not done)

## 5. tpt-usb-rescue-fs
- [x] FAT32 parser (cluster chains, directory tree, LFN, deleted entries)
- [x] exFAT parser (file sets, NoFatChain runs, deleted entries)
- [x] NTFS parser (MFT records with fixups, resident and run-list data, tree from parent references, deleted records)
- [x] Rebuild tree in memory and extract despite bad partition table (boot-sector scan finds FAT32, exFAT and NTFS volumes without the MBR)

## 6. Edge cases (spec section 9)
- [x] Disconnect mid-read -> instant resume (tested: device removed mid-pass 1, resumed from the mapfile)
- [x] Firmware lockup -> timeout + reset (timeout tested; reset is the Linux sysfs re-enumeration above, untested on hardware; the run stops after a reset, so resume afterwards)
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

## 9. Review follow-up: bugs and data safety (2026-11 platform review)
Findings come from a read-through of all crates. Fixed items are covered by tests where noted; Linux and macOS changes compile and pass clippy for those targets but have not run on real devices.
- [x] Linux/macOS: output-on-source check failed for an output file that does not exist yet (stat the nearest existing ancestor)
- [x] `--resume` refuses a missing or truncated image instead of recreating it as zeros the mapfile calls recovered
- [x] `analyze --extract` sanitizes names from untrusted images (no `..`, drive prefixes, separators; tested)
- [x] Resume guard counts any recorded progress, not only recovered/bad sectors
- [x] System-disk and output-on-source guards cover partitions and volumes (Linux sysfs + mountinfo for btrfs/LVM/LUKS/md, macOS slices, Windows `\\.\X:` volumes)
- [ ] macOS: system-disk guard on APFS (root is a synthesized container disk, not the physical disk)
- [x] Short reads queue the unread tail for the next pass (tested)
- [x] Image is fsynced before the mapfile records data as recovered
- [x] Per-OS device-removal error codes (EIO stays a media error; tested)
- [x] Stop after 10 consecutive timed-out reads (no more leaked reader threads); USB reset fires once per hang (tested)
- [x] exFAT NoFatChain directories read beyond the first cluster (tested)
- [x] NTFS/FAT32/exFAT: checked arithmetic and allocation caps on hostile input
- [ ] Run the fuzz targets again with overflow checks (debug build) after the fs hardening
- [x] `detect()` ignores backup boot sectors (no test yet)
- [x] Mapfile: a damaged record in the middle is skipped (later records kept, user warned); failed writes roll back; `u32::MAX` clamp removed (tested)
- [x] Carver: ZIP walk picks the end record with a consistent directory (stored inner ZIPs; tested); files with no end marker are capped at 256 MiB; `find_all` is lazy
- [x] USB reset retries re-authorizing and reports how to recover if it fails
- [x] `BLKGETSIZE64` encoding correct on MIPS, PowerPC and SPARC
- [x] Windows UNC output paths are refused instead of matched to a fake drive letter
- [x] `index.csv` names that escape the output folder are rejected (tested)
- [x] `mapfile --export --out` never overwrites an existing file, including the mapfile itself (tested)
- [ ] Pass 3 only retries the first and last sector of each bad run: either implement real trimming or fix the wording above
- [ ] Passes 2 and 3 fsync twice per sector; batch them
- [ ] FAT32 long names: validate the LFN checksum
- [x] Tests added: resume with a truncated or missing image (end to end)
- [ ] Tests still missing: hostile extract names end to end, backup-boot-sector dedupe, `Fault::Fatal` paths in the engine, pass 3 behaviour

## 10. CLI usability and automation
- [x] Progress on stderr (percent, bytes, rate, ETA, bad bytes); non-TTY aware; `--quiet` (a `-v` flag is not added)
- [x] Ctrl-C stops cleanly through `ImageRequest.cancel` and prints how to resume; a second Ctrl-C quits at once (not hand-tested with a real terminal)
- [x] Documented exit codes: 0 complete, 2 usage, 3 stopped or device removed, 4 verify mismatch, 5 finished with unrecovered data (docs/USAGE.md; 0, 2, 4 tested end to end, 3 and 5 need a device)
- [x] `--json` output for image, carve, verify, list-devices, mapfile (docs/USAGE.md; tested)
- [ ] `--json` for `analyze`
- [x] `--mapfile` defaults to `<output>.map`; "Phase 1/2" wording dropped; `analyze --extract` help fixed
- [x] Tunables: `--timeout`, `--chunk-kib`, `--no-reset`
- [ ] More tunables: `--min-score`, `--types`; optional `tpt-usb-rescue.toml`
- [x] `image --hash` hashes the finished image into `<output>.sha256`; `verify` checks it (tested)
- [x] Per-file SHA-256 in the carve `index.csv`; `verify` checks it (older indexes still read)
- [x] `carve` and `analyze` say "N more" when they truncate their listings; names from images are printed with control characters removed
- [x] Shell completions (`completions <shell>`) and man pages (`man --out-dir`); both ship in the release archives (tested locally; the archive step has not run on GitHub)

## 11. CI and release automation
- [x] CI: macOS job, `--locked`, rust-cache, `cargo deny`, scheduled advisory check (`security.yml`), fuzz build-only job, `cargo doc -D warnings`, doc tests (workflows written; not yet run on GitHub)
- [ ] CI: run MSRV tests, not just `cargo check`
- [x] CI: push only on master, cancel superseded runs; dependabot added
- [x] Tag-triggered release workflow (`release.yml`): Windows zip with the GUI, Linux and macOS (Apple silicon and Intel) tarballs, SHA256SUMS, GitHub Release from the CHANGELOG; refuses a tag that does not match the crate version (not yet run: push a `v0.1.0-rc.1` tag to test it)
- [ ] Release: Linux aarch64 build, and a docs/release checklist update
- [x] `cargo-binstall` metadata (CLI crate)
- [ ] Decide `publish` for the GUI crate; consider release-plz
- [x] `.gitattributes` (`* text=auto eol=lf`)

## 12. Adoption: docs, examples, templates, packaging
- [x] README install section
- [ ] README: badges, GUI screenshot or GIF, 60-second quickstart with real output; fix "macOS later" (done) and add the GUI to the crate table (done)
- [x] Fix the stray `` `r`n `` in CHANGELOG.md line 11
- [x] `examples/`: `make-test-image` (bash and PowerShell; a noise image with hidden PNGs and a GIF), `recover.sh` / `recover.ps1` (image, analyze, carve, verify; resumes on re-run), chain-of-custody template (all run here)
- [ ] `examples/`: a practice image with a real FAT32/NTFS filesystem and deleted files (needs a way to build one without mkfs); sample config once a config file exists
- [x] docs: `docs/TROUBLESHOOTING.md`, exit-code table in USAGE.md
- [ ] docs: failing-drive playbook (physical handling, when to stop), worked walkthrough with real output
- [ ] Packaging: Scoop, winget, Homebrew tap, AUR, `.deb`, Docker recipe (`--device /dev/sdX`)
- [x] Issue and PR templates

## 13. Ideas (post-1.0)
- [ ] Recovery report (HTML/JSON): bad-sector heatmap, per-file intact/damaged score, hashes; doubles as chain of custody
- [ ] Adaptive imaging: health probe, speed-collapse detection, cool-down pauses
- [ ] Auto-resume when the device is plugged back in
- [ ] Browse the image read-only (FUSE/WinFsp or local web UI) before extracting
- [ ] Batch mode for repair shops (queue of devices, CSV/JSON summary)
- [ ] ddrescue mapfile import and `--compare` of two runs

## Decisions
- No external `tpt-*` crates (tpt-media, tpt-math, tpt-archon). Validation and hashing are implemented in-repo / with generic crates (blake3, sha2, memmap2).
- Dependency licenses: a crate is accepted when one of its licenses is permissive and listed in `deny.toml`. Crates offered only under Apache-2.0 are rejected.
