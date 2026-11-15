# Fuzzing

Four libFuzzer targets exercise the parsers that read untrusted images: `fat32`, `exfat`, `ntfs`
and `carve`. They need nightly Rust and [cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz), and
run on Linux, macOS and WSL.

```sh
cargo install cargo-fuzz
cargo +nightly fuzz run ntfs fuzz/corpus/ntfs fuzz/seeds/ntfs -a -- -max_len=262144 -max_total_time=60
```

- **Seeds** (`fuzz/seeds/<target>/valid.img`) are small valid volumes and files. Random bytes almost
  never form a valid boot sector, so without them the fuzzer stays on the "reject" path and never
  reaches the directory and run-list code. Always pass the seeds directory.
- **`-max_len`**: the default input limit is 4096 bytes, which cuts the seed volumes short. Use
  262144.
- **`-a`** keeps debug assertions on, so arithmetic overflow is a crash rather than a silent wrap.
- `fuzz/corpus` and `fuzz/artifacts` are git-ignored. A crash is saved under `fuzz/artifacts/<target>`:
  reproduce it with `cargo +nightly fuzz run <target> fuzz/artifacts/<target>/<file>`, then turn it
  into a regression test.
- To rebuild the filesystem seeds after changing the test images:
  `cargo test -p tpt-usb-rescue-fs write_fuzz_seed -- --ignored`.

CI only builds the targets (`cargo fuzz build`); it does not run them.
