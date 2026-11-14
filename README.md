# tpt-usb-rescue

Forensic data recovery engine (Rust) for degraded or corrupted USB mass storage.
Status: **pre-alpha**. The imaging, carving and filesystem code is implemented and tested on
synthetic data. Hardware paths are compiled but not yet validated on real failing media.
See [spec.txt](spec.txt) and [TODO.md](TODO.md).

1. **Image**: read-only, direct I/O, three passes, resumable via a crash-safe mapfile.
2. **Carve / parse**: zero-copy signature carving (20 file types, including PNG, JPEG, PDF, ZIP/DOCX, MP4, 7z and SQLite) with structural
   validation, and FAT32 / exFAT / NTFS parsing that works without a partition table.

## Usage

```sh
# List devices (physical drives usually need administrator or root)
tpt-usb-rescue list-devices

# Image a device or file; the mapfile records progress
tpt-usb-rescue image /dev/sdb recovery.img --mapfile recovery.map
tpt-usb-rescue image /dev/sdb recovery.img --mapfile recovery.map --resume
tpt-usb-rescue image /dev/sdb recovery.img --mapfile recovery.map --pass 1

# Inspect or export the map
tpt-usb-rescue mapfile recovery.map
tpt-usb-rescue mapfile recovery.map --export ddrescue --out recovery.ddr

# Partition table, volumes and files; optionally extract them
tpt-usb-rescue analyze recovery.img --extract ./volumes

# Carve files, rejecting structurally invalid candidates, then verify them
tpt-usb-rescue carve recovery.img ./carved --validate-media
tpt-usb-rescue verify ./carved recovery.img
```

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
| `tpt-usb-rescue-hal` | Raw read-only device access (Linux, Windows; macOS later) |
| `tpt-usb-rescue-imager` | Multi-pass imager + mapfile |
| `tpt-usb-rescue-carver` | Signature carver + validation + verification |
| `tpt-usb-rescue-fs` | FAT32, exFAT and NTFS parsers, volume detection |
| `tpt-usb-rescue` | CLI |

## Contributing
See [CONTRIBUTING.md](CONTRIBUTING.md). Report security issues as described in [SECURITY.md](SECURITY.md).

## License
Licensed under either of [Apache-2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT) at your option.
Copyright (c) 2026 TPT Solutions. Unless you state otherwise, contributions are dual-licensed as above.
