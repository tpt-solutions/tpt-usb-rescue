# Changelog

All notable changes are recorded here. The format follows Keep a Changelog, and the project
uses semantic versioning once 1.0.0 is released.

## [Unreleased]

## [0.1.0] - 2026-11-14

### Added
- Cargo workspace (edition 2024, MSRV 1.85) with crates `tpt-usb-rescue-hal`, `rescue-imager`,
  `rescue-carver`, `rescue-fs` and the `tpt-usb-rescue` CLI.
- Dual MIT / Apache-2.0 license files.
- CLI skeleton with `image`, `carve`, `analyze`, `verify`, `list-devices` and `mapfile` subcommands.
- CI workflow for Linux and Windows (fmt, clippy, tests) and an MSRV check.
