# tpt-usb-rescue

Command-line forensic data recovery tool for degraded or corrupted USB mass storage.

Status: **release candidate 0.1.0-rc.1**. The imaging, carving and filesystem code is tested on
synthetic data and on real Linux block devices. Real USB sticks, failing drives and Windows
physical drives are **not yet tested**; see [docs/release/TESTING.md](../../docs/release/TESTING.md).

## Commands

- `list-devices`: list candidate devices. Physical drives usually need administrator or root.
- `image`: read a device or file into an image, with a crash-safe mapfile recording progress.
  Supports `--pass`, `--resume`, `--allow-system-disk`, `--mapfile`, `--timeout`, `--chunk-kib`,
  `--no-reset` and `--hash`.
- `mapfile`: inspect a mapfile, or export it with `--export ddrescue|toml`.
- `analyze`: show the partition table, volumes and files, and optionally `--extract` them.
- `carve`: carve files from an image, with `--validate-media` to reject structurally invalid
  candidates.
- `verify`: check carved files, and the image hash written by `image --hash`, against the source.
- `completions <shell>`: print shell completions.
- `man [--out-dir DIR]`: write manual pages.

Most commands accept `--json` (one JSON object on stdout, messages on stderr) and `-q` (silence
progress). Exit codes are listed in [docs/USAGE.md](../../docs/USAGE.md#exit-codes).

## Usage

```sh
tpt-usb-rescue list-devices
tpt-usb-rescue image /dev/sdb recovery.img --mapfile recovery.map --hash
tpt-usb-rescue image /dev/sdb recovery.img --mapfile recovery.map --resume
tpt-usb-rescue mapfile recovery.map --export ddrescue --out recovery.ddr
tpt-usb-rescue analyze recovery.img --extract ./volumes
tpt-usb-rescue carve recovery.img ./carved --validate-media
tpt-usb-rescue verify ./carved recovery.img
```

The imager opens sources read-only. It refuses an output path that is a device node or lives on
the source disk, and refuses the system disk unless `--allow-system-disk` is given.

## Library crates

The engine is split into crates that can be used on their own:

- [`tpt-usb-rescue-hal`](../tpt-usb-rescue-hal): raw read-only device access.
- [`tpt-usb-rescue-imager`](../tpt-usb-rescue-imager): multi-pass imager and mapfile.
- [`tpt-usb-rescue-carver`](../tpt-usb-rescue-carver): signature carving and validation.
- [`tpt-usb-rescue-fs`](../tpt-usb-rescue-fs): FAT32, exFAT and NTFS parsers.

## License

Licensed under either of [Apache License, Version 2.0](../../LICENSE-APACHE) or
[MIT license](../../LICENSE-MIT) at your option.
