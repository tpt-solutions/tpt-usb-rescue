# Changelog

All notable changes to `tpt-usb-rescue-fs` are recorded here. The format follows Keep a
Changelog, and the crate uses semantic versioning once 1.0.0 is released.

## [Unreleased]

### Added
- NTFS parser: MFT records with fixups, resident and run-list data, the tree built from parent
  references, and deleted records.
- FAT32 and exFAT parsers: directory trees, LFN, deleted entries, and FAT chains and contiguous
  runs. exFAT directories stored without a FAT chain are read in full.
- Boot-sector volume detection (`detect`) that works without a partition table.
- MBR partition listing (`mbr_partitions`).

### Fixed
- Backup boot sectors are no longer reported as volumes.
- FAT32: long names are only used when their checksum matches the short entry that follows, and
  deleted multi-piece names keep their order.
- A cluster only exists if it lies inside the image, so a corrupt cluster count cannot cause a
  huge allocation (found by fuzzing the exFAT parser from a valid seed).
- Checked arithmetic and bounded allocations in the FAT32, exFAT and NTFS parsers.

### Known limitations
- Fragmented files are not stitched together from fs metadata.
- ext4, APFS and other filesystems are not parsed.
