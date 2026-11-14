# Changelog

All notable changes to `tpt-usb-rescue-fs` are recorded here. The format follows Keep a
Changelog, and the crate uses semantic versioning once 1.0.0 is released.

## [Unreleased]

### Added
- FAT32 and exFAT parsers: directory trees, LFN, deleted entries, and FAT chains and contiguous
  runs.
- Boot-sector volume detection (`detect`) that works without a partition table.
- MBR partition listing (`mbr_partitions`).

### Known limitations
- NTFS is not parsed.
