# Usage

tpt-usb-rescue has a command-line tool (`tpt-usb-rescue`) and a Windows window
(`tpt-usb-rescue-gui`). Both run the same operations and share the same safety checks.

## Before you start

- **Work on a copy of the evidence when you can.** Image the failing device first, then carve and
  verify the image. Carving never touches the device.
- **Permissions.** Opening a physical drive needs Administrator on Windows and root on Linux. macOS
  raw disks need root. Regular files need no special rights.
- **Free space.** The output image is the same size as the device. Put it on a different disk from
  the source. The tool refuses an output path on the source disk.
- **System disk.** Imaging the disk the OS runs from is refused unless you pass
  `--allow-system-disk` (CLI) or tick *Allow system disk* (GUI).

## Imaging a device

```sh
tpt-usb-rescue list-devices
tpt-usb-rescue image /dev/sdb recovery.img
```

The progress file defaults to the image name plus `.map` (`recovery.img.map`); give another path
with `--mapfile`. While it runs, a live line on stderr shows percent done, recovered and bad bytes,
speed and time left. Use `-q` / `--quiet` to turn it off. When stderr is not a terminal, one line
is printed every 30 seconds instead.

The three passes run in order:

1. **Sweep** reads everything in 1 MiB chunks. Chunks that fail are marked `NonTrimmed`.
2. **Retry** re-reads those chunks at 4 KiB, then sector by sector. Sectors that still fail are
   `BadSector`.
3. **Trim** tries the first and last sector of each bad run, and keeps going inward while reads
   succeed, so the bad area is as small as possible.

Run one pass with `--pass 1`, `--pass 2` or `--pass 3`.

Options for awkward drives:

| Option | Effect |
|---|---|
| `--timeout SECS` | How long a read may take before it counts as hung (default 5) |
| `--chunk-kib KIB` | Size of one pass-1 read (default 1024). Smaller reads isolate bad areas sooner |
| `--no-reset` | Do not re-enumerate the USB device after a hung read |
| `--hash` | After copying, hash the image and write `recovery.img.sha256` |

Every checkpoint syncs the image before the mapfile records a sector as recovered, so a crash or
power cut never leaves a map claiming data that is not on disk.

### Stopping and resuming

Press Ctrl-C once to stop cleanly: the run finishes its current block, saves progress and exits
with code 3. A second Ctrl-C quits at once (the map is still safe, but the current block is
re-read). The run also stops, with progress saved, if the device is unplugged or stops answering.
Continue with:

```sh
tpt-usb-rescue image /dev/sdb recovery.img --resume
```

`--resume` keeps the existing image and map and continues. Without it, the tool refuses to start
over a map that already records progress, so you cannot overwrite work by accident. It also
refuses to resume if the image is missing or shorter than the map expects, because the map would
then claim data that is no longer there.

### Stalled reads

A read that does not finish within 5 seconds (`--timeout`) counts as a media failure, and the run
carries on. On Linux the tool also asks the kernel to re-enumerate the USB device behind the disk
(needs root), once per hang. After a reset the device handle is invalid, so the run stops. Re-run
with `--resume`. If 10 reads in a row time out, the device is treated as dead and the run stops
with progress saved.

### Mapfile

The mapfile is a binary, append-only log. Each record has its own CRC, so a torn write at the end
is detected and dropped. A damaged record in the middle is skipped, and the sectors it described
are read again. Inspect the map or export it for GNU ddrescue or TOML:

```sh
tpt-usb-rescue mapfile recovery.map
tpt-usb-rescue mapfile recovery.map --export ddrescue --out recovery.ddr
tpt-usb-rescue mapfile recovery.map --export toml
```

`--out` never overwrites an existing file.

## Carving files from an image

```sh
tpt-usb-rescue carve recovery.img ./carved --validate-media
```

- Without `--validate-media` every candidate is written, and its score is recorded in
  `index.csv`.
- With `--validate-media` candidates scoring below 0.5 are rejected as false positives.
- Each file is named `file_<id>.<ext>`. `index.csv` lists offsets, sizes, type, score and the
  SHA-256 of each file. `verify` checks a file against that digest as well as against the image.
  (Indexes written by older versions have no digest column; they still verify.)

## Analyzing an image

```sh
tpt-usb-rescue analyze recovery.img
tpt-usb-rescue analyze recovery.img --extract ./volumes
```

`analyze` prints the MBR partition table (if any), then searches every sector for FAT32, exFAT and
NTFS boot sectors. It works even when the partition table is damaged. With `--extract`, files from
each volume are written out. Deleted entries are written with a `DELETED_` prefix.

## Verifying output

```sh
tpt-usb-rescue verify ./carved recovery.img
```

`verify` prints the BLAKE3 and SHA-256 digests of the image, then checks every carved file against
its byte range in the image. It exits with code 4 if any file differs or is missing. If the image
was made with `--hash`, `verify` also checks it against `recovery.img.sha256` and reports
`IMAGE CHANGED` if it no longer matches.

## Shell completions and man pages

The release archives include both, in `completions/` and `man/`. You can also generate them from
the program:

```sh
# Completions: bash, zsh, fish, elvish or powershell
tpt-usb-rescue completions bash > ~/.local/share/bash-completion/completions/tpt-usb-rescue
tpt-usb-rescue completions zsh  > ~/.zfunc/_tpt-usb-rescue
tpt-usb-rescue completions fish > ~/.config/fish/completions/tpt-usb-rescue.fish
tpt-usb-rescue completions powershell >> $PROFILE

# Manual pages: one for the tool and one per command (tpt-usb-rescue-image.1, ...)
tpt-usb-rescue man --out-dir ~/.local/share/man/man1
man tpt-usb-rescue-image
```

## Exit codes

| Code | Meaning |
|---|---|
| 0 | Success |
| 1 | Error (bad arguments for the data, I/O failure, refused by a safety check) |
| 2 | Usage error (unknown option, missing argument) |
| 3 | `image` stopped early with progress saved: Ctrl-C, device removed or unresponsive. Resume with `--resume` |
| 4 | `verify` found a carved file or the image that does not match |
| 5 | `image` finished a full run, but some sectors could not be recovered |

Code 5 is only returned when all three passes ran (no `--pass`, or `--pass 3`).

## Scripting with `--json`

`--json` makes `image`, `carve`, `verify`, `list-devices` and `mapfile` print their result as one
JSON object on a single line of stdout. All other messages (progress, per-pass lines, warnings) go
to stderr, so stdout can be piped straight into `jq` or a script:

```sh
tpt-usb-rescue --json image /dev/sdb recovery.img -q | jq '{ok, unrecovered_bytes, exit_code}'
tpt-usb-rescue --json verify ./carved recovery.img | jq '.files[] | select(.result != "ok")'
```

The exit code is the same as without `--json`. Every object has `ok` and, where a run can end in a
non-zero code, `exit_code`. If a command fails, you still get one object on stdout:
`{"ok":false,"error":"...","interrupted":false,"exit_code":1}`, with `interrupted` true for exit
code 3.

| Command | Fields |
|---|---|
| `image` | `output`, `mapfile`, `recovered_bytes`, `bad_bytes`, `non_trimmed_bytes`, `untested_bytes`, `unrecovered_bytes`, `hash_recorded`, `exit_code` |
| `carve` | `index`, `count`, `intact`, `files[]` with `id`, `file`, `type`, `start_offset`, `end_offset`, `size_bytes`, `integrity_score` |
| `verify` | `image_blake3`, `image_sha256`, `image_hash` (`matches`, `changed` or `not_recorded`), `checked`, `failed`, `files[]` with `id`, `file`, `result` (`ok`, `mismatch` or `missing`), `exit_code` |
| `list-devices` | `devices[]` with `path`, `size_bytes`, `removable` (true, false or null when unknown) |
| `mapfile` | `path`, `sector_size`, `sector_count`, `replayed_records`, `skipped_records`, `recovered_bytes`, `bad_bytes`, `non_trimmed_bytes`, `untested_bytes`, `bad_runs[]` with `start_sector`, `sectors` |

`analyze` and `mapfile --export` have no JSON form: with `--json` they exit with code 2. Fields
may be added in later versions; do not rely on a fixed set of keys.

## The Windows GUI

Run `tpt-usb-rescue-gui.exe`. The window is a four-step guide, from top to bottom:

- **Top line:** says whether the program runs as Administrator. Disks can only be read when it does.
  If it says NO, close the program, right-click it and choose *Run as administrator*.
- **Step 1: Choose what to copy.** Pick your USB stick in *Found disks* (press *Refresh* after you
  plug it in), or browse for a file to practise with.
- **Step 2: Choose where to save the copy.** Type a file name or press *Browse*. Use a different disk
  from the stick. *Progress notes* are created for you; keep them next to the copy.
- **Step 3: Copy the disk.** Press *Start copy*. You are asked to confirm first. The stick is only
  read. Press *Stop* at any time; your progress is saved. To carry on later, tick *Continue a previous
  run* and press *Start copy* again.
- **Step 4: Find files (optional).** *List files in the copy* shows what Windows can read.
  *Find files in the copy* saves pictures, documents and other files into a `carved` folder next to
  the copy. *Check found files* confirms the saved files match the copy.

Every control has a tooltip: rest the mouse on it. *Help: how do I use this?* shows the same steps
in a dialog. Every failure explains what went wrong and what to try next.
## Testing without hardware

The quickest way to try the tool is the practice image in [examples/](../examples/README.md): two
scripts build one with no extra software, and `recover.sh` / `recover.ps1` run a whole recovery on
it. Something went wrong? See [Troubleshooting](TROUBLESHOOTING.md).

For developers, the repository also contains tests and examples on synthetic data:

```sh
cargo test --workspace
cargo run -p tpt-usb-rescue-carver --example carve_file -- some.img
cargo run -p tpt-usb-rescue-fs --example list_volumes -- some.img
cargo bench -p tpt-usb-rescue-carver
```

Imaging a regular file works like imaging a device, so you can copy any file as the source:

```sh
tpt-usb-rescue image source.bin copy.img --mapfile copy.map
```
