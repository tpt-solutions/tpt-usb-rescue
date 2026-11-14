# Changelog

All notable changes to `tpt-usb-rescue-hal` are recorded here. The format follows Keep a
Changelog, and the crate uses semantic versioning once 1.0.0 is released.

## [Unreleased]

### Added
- Aligned buffers (`AlignedBuf`) for direct I/O.
- Read-only file/image and device handles (`open_image`, `open_device`, `FileDevice`).
- Linux backend using `O_DIRECT`, `BLKSSZGET` and `BLKGETSIZE64`.
- Windows backend using `FILE_FLAG_NO_BUFFERING` and `DeviceIoControl`.
- Device listing (`list_devices`, `DeviceInfo`).
- Output and system-disk safety guards (`check_output`, `check_not_system_disk`, `path_on_device`).

### Known limitations
- Hardware paths have not been run against physical USB media yet.
- macOS reports `HalError::Unsupported`.
