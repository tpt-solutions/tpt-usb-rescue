# Architecture

tpt-usb-rescue is a set of small crates. Each one has one job, and the dependency direction is
one-way: the CLI and GUI sit on top of the libraries, and the libraries never depend on the
front ends.

```
tpt-usb-rescue-gui ─┐
                    ├─► tpt-usb-rescue (ops) ─► tpt-usb-rescue-imager ─► tpt-usb-rescue-hal
tpt-usb-rescue CLI ─┘                       ├─► tpt-usb-rescue-carver
                                            └─► tpt-usb-rescue-fs
```

## Crates

| Crate | Responsibility |
|---|---|
| `tpt-usb-rescue-hal` | Read-only device access: Linux (`O_DIRECT`), Windows (`FILE_FLAG_NO_BUFFERING`), macOS (`F_NOCACHE`). Device listing, safety guards, USB reset on Linux. |
| `tpt-usb-rescue-imager` | Three-pass rescue engine, the binary mapfile, the image writer. |
| `tpt-usb-rescue-carver` | Signature table, end-of-file walkers, validators, carving, extraction, hashing and verification. |
| `tpt-usb-rescue-fs` | FAT32, exFAT and NTFS parsers that work on byte slices, and volume detection by boot-sector scan. |
| `tpt-usb-rescue` | `ops` (the operations, writing reports to any `Write`) and the `tpt-usb-rescue` CLI. |
| `tpt-usb-rescue-gui` | Win32 window, with no third-party dependencies. Runs `ops` on worker threads. |

## Imaging

The engine works on sectors. A sector's state lives in the mapfile as one of four values:
`Untested`, `Recovered`, `NonTrimmed` (a pass-1 chunk failed) or `BadSector` (failed at sector
size).

- Reads go through a reader thread. The engine waits with a timeout (5 s by default). A timed-out
  read counts as a media failure, and the reader is replaced. On Linux a reset hook also
  re-enumerates the USB device.
- Each checkpoint syncs the image first, then appends the mapfile records. The map therefore never
  claims data that is not on disk.
- Device-removal errors stop the run cleanly and keep the progress.
- The image writer skips all-zero blocks when the image is new, so bad areas stay sparse.

### Mapfile format

```
header (24 bytes):  "TPTMAP01" | sector_size u32 | sector_count u64 | crc32 u32
record (20 bytes):  start_sector u64 | length_sectors u32 | status u8 | reserved [u8;3] | crc32 u32
```

Records are appended, and a later record overrides an earlier one for the sectors it covers.
Replay on open stops at the first record with a bad CRC or a short length, and truncates the file
there.

## Carving

1. Every signature in `SIGNATURES` is searched for in the data. A signature may also require a
   secondary tag at a fixed offset (WEBP, WAV and AVI all start with `RIFF`), and may start before
   its match (MP4's `ftyp` sits four bytes into the file).
2. Hits are sorted by start offset. A hit inside an already-carved file is skipped.
3. The end of each file comes from its structure where that is cheap (see `docs/FORMATS.md`).
   A candidate that never closes is cut at the next signature hit, so a stray header cannot hide
   the real files after it.
4. The validator scores the candidate as the fraction of format checks it passes. Candidates below
   0.5 are rejected when validation is on.

The carver works on a `&[u8]`. The CLI memory-maps the image, so the carver does not copy it.

## Filesystems

Each parser reads the boot sector and metadata from a byte slice, and bounds-checks every offset.
Malformed input produces an error or a partial result, never a panic. The parsers are covered by
property tests that mutate valid volumes, and by cargo-fuzz targets in `fuzz/`.

- **FAT32.** Follows FAT chains, with cycle detection. Reads LFN entries. Deleted entries are
  listed, and read as a contiguous run because the chain has been freed.
- **exFAT.** Reads file sets and checks the NoFatChain flag. Deleted entries are listed the same
  way as in FAT32.
- **NTFS.** Applies MFT update-sequence fixups, decodes resident and run-list data, and builds the
  tree from `$FILE_NAME` parent references over the whole MFT, not from index blocks. A record
  whose fixup fails is dropped.

## Safety model

- Every device is opened read-only. Nothing in the crates opens a source device for writing.
- `check_output` refuses an output path that is a device node or lives on the source device.
- `check_not_system_disk` refuses the disk that hosts the running system unless overridden.
- Both checks are in `ops`, so the CLI and the GUI both run them.

## Platforms

| Platform | Device access | Reset | Status |
|---|---|---|---|
| Windows | `\\.\PhysicalDriveN`, Administrator | none | Compiled, tested on synthetic data |
| Linux | `/dev/sdX`, root | USB re-enumeration | HAL and imager clippy clean for Linux; tested on synthetic data on Windows only |
| macOS | `/dev/rdiskN`, root | none | Type-checked and clippy clean only |

No platform has been run against failing hardware yet.
