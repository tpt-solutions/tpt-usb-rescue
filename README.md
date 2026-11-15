# tpt-usb-rescue

Forensic data recovery engine (Rust) for degraded or corrupted USB mass storage.
Status: **pre-alpha**. The imaging, carving and filesystem code is implemented and tested on
synthetic data. Hardware paths are compiled but not yet validated on real failing media.
See [spec.txt](spec.txt) and [TODO.md](TODO.md).

1. **Image**: read-only, direct I/O, three passes, resumable via a crash-safe mapfile.
2. **Carve / parse**: zero-copy signature carving (20 file types, including PNG, JPEG, PDF, ZIP/DOCX, MP4, 7z and SQLite) with structural
   validation, and FAT32 / exFAT / NTFS parsing that works without a partition table.

## Install

Prebuilt binaries for Windows (CLI and GUI), Linux and macOS are attached to each
[GitHub Release](https://github.com/tpt-solutions/tpt-usb-rescue/releases). Download the archive
for your system, check it against `SHA256SUMS`, and unpack it. Or build from source:

```sh
cargo install --locked --git https://github.com/tpt-solutions/tpt-usb-rescue tpt-usb-rescue
# once releases exist, this fetches the prebuilt binary instead of compiling:
cargo binstall tpt-usb-rescue
```

Reading a raw disk needs administrator rights on Windows and root on Linux and macOS. Shell
completions and man pages: `tpt-usb-rescue completions <shell>`, `tpt-usb-rescue man --out-dir <dir>`
(see [Usage](docs/USAGE.md#shell-completions-and-man-pages)).

## Usage

```sh
# List devices (physical drives usually need administrator or root)
tpt-usb-rescue list-devices

# Image a device or file. Progress is saved in recovery.img.map; Ctrl-C stops cleanly
tpt-usb-rescue image /dev/sdb recovery.img
tpt-usb-rescue image /dev/sdb recovery.img --resume
tpt-usb-rescue image /dev/sdb recovery.img --hash   # also writes recovery.img.sha256

# Inspect or export the map
tpt-usb-rescue mapfile recovery.img.map
tpt-usb-rescue mapfile recovery.img.map --export ddrescue --out recovery.ddr

# Partition table, volumes and files; optionally extract them
tpt-usb-rescue analyze recovery.img --extract ./volumes

# Carve files, rejecting structurally invalid candidates, then verify them
tpt-usb-rescue carve recovery.img ./carved --validate-media
tpt-usb-rescue verify ./carved recovery.img
```

Exit codes: 0 success, 3 stopped with progress saved (resume with `--resume`), 4 verification
failed, 5 finished with unrecoverable sectors. See [Usage](docs/USAGE.md#exit-codes).

Safety: the imager opens sources read-only. It refuses an output path that is a device node or
lives on the source disk, and refuses the system disk unless `--allow-system-disk` is given.

## GUI (Windows)

`target\release\tpt-usb-rescue-gui.exe` is a plain Win32 window with no third-party GUI
dependencies. It runs the same operations as the CLI: pick a source device or file, an image path,
then Start imaging, Carve, Verify or Analyze. Progress and results appear in the log. Physical drives
need a window started as Administrator.
## Workspace
| Crate | Purpose |
|---|---|
| `tpt-usb-rescue-hal` | Raw read-only device access (Linux, Windows, macOS) |
| `tpt-usb-rescue-imager` | Multi-pass imager + mapfile |
| `tpt-usb-rescue-carver` | Signature carver + validation + verification |
| `tpt-usb-rescue-fs` | FAT32, exFAT and NTFS parsers, volume detection |
| `tpt-usb-rescue` | CLI |
| `tpt-usb-rescue-gui` | Windows GUI |

## Documentation

- [Usage](docs/USAGE.md): imaging, resume, carving, analysis, verification, exit codes and the GUI
- [Troubleshooting](docs/TROUBLESHOOTING.md): common errors, slow drives, what the exit codes mean
- [Examples](examples/README.md): practice image, one-command recovery scripts (bash and PowerShell), chain-of-custody form
- [Architecture](docs/ARCHITECTURE.md): crates, mapfile format, carving and filesystem design, safety model
- [Supported formats](docs/FORMATS.md): the 20 carved types and the filesystems read
- [Release notes draft for 1.0.0](docs/release/1.0.0-DRAFT.md) (not published)
## Contributing
See [CONTRIBUTING.md](CONTRIBUTING.md). Report security issues as described in [SECURITY.md](SECURITY.md).

## License
Licensed under either of [Apache-2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT) at your option.
Copyright (c) 2026 TPT Solutions. Unless you state otherwise, contributions are dual-licensed as above.
