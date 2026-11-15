# Changelog

All notable changes to `tpt-usb-rescue-gui` are recorded here. The format follows Keep a
Changelog, and the crate uses semantic versioning once 1.0.0 is released.

## [Unreleased]

### Added
- Windows GUI on the Win32 API, with no third-party dependencies.
- A four-step window: choose the source, choose the output, copy the disk, then find files.
- Image, carve, verify and analyze run on worker threads, with a progress bar and a log.
- *Stop* during imaging, with progress saved for `--resume`.
- Confirmation before copying, a tooltip on every control, and a *Help* dialog.
- Shows whether the program runs as Administrator.
- Refuses to run on other platforms, with a message that points to the CLI.

### Known limitations
- Not yet driven with real USB sticks or physical drives.
- Windows binaries are not code-signed.
- Physical drives need the window to be started as Administrator.
