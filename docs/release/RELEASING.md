# Releasing

How a version gets from `master` to a GitHub Release. Nothing here runs by itself except the
workflow that a tag starts: pushing a tag is the one step that publishes, and it is yours to take.

## Version plan

- `0.1.0-rc.1`, `-rc.2`, ...: release candidates for testers ([TESTING.md](TESTING.md)). A tag with
  a hyphen is published as a **pre-release**.
- `1.0.0`: only after the release gate in [1.0.0-DRAFT.md](1.0.0-DRAFT.md) is complete (the real
  hardware checks).

The release workflow refuses a tag that does not match the version in the workspace `Cargo.toml`
(`v0.1.0-rc.1` needs `version = "0.1.0-rc.1"`, including the five path dependencies in
`[workspace.dependencies]`).

## Before tagging

Run these on the exact commit that will be tagged, from a clean checkout of it:

```sh
git status --short                        # nothing unexpected is uncommitted
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo +1.85 check --workspace --all-targets --locked     # the minimum supported Rust
cargo deny check
```

On Linux, as root: `sudo BIN=target/release/tpt-usb-rescue tools/linux-validate.sh` should end with
`RESULT: 15 passed, 0 failed`.

Then check by hand:

- [ ] The CI badge is green on `master` (all jobs: tests on three systems, docs, MSRV, deny, fuzz build).
- [ ] `CHANGELOG.md` has the changes under `## [Unreleased]`. The release notes are taken from the
      section for the version, or from `[Unreleased]` if there is no such section.
- [ ] `docs/release/TESTING.md` still states honestly what has and has not been tested.
- [ ] The README status line matches (see below).
- [ ] The version in `Cargo.toml` and the tag you are about to push are the same.

## Publishing a release candidate

```sh
git tag -a v0.1.0-rc.1 -m "0.1.0-rc.1"
git push origin v0.1.0-rc.1
```

The `Release` workflow then builds Windows (CLI and GUI), Linux and macOS (Apple silicon and Intel)
archives, adds `SHA256SUMS`, and creates the pre-release. Watch the run in the Actions tab.

Afterwards, check the pre-release page:

- [ ] Four archives plus `SHA256SUMS`; the notes show the changelog section.
- [ ] Download one archive, check it with `sha256sum -c SHA256SUMS --ignore-missing`, unpack it, and run
      `tpt-usb-rescue --version` and `examples/recover.sh` (or `.ps1`) on the practice images.
- [ ] The archive contains `man/`, `completions/` and `examples/` (the Intel macOS one has no
      `man/` or `completions/`: that build is cross-compiled and cannot run its own binary).

This workflow has not run yet. If it fails, nothing is published until the final job succeeds, so
fix the cause and push a new tag (`-rc.2` after bumping the version); delete the failed tag with
`git push origin :refs/tags/v0.1.0-rc.1`.

## Taking a bad release back

Delete the GitHub Release (Releases page, or `gh release delete <tag>`) and its tag
(`git push origin :refs/tags/<tag>`). Do not reuse the version: bump it, so no one holds two
different files with the same name.

## Promoting to 1.0.0

1. Complete the release gate in [1.0.0-DRAFT.md](1.0.0-DRAFT.md).
2. Set the workspace `version` and the five path dependencies to `1.0.0`; `cargo update --workspace`;
   in `fuzz/` run `cargo +nightly check` to refresh its lockfile.
3. In `CHANGELOG.md`, rename `[Unreleased]` to `[1.0.0] - <date>` and start a new empty
   `[Unreleased]`.
4. Move the notes in the 1.0.0 draft into the changelog section, delete the draft banner, and tag
   `v1.0.0`.

## Not set up yet

- **Publishing to crates.io.** Nothing is configured for it. The workspace crates depend on each
  other by path and version, so they would have to be published in dependency order (hal, imager,
  carver, fs, then `tpt-usb-rescue`). Decide first whether the GUI crate should be published at
  all (it has no `publish = false`).
- **Code signing.** The Windows and macOS binaries are unsigned: SmartScreen and Gatekeeper will
  warn. The release notes tell users to check `SHA256SUMS`.
- **Linux aarch64 and musl builds.**
- **Package-manager manifests** (Scoop, winget, Homebrew, AUR, `.deb`). They need a published
  release to point at, so add them after the first one.
