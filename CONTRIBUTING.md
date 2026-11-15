# Contributing to tpt-usb-rescue

Contributions are handled through GitHub issues only. Please do not open pull requests; they will
not be merged.

- **Bug reports:** use the [bug report](https://github.com/tpt-solutions/tpt-usb-rescue/issues/new?template=bug_report.yml) template.
- **Feature requests:** use the [feature request](https://github.com/tpt-solutions/tpt-usb-rescue/issues/new?template=feature_request.yml) template.

When you report a problem, include the exact command, the tool version (`--version`), your operating
system, and the output. Do not attach files recovered from private drives.

## Ground rules

These apply to everything the project does, so they also shape what a good issue looks like:

- **The imager never writes to a source device.** Any behaviour that writes to a source device is a bug.
- **Output must never land on the device being imaged**, and the system disk must not be imaged
  without an explicit override.
- **Report, don't guess.** If a parser crashes or hangs on a malformed image, include the image or a
  description of its structure.

## Licensing

Contributions are dual-licensed under MIT OR Apache-2.0, as described in the README.
