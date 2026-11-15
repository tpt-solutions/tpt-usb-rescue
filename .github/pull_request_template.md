## What and why

<!-- What does this change, and what problem does it solve? -->

## How it was tested

<!-- Tests added or run. For code that touches a real device, say what hardware or OS it ran on, or say plainly that it did not run on one. -->

## Checklist

- [ ] `cargo fmt --all` and `cargo clippy --workspace --all-targets -- -D warnings` are clean
- [ ] `cargo test --workspace` passes
- [ ] A bug fix has a test that fails without it
- [ ] Anything that reads untrusted image data uses checked arithmetic and bounded allocations
- [ ] User-visible changes are in `CHANGELOG.md` and, if needed, `docs/USAGE.md`
- [ ] The tool still only reads the source device (nothing writes to it)
