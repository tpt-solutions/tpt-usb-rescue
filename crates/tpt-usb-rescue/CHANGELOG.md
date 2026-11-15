# Changelog

All notable changes to the `tpt-usb-rescue` CLI are recorded here. The format follows Keep a
Changelog, and the crate uses semantic versioning once 1.0.0 is released.

## [Unreleased]

### Added
- `image` command with `--pass`, `--resume`, `--allow-system-disk`, `--mapfile`, `--timeout`,
  `--chunk-kib`, `--no-reset` and `--hash` (writes `<output>.sha256`, which `verify` checks).
- Live progress on stderr (`-q` to silence) and a clean stop on Ctrl-C (a second Ctrl-C quits at once).
- Documented exit codes: 0 complete, 2 usage, 3 stopped or device removed, 4 verify failed,
  5 finished with unrecovered data.
- `--json` for `image`, `carve`, `verify`, `list-devices`, `mapfile` and `analyze`.
- `carve --validate-media`; `carve` records a SHA-256 for each file in `index.csv`, and `verify` checks it.
- `analyze --extract`, with file names from the image sanitized.
- `verify`, `list-devices`, and `mapfile --export ddrescue|toml`.
- `completions <shell>` and `man [--out-dir DIR]`.
- End-to-end tests on synthetic images.

### Fixed
- `--resume` refuses a missing or truncated image.
- `analyze --extract` cannot write outside the output folder.
- `verify` rejects `index.csv` entries that name files outside the output folder.

### Known limitations
- Real USB sticks, failing drives and Windows physical drives have not been tested yet.
- A timed-out read on Linux re-enumerates the USB device (needs root). This is not yet tested on
  hardware, and Windows has no reset.
- `--min-score` and `--types` are not available yet.
