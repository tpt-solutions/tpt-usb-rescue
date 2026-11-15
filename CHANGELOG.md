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
- Hung reads: the USB reset fires once per hang, and after 5 timed-out reads in a row the run checks that the drive can still read a sector it already recovered. If it can, the run carries on past the hung area; if not, it stops with progress saved. (A first version gave up on any hung area longer than a few sectors; real device-mapper testing caught that.)
- exFAT directories stored without a FAT chain are read in full; backup boot sectors are no longer reported as volumes; NTFS, FAT32 and exFAT parsers use checked arithmetic and bounded allocations.
- The carver picks the right end record for ZIPs that contain stored ZIPs and caps files with no end marker at 256 MiB.
- `verify` rejects `index.csv` entries that name files outside the output folder; `mapfile --out` never overwrites.
- exFAT and FAT32: a cluster only exists if it lies inside the image, so a corrupt cluster count can no longer make a huge allocation (found by fuzzing the exFAT parser from a valid seed).
- FAT32: long names are only used when their checksum matches the short entry that follows, and deleted multi-piece names keep their order.
- A short read queues the unread tail for the next pass instead of dropping it.
- The resume guard counts any recorded progress, not only recovered and bad sectors.
- Windows: UNC output paths are refused instead of being matched to a made-up drive letter.
- `mapfile --export --out` never overwrites an existing file, including the mapfile itself.
- The USB reset retries re-authorizing the device and tells the user how to recover if it still fails.
- `BLKGETSIZE64` is encoded correctly on MIPS, PowerPC and SPARC.
- Pass 3 wording in the docs now matches what the code does (it trims inward from the edges of a bad run).
- macOS system-disk guard: on an APFS Mac the root volume sits on a synthesized container disk, which the device-number check could not see. The guard now asks `diskutil` which physical disks back a volume, and refuses when `diskutil` cannot answer. Type-checked and parser-tested; not run on a Mac.

### Added
- `tools/linux-validate.sh`: 15 checks on real Linux block devices (loop and device-mapper), covering unreadable sectors, hung reads, Ctrl-C with `--resume`, and the safety guards. It found and fixed the hung-area abort.
- Release preparation: `docs/release/TESTING.md` (tester guide and hardware checklist), `docs/release/RELEASING.md` (process), and a tag-triggered release workflow that refuses a tag that does not match the crate version.
- `tpt-usb-rescue-gui` has its own README and CHANGELOG.
- `completions <shell>` and `man [--out-dir DIR]` generate shell completions and manual pages; the release archives include them and the examples.
- `--json` for `image`, `carve`, `verify`, `list-devices` and `mapfile`: one JSON object on stdout, messages on stderr. `carve` now records a SHA-256 for every file in `index.csv`, and `verify` checks it.
- `examples/practice-fat32.img`: a small FAT32 volume with a deleted file to try `analyze --extract` on.
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
- `analyze --json`: partitions, volumes and their entries as one JSON object on stdout.
- `examples/practice-exfat.img` and `examples/practice-ntfs.img`: practice volumes for the exFAT and NTFS parsers, each with a deleted file.
- Tests for pass-3 edge recovery in the imager, and for a hostile file name from an image, checked end to end.

### Changed
- `tpt-usb-rescue-imager` and `tpt-usb-rescue-carver` expose real types instead of the scaffold placeholders.
  `BlockRegion::length_sectors` is now `u64`.
- Workspace version is `0.1.0-rc.1`. Contributions are through GitHub issues only (pull requests are not accepted).

### Known limitations
- Real USB sticks, failing drives and Windows physical drives have not been tested yet. The Linux checks use loop and device-mapper devices, not hardware. The full hardware checklist is in [docs/release/TESTING.md](docs/release/TESTING.md).
- Reads time out after 5 s. On Linux a timed-out read re-enumerates the USB device (needs root); this is not yet tested on hardware, and Windows has no reset.
- macOS has never run. The system-disk guard asks `diskutil` which physical disks back an APFS volume; that code is type-checked and parser-tested, not run on a Mac. Do not image the disk macOS boots from.
- Carving is contiguous only: fragmented files are not reassembled.
- `--min-score` and `--types` are not available yet.
