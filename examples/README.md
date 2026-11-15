# Examples

Everything here works with the downloaded program; nothing needs Rust or a build.

| File | What it does |
|---|---|
| `make-test-image.sh` / `.ps1` | Builds a small practice image (noise with two PNGs and a GIF hidden in it), so you can try the tool without a drive |
| `recover.sh` / `.ps1` | The whole recovery in one command: image, analyze, carve, verify. Safe to run again to resume |
| `chain-of-custody.md` | A form to fill in when the recovery may be needed as evidence |

## Try it in two minutes

Linux or macOS:

```sh
./make-test-image.sh test.img
./recover.sh test.img ./try-it
ls try-it/carved        # file_000001.png  file_000002.gif  file_000003.png  index.csv
```

Windows (PowerShell):

```powershell
.\make-test-image.ps1 -Out test.img
.\recover.ps1 test.img .\try-it
dir .\try-it\carved
```

If the tool is not on your `PATH`, point the scripts at it: `TPT=./tpt-usb-rescue ./recover.sh ...`
or `$env:TPT = ".\tpt-usb-rescue.exe"`.

You should see three carved files, each with a score of 1.00, and `verify` ending in
`3 checked, 0 failed`.

## Doing it for real

```sh
sudo ./recover.sh /dev/sdb ~/rescue/usb1
```

```powershell
.\recover.ps1 \\.\PhysicalDrive2 D:\rescue\usb1     # in a window started as Administrator
```

- Find the device with `tpt-usb-rescue list-devices`. Check the size against the drive you mean:
  the tool only reads the source, but you want the right one.
- Put the work folder on a **different** disk with room for the whole drive. The tool refuses an
  output on the source disk.
- Extra options go after the work folder and are passed to `image`, for example
  `--timeout 10 --chunk-kib 256` for a drive that hangs. See [docs/USAGE.md](../docs/USAGE.md).
- If the run stops (Ctrl-C, unplugged drive), run the same command again: it resumes from the
  progress file. Exit code 3 means "stopped, resumable"; 5 means "finished, but some sectors could
  not be read".

## What the scripts leave behind

```
rescue/usb1/
  recovery.img          the copy of the drive (unreadable sectors are zeros)
  recovery.img.map      which sectors were recovered or bad
  recovery.img.sha256   the hash of the finished image
  volumes/              files read from the filesystem, when one could be found
  carved/               files found by signature, plus index.csv (type, offsets, score)
```

Work from the image, not the drive. Do not mount, repair or `chkdsk` the failing drive: every
extra read can make it worse.

Problems? See [docs/TROUBLESHOOTING.md](../docs/TROUBLESHOOTING.md).
