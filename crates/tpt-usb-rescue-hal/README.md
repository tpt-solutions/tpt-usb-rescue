# tpt-usb-rescue-hal

Cross-platform raw, cache-bypassing block device access for the
[tpt-usb-rescue](https://github.com/tpt-solutions/tpt-usb-rescue) recovery engine.

Status: **release candidate 0.1.0-rc.1**. The Linux (`O_DIRECT`) backend has been run on real
Linux block devices (loop and device-mapper), not on USB hardware. The Windows
(`FILE_FLAG_NO_BUFFERING`) backend is compiled but not run on a physical drive. The macOS
(`/dev/rdiskN`, `F_NOCACHE`) backend is type-checked but has never run on a Mac.

## What it provides

- `RawDevice`: a read-only trait for sector-aligned reads and geometry queries.
- `open_image` / `open_device`: read-only handles (`FileDevice`) for image files and physical devices.
- `list_devices`: enumerate candidate devices (`DeviceInfo`).
- `reset_device`: USB re-enumeration on Linux through sysfs `authorized` (needs root). Windows and
  macOS have no reset.
- `AlignedBuf`: buffers aligned for direct I/O.
- `check_output`, `check_not_system_disk`, `path_on_device`: safety guards that stop output
  from landing on the source disk or the system disk. Linux resolves LVM, LUKS, md and btrfs to
  the physical disk; macOS and Windows check slices and `\\.\X:` volumes. On macOS the guard also
  follows APFS containers to their physical disks with `diskutil`; that has not been run on a Mac.

Every handle is opened read-only. Nothing in this crate writes to a device.

## Usage

This crate is not yet published to crates.io. Depend on it by path or git until it is.

```rust
use tpt_usb_rescue_hal::{AlignedBuf, RawDevice, open_image};
use std::path::Path;

let dev = open_image(Path::new("recovery.img"))?;
let mut buf = AlignedBuf::new(4096, 512)?; // length, alignment
// read sector-aligned data via the RawDevice trait
```

## License

Licensed under either of [Apache License, Version 2.0](../../LICENSE-APACHE) or
[MIT license](../../LICENSE-MIT) at your option.
