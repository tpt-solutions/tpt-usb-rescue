# Contributing to tpt-usb-rescue

Thanks for helping. This project recovers data from failing hardware, so correctness and
"never write to the source device" matter more than speed of development.

## Ground rules

- **The imager never writes to a source device.** Devices are opened read-only. Do not add code
  paths that open a device for writing.
- **Refuse dangerous targets.** Output must never land on the device being imaged, and the system
  disk must not be imaged without an explicit override.
- **No `unsafe` without a `// SAFETY:` comment.** The workspace denies `unsafe_op_in_unsafe_fn` and
  warns on `undocumented_unsafe_blocks`.
- **Validate, don't trust.** Parsers operate on untrusted bytes. They must return errors rather than
  panic, and must bounds-check every offset.

## Workflow

1. Fork and create a topic branch.
2. Make the change with tests. Parsers need a test for a valid input and at least one malformed input.
3. Run the same checks as CI:

   ```sh
   cargo fmt --all --check
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   ```

4. Update `CHANGELOG.md` under `Unreleased` and tick off the relevant item in `TODO.md`.
5. Open a pull request describing what was tested and on which platform. Hardware-dependent
   behaviour (real USB sticks, bad sectors) should be called out as untested unless you tried it.

## Licensing

By contributing you agree that your contribution is dual-licensed under MIT OR Apache-2.0, as
described in the README.
