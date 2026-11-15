# Recovery playbook

A plain-language order of work for a USB stick, SD card or external drive that is failing. Follow it
top to bottom. Each step says why, so you can judge when your case is different.

## 1. Decide whether software is the right tool

Software can help when the drive is **detected by the computer** but reads slowly, fails on some
areas, disconnects now and then, or has a damaged filesystem.

Stop and consider a data recovery lab when:

- the drive clicks, grinds, beeps or will not spin up (hard drives);
- it was dropped, bent, water-damaged or burnt, or the connector is broken;
- the computer does not detect it at all, on any port or machine;
- the data is valuable enough that one more failed attempt would be a disaster.

Nothing in this tool opens a drive or repairs hardware.

## 2. Leave the drive alone

- Do **not** let the system "scan and fix" it, run `chkdsk` or `fsck`, format it, or write to it.
- If the system offers to initialize or format the drive, say no.
- If it is mounted, unmount it (or unplug it, then plug it back in without mounting). On Windows,
  Disk Management may show it as "RAW" or ask to format: ignore that.
- Every extra read stresses a failing drive. Plan one careful pass instead of many experiments.

## 3. Get the setup right

- Connect the drive **directly** to the computer (no hub, no extension cable), to a port on the
  computer itself. Use a good cable, and for hard drives make sure they get enough power.
- Keep the computer from sleeping during the run.
- Choose where the image goes: a **different** disk with room for the whole drive. The tool
  refuses to write the image onto the source disk.
- If the drive might be evidence, use a hardware write blocker between the drive and the computer
  if you have one. The tool only reads the source, but a write blocker also stops the operating
  system from touching it. Fill in [examples/chain-of-custody.md](../examples/chain-of-custody.md)
  as you go.

## 4. Practise first (two minutes)

Try the whole flow on the practice image, so the commands are familiar before you point them at
something that matters: see [examples/README.md](../examples/README.md).

## 5. Make the image

Find the device with `tpt-usb-rescue list-devices` and check the size matches the drive. Then:

```sh
sudo ./examples/recover.sh /dev/sdb ~/rescue/usb1
```

or, without the script:

```sh
tpt-usb-rescue image /dev/sdb recovery.img --hash
```

Then leave it alone. A failing drive can take hours. The progress line shows how far it is.

- **Ctrl-C** stops cleanly and saves progress. Run the same command with `--resume` to continue.
- If the drive **disconnects**, the run stops with progress saved (exit code 3). Replug it, check
  the device name again, and resume.
- If the drive **hangs** on some areas, reads that take too long are given up on and the run
  carries on. A shorter `--timeout` and a smaller `--chunk-kib` find the edge of a bad area
  sooner. See [Troubleshooting](TROUBLESHOOTING.md#it-is-very-slow).
- If it keeps failing at the same point, do not keep retrying for hours. Unreadable sectors are
  recorded in the progress file; the rest of the drive is still copied.

When it finishes, exit code 0 means everything was read. Exit code 5 means the copy is finished but
some sectors could not be read; they are zeros in the image.

## 6. Put the drive away

Once you have an image you are happy with, **stop using the drive**. Everything from here on works
on the image. If it is evidence, label it and store it as the original.

## 7. Work from the image

```sh
tpt-usb-rescue analyze recovery.img --extract ./volumes   # files from a readable filesystem
tpt-usb-rescue carve   recovery.img ./carved --validate-media   # files found by signature
tpt-usb-rescue verify  ./carved recovery.img
```

- Start with `analyze --extract`: it keeps real names and folders when the filesystem can still be
  read. Deleted entries are written with a `DELETED_` prefix.
- Use `carve` for what is left, or when no filesystem can be read. Carved files have generic names;
  `index.csv` records where each came from and its score. A score of 1.00 means the structure
  checks out; lower means damaged or not really a file of that type.
- `verify` confirms that the carved files match the image and that the image has not changed
  since it was made.

## 8. Keep what matters

- Keep `recovery.img`, `recovery.img.map` and `recovery.img.sha256` together until you are sure
  you have what you need.
- Copy the recovered files to a healthy disk, and open a few of them to check they are good.
  Large files stored in pieces may be damaged or cut short.
- Only then, if you want to reuse the drive, replace it. A drive that has failed once is not one to
  trust with new data.

## What this tool cannot do

- Repair hardware, or read a drive the computer cannot detect.
- Recover data that was overwritten, or on drives that encrypt data when the key is lost.
- Reassemble files that were stored in several separate pieces from carving alone.
- Guarantee anything on a drive that is physically failing: it will copy what the drive is still
  willing to give.
