# Changelog

All notable changes to `tpt-usb-rescue-hal` are recorded here. The format follows Keep a
Changelog, and the crate uses semantic versioning once 1.0.0 is released.

## [Unreleased]

### Added
- Aligned buffers (`AlignedBuf`) for direct I/O.
- Read-only file/image and device handles (`open_image`, `open_device`, `FileDevice`).
- Linux backend using `O_DIRECT`, `BLKSSZGET` and `BLKGETSIZE64`, with `BLKGETSIZE64` encoded
  correctly on MIPS, PowerPC and SPARC.
- Linux USB device reset through sysfs `authorized` (`reset_device`), with retries and recovery advice.
- Windows backend using `FILE_FLAG_NO_BUFFERING` and `DeviceIoControl`.
- macOS backend using `/dev/rdiskN`, `F_NOCACHE` and DKIOC geometry (type-checked, not run on a Mac).
- Device listing (`list_devices`, `DeviceInfo`).
- Output and system-disk safety guards (`check_output`, `check_not_system_disk`, `path_on_device`).
  They cover partitions and volumes: LVM, LUKS, md and btrfs on Linux, slices on macOS, and
  `\\.\X:` volumes on Windows. Windows UNC output paths are refused.
- macOS system-disk guard follows APFS containers to their physical disks with `diskutil`, and refuses when `diskutil` cannot answer.

### Known limitations
- The Linux checks ran on loop and device-mapper devices, not on USB hardware.
- The Windows and macOS backends have not been run on physical drives.
- The macOS APFS guard (`diskutil`) has not been run on a Mac; its parser is unit-tested.
- Windows has no device reset.
