# tpt-usb-rescue

Forensic data recovery engine (Rust) for degraded or corrupted USB mass storage.
Status: **pre-alpha scaffold** — see [spec.txt](spec.txt) and [TODO.md](TODO.md).

1. **Image** — Direct I/O, multi-pass, resumable via a crash-safe mapfile.
2. **Carve / parse** — zero-copy signature carving and raw FAT32/exFAT/NTFS parsing, with media validation.

## Workspace
| Crate | Purpose |
|---|---|
| `tpt-usb-rescue-hal` | Raw device access (Linux, Windows; macOS later) |
| `rescue-imager` | Multi-pass imager + mapfile |
| `rescue-carver` | Signature carver + validation |
| `rescue-fs` | Filesystem parsers |
| `tpt-usb-rescue` | CLI |

## License
Licensed under either of [Apache-2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT) at your option.
Copyright (c) 2026 TPT Solutions. Unless you state otherwise, contributions are dual-licensed as above.
