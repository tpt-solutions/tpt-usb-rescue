# tpt-usb-rescue-hal

Cross-platform raw, cache-bypassing block device access for the
[tpt-usb-rescue](https://github.com/tpt-solutions/tpt-usb-rescue) recovery engine.

Status: **pre-alpha**. The Linux (`O_DIRECT`) and Windows (`FILE_FLAG_NO_BUFFERING`) backends
are compiled and reviewed but have not yet been run against physical USB media. macOS is
reported as unsupported for now.

## What it provides

- `RawDevice`: a read-only trait for sector-aligned reads and geometry queries.
- `open_image` / `open_device`: read-only handles for image files and physical devices.
- `list_devices`: enumerate candidate devices (`DeviceInfo`).
- `AlignedBuf`: buffers aligned for direct I/O.
- `check_output`, `check_not_system_disk`, `path_on_device`: safety guards that stop output
  from landing on the source disk or the system disk.

Every handle is opened read-only. Nothing in this crate writes to a device.

## Usage

```toml
[dependencies]
tpt-usb-rescue-hal = "0.1"
```

```rust
use tpt_usb_rescue_hal::{AlignedBuf, RawDevice, open_image};
use std::path::Path;

let dev = open_image(Path::new("recovery.img"))?;
let mut buf = AlignedBuf::new(4096);
// read sector-aligned data via the RawDevice trait
```

## License

Licensed under either of [Apache License, Version 2.0](../../LICENSE-APACHE) or
[MIT license](../../LICENSE-MIT) at your option.
