# tpt-usb-rescue-gui

Windows GUI for [tpt-usb-rescue](https://github.com/tpt-solutions/tpt-usb-rescue). It is a plain
Win32 window with no third-party GUI dependencies, and it runs the same operations as the
command-line tool on worker threads.

Status: **release candidate 0.1.0-rc.1**. It builds and shares the CLI's imaging code, and the
file-source paths are tested through the CLI. The window has **not yet been driven with real
drives** (hardware check H6 in [TESTING.md](../../docs/release/TESTING.md)).

## Using it

Run `tpt-usb-rescue-gui.exe`. The window is a four-step guide:

1. **Choose what to copy.** Pick a USB stick in *Found disks* (press *Refresh* after plugging it
   in), or browse for a file to practise with.
2. **Choose where to save the copy.** Use a different disk from the stick.
3. **Copy the disk.** *Start copy* asks for confirmation. *Stop* saves progress; tick *Continue a
   previous run* to resume.
4. **Find files (optional).** List the files Windows can read, find and save files into a
   `carved` folder, or check the saved files against the copy.

Physical drives need the window started as Administrator. Every control has a tooltip, and
*Help: how do I use this?* shows the steps in a dialog. The Windows binaries are not code-signed,
so SmartScreen may warn; check the release's `SHA256SUMS`.

The full walkthrough is in [docs/USAGE.md](../../docs/USAGE.md#the-windows-gui).

## Building

Built as part of the workspace. On other platforms the binary prints a message and exits.

```sh
cargo build --release -p tpt-usb-rescue-gui
```

## License

Licensed under either of [Apache License, Version 2.0](../../LICENSE-APACHE) or
[MIT license](../../LICENSE-MIT) at your option.
