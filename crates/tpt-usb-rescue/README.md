# tpt-usb-rescue

Command-line forensic data recovery tool for degraded or corrupted USB mass storage.

Status: **pre-alpha**. The imaging, carving and filesystem code is implemented and tested on
synthetic data. Hardware paths are compiled but not yet validated on real failing media.

## Commands

- `list-devices`: list candidate devices. Physical drives usually need administrator or root.
- `image`: read a device or file into an image, with a crash-safe mapfile recording progress.
  Supports `--pass`, `--resume` and `--allow-system-disk`.
- `mapfile`: inspect a mapfile, or export it with `--export ddrescue|toml`.
- `analyze`: show the partition table, volumes and files, and optionally `--extract` them.
- `carve`: carve files from an image, with `--validate-media` to reject structurally invalid
  candidates.
- `verify`: check carved files against the source image.

## Usage

```sh
tpt-usb-rescue list-devices
tpt-usb-rescue image /dev/sdb recovery.img --mapfile recovery.map
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
- [`tpt-usb-rescue-fs`](../tpt-usb-rescue-fs): FAT32 and exFAT parsers.

## License

Licensed under either of [Apache License, Version 2.0](../../LICENSE-APACHE) or
[MIT license](../../LICENSE-MIT) at your option.
