# Testing guide

For people trying a pre-release build (`0.1.0-rc.1`). It says what has already been checked, what
has **not**, and exactly what to try and report. The tool only ever *reads* the device you point it
at, but it is new software: use a drive you can afford to lose access to, and keep other copies of
anything important.

## Where the project stands

| Area | Status | How it was checked |
|---|---|---|
| Imaging logic: passes, mapfile, resume, bad-sector map | Verified on synthetic data and on real Linux block devices | Unit and end-to-end tests; `tools/linux-validate.sh` (loop and device-mapper devices) |
| Unreadable sectors | Verified on Linux: exactly the bad sectors are reported and zero-filled, everything else is byte-identical | device-mapper `error` target |
| Hung reads | Verified on Linux: a hung area is survived and the run finishes by itself | device-mapper `delay` target |
| Ctrl-C and `--resume` | Verified on Linux: exit code 3, then a byte-identical image | device-mapper `delay` target |
| Safety guards | Verified on Linux: output on the imaged disk, output on one of its partitions, and the disk that hosts `/` are all refused | loop devices, partitions, the real root disk |
| Carver and filesystem parsers | Verified on synthetic files and volumes; fuzzed (see `fuzz/README.md`) | Tests, libFuzzer |
| **Real USB sticks and failing drives** | **Not yet tested** | |
| **Windows physical drives** (`\\.\PhysicalDriveN`) | **Not yet tested.** The code builds and the file-source paths are tested | |
| **macOS** | **Not yet tested.** It builds; it has never run on a Mac | |
| **Linux USB reset after a hang** | **Not yet tested.** Needs root and a real USB device | |
| **Windows GUI** | **Not exercised in the latest review.** It builds and shares the CLI's imaging code, but nobody has driven the window with real drives | |
| Carving quality on real-world images | Not compared with a reference tool | |

## Five-minute check (no hardware)

Unpack the release archive, then from its folder:

```sh
./examples/make-test-image.sh test.img          # or .\examples\make-test-image.ps1
./examples/recover.sh test.img ./try-it         # or .\examples\recover.ps1 test.img .\try-it
./examples/recover.sh examples/practice-fat32.img ./try-fat
```

Expected: three carved files with score 1.00 and `3 checked, 0 failed`; and, for the FAT32 image,
five extracted files including `DELETED__ECRET.DAT`. See [examples/README.md](../../examples/README.md).

## Linux: automated device checks (needs root)

```sh
sudo BIN=./tpt-usb-rescue tools/linux-validate.sh
```

It needs `losetup`, `dmsetup` (with the `linear`, `error` and `delay` targets), `sfdisk`,
`mkfs.ext4` and `python3`, and expects `RESULT: 15 passed, 0 failed`. Please run it on your
distribution and report any failure with the full output. It touches only loop and device-mapper
devices it creates itself.

## Real hardware checklist

For each case, record the tool version (`tpt-usb-rescue --version`), your operating system, the
drive (type, size, make), the exact command, the exit code, and the output of
`tpt-usb-rescue mapfile <image>.map`.

**H1. A healthy USB stick (Windows, Linux, macOS).** Image it with `--hash`. The progress line shows
the speed. Compare with what you get from `dd` (Linux, macOS) or another imaging tool, and confirm
the result: `sha256sum /dev/sdX` (or the platform's tool) of the *device* should equal the image's
hash in `recovery.img.sha256`. Expected: exit 0, identical hashes, speed in the same range as the
other tool. Report the speed.

**H2. A stick or card with real read errors.** Image it. Expected: the run finishes without hanging
the operating system, exit 5 if some sectors could not be read, and `mapfile` lists the bad runs.
Check that files outside the bad runs open correctly.

**H3. Unplug during a run, then resume.** Start imaging, pull the device after a few seconds. Expected:
the run stops with exit 3 and `device removed` (or `stopped responding`), no crash. Plug it back in,
check the device name, and repeat the command with `--resume`. Expected: it completes, and the
image hash equals the device hash. This is the check that no sector is recorded as recovered that
is not on disk.

**H4. Linux only, as root: a device that hangs.** If you have a stick that hangs on reads, image it
and note whether the message `USB reset failed` appears, whether the device re-appears, and whether
the run continues. A device you can make hang on purpose is especially useful.

**H5. Carving a real image.** Carve an image of a card that held photos or documents, with
`--validate-media`, and compare the count of intact files with a reference tool (PhotoRec,
foremost). Report both counts and any file types we miss or get wrong.

**H6. Windows GUI.** Start it as Administrator. Pick a drive, an output path, run each of Image,
Carve, Verify and Analyze, and try Stop in the middle of imaging. Report anything that is confusing
or breaks.

**H7. macOS.** Run `list-devices`, then image a spare USB stick. **Do not image the disk macOS
boots from:** the guard that refuses the system disk is not verified on APFS.

## What to report

Open an issue and include the information above. Please do not attach recovered files or full
images from private drives. If you can, run with `--json` and `-q` and attach the JSON line: it
has the byte counts and exit code in a form that is easy to compare.

A result of "it worked" is useful too, especially on hardware: say which drive and which system.
