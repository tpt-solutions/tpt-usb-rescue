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
tpt-usb-rescue image /dev/sdb recovery.img --mapfile recovery.map
```

The three passes run in order:

1. **Sweep** reads everything in 1 MiB chunks. Chunks that fail are marked `NonTrimmed`.
2. **Retry** re-reads those chunks at 4 KiB, then sector by sector. Sectors that still fail are
   `BadSector`.
3. **Trim** tries the first and last sector of each bad run, and keeps going inward while reads
   succeed, so the bad area is as small as possible.

Run one pass with `--pass 1`, `--pass 2` or `--pass 3`.

Every checkpoint syncs the image before the mapfile records a sector as recovered, so a crash or
power cut never leaves a map claiming data that is not on disk.

### Stopping and resuming

If the device is unplugged, or a read stalls, the run stops with the progress saved:

```sh
tpt-usb-rescue image /dev/sdb recovery.img --mapfile recovery.map --resume
```

`--resume` keeps the existing image and map and continues. Without it, the tool refuses to start
over a map that already records progress, so you cannot overwrite work by accident.

### Stalled reads

A read that does not finish within 5 seconds counts as a media failure, and the run carries on.
On Linux the tool also asks the kernel to re-enumerate the USB device behind the disk (needs root).
After a reset the device handle is invalid, so the run stops. Re-run with `--resume`.

### Mapfile

The mapfile is a binary, append-only log. Each record has its own CRC, so a torn write at the end
is detected and dropped. Inspect it or export it for GNU ddrescue or TOML:

```sh
tpt-usb-rescue mapfile recovery.map
tpt-usb-rescue mapfile recovery.map --export ddrescue --out recovery.ddr
tpt-usb-rescue mapfile recovery.map --export toml
```

## Carving files from an image

```sh
tpt-usb-rescue carve recovery.img ./carved --validate-media
```

- Without `--validate-media` every candidate is written, and its score is recorded in
  `index.csv`.
- With `--validate-media` candidates scoring below 0.5 are rejected as false positives.
- Each file is named `file_<id>.<ext>`. `index.csv` lists offsets, sizes, type and score.

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
its byte range in the image. It exits with an error if any file differs or is missing.

## The Windows GUI

Run `tpt-usb-rescue-gui.exe`, or start it as Administrator to see physical drives.

1. Choose a source in **Source**, or pick a device in the **Device** list and press **Refresh** if
   it is missing.
2. Set **Image** to the output file. The mapfile defaults to the same name with `.map`.
3. Press **Start imaging**. Progress and results appear in the log.
4. **Carve**, **Verify** and **Analyze** work on the **Image** file. Carved files go to a `carved`
   folder beside it.

The window uses the same code as the command line, so the same safety checks apply.

## Testing without hardware

The repository contains everything needed to try the tool on synthetic data:

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
