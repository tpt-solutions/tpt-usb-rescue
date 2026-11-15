# Changelog

All notable changes are recorded here. The format follows Keep a Changelog, and the project
uses semantic versioning once 1.0.0 is released.

## [Unreleased]

### Fixed
- Linux and macOS: imaging to an output file that did not exist yet failed the output-on-source check.
- `--resume` now refuses a missing or truncated image instead of recreating it as zeros the mapfile calls recovered.
- `analyze --extract` sanitizes file names from the image, so a hostile image cannot write outside the output folder.
- The system-disk and output-on-source guards now cover partitions and volumes, and resolve LVM, LUKS, md and btrfs to the physical disk (Linux), slices (macOS) and `\\.\X:` volumes (Windows).
- The imager syncs the image before the mapfile records data as recovered; short reads no longer leave sectors untested; device-removal error codes are per-OS.
- A damaged record in the middle of a mapfile no longer discards every later record.
- A run now stops after 10 consecutive timed-out reads instead of leaking reader threads, and the USB reset fires once per hang.
- exFAT directories stored without a FAT chain are read in full; backup boot sectors are no longer reported as volumes; NTFS, FAT32 and exFAT parsers use checked arithmetic and bounded allocations.
- The carver picks the right end record for ZIPs that contain stored ZIPs and caps files with no end marker at 256 MiB.
- `verify` rejects `index.csv` entries that name files outside the output folder; `mapfile --out` never overwrites.

### Added
- `--json` for `image`, `carve`, `verify`, `list-devices` and `mapfile`: one JSON object on stdout, messages on stderr. `carve` now records a SHA-256 for every file in `index.csv`, and `verify` checks it.
- `examples/`: practice-image and one-command recovery scripts (bash and PowerShell) and a chain-of-custody form; `docs/TROUBLESHOOTING.md`; issue and pull request templates.
- CLI: live progress on stderr (`-q` to silence), clean stop on Ctrl-C, documented exit codes (3 stopped, 4 verify failed, 5 unrecovered data), optional `--mapfile` (defaults to `<output>.map`), `--timeout`, `--chunk-kib`, `--no-reset` and `--hash` (writes `<output>.sha256`, checked by `verify`).
- Carver covers the top 20 file types, with structural end detection and validators.
- NTFS parser in `tpt-usb-rescue-fs` (MFT records, run lists, deleted records); detection and listing of NTFS volumes.
- macOS HAL (`/dev/rdiskN`, `F_NOCACHE`, DKIOC geometry): type-checked for macOS, not run on a Mac.
- USB device reset on Linux through sysfs `authorized` (needs root), called from the engine when a read times out.
- 5 s read timeout in the imager: reads run on a reader thread and a timed-out read is treated as a media failure.
- `deny.toml` license, ban and source policy; benchmarks; cargo-fuzz targets; library examples.
- `tpt-usb-rescue-gui`: Windows GUI built on the Win32 API with no third-party dependencies. Runs image, carve, verify and analyze on worker threads, with a progress bar and log.
- Shared operations moved into `tpt_usb_rescue::ops` so the CLI and GUI use the same code path.
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
- Reads time out after 5 s. On Linux a timed-out read re-enumerates the USB device (needs root); this is not yet tested on hardware, and Windows has no reset.


## [0.1.0] - 2026-11-14

### Added
- Cargo workspace (edition 2024, MSRV 1.85) with crates `tpt-usb-rescue-hal`, `tpt-usb-rescue-imager`,
  `tpt-usb-rescue-carver`, `tpt-usb-rescue-fs` and the `tpt-usb-rescue` CLI.
- Dual MIT / Apache-2.0 license files.
- CLI skeleton with `image`, `carve`, `analyze`, `verify`, `list-devices` and `mapfile` subcommands.
- CI workflow for Linux and Windows (fmt, clippy, tests) and an MSRV check.
