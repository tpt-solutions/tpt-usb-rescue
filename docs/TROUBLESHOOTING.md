# Troubleshooting

## Before anything else

New to this? Read the [recovery playbook](PLAYBOOK.md) first: it puts the steps in order.

- Work on the **image**, never on the failing drive. Do not mount it, run `chkdsk` or `fsck`, or
  let the system "repair" it. Every extra read can make a dying drive worse.
- If the drive clicks, grinds or has been dropped, stop. A software tool cannot help with a
  mechanical fault, and a data recovery lab can.
- The tool only reads the source device. It refuses to write the image to the source disk.

## Permission denied, or `list-devices` shows nothing

Raw disks need elevated rights. On Windows, start PowerShell or the GUI as Administrator. On Linux
and macOS use `sudo`. If the list is still empty, check that the system sees the drive at all
(Disk Management on Windows, `lsblk` on Linux, `diskutil list` on macOS). A drive the operating
system cannot detect cannot be imaged by this tool.

## "hosts the running system"

The tool will not image the disk your operating system runs from, because you would copy a disk
that is changing under you. If you really mean it, add `--allow-system-disk`.

## "is on the device being imaged" or "is a device node"

The output file must be a regular file on a **different** disk from the one you are copying. Pick
a folder on another drive with room for the whole device.

## "mapfile ... already records progress"

You ran `image` again for an output that already has a progress file. This stops you overwriting
earlier work. To continue the earlier run, add `--resume`. To start over, delete the `.map` file
(and the image).

## "cannot resume: image ... is missing" or "is N bytes but the mapfile expects ..."

The progress file says data was recovered, but the image is missing or shorter than it should be, so
resuming would hide missing data behind zeros. Find the original image file, or start over with a
new image and map.

## The run stopped: "device removed" or "stopped responding"

The drive disconnected, or stopped answering (several reads in a row timed out and it could no longer re-read a sector it had already recovered). Progress is saved.

1. Unplug it, wait a few seconds, and plug it back in, directly into the computer rather than
   through a hub. Try another port or cable.
2. Run the same command again with `--resume`. The device path may have changed (for example
   `/dev/sdc` instead of `/dev/sdb`): use the one `list-devices` shows now.

Exit code 3 means exactly this: stopped, resumable.

## It is very slow

Failing drives are slow, and most of the time goes on the bad areas. This is expected. A large
drive with many bad sectors can take many hours; do not interrupt it unless you must (Ctrl-C saves
progress). Things that help:

- A smaller `--chunk-kib` (for example 128) finds the edge of a bad area sooner.
- A shorter `--timeout` (for example 2) stops waiting on hung reads sooner. A longer one helps
  drives that are slow but do answer.
- On Linux, running as root lets the tool re-enumerate a hung USB device (`--no-reset` turns that
  off).

## Exit code 5: "finished, but ... could not be recovered"

The copy is complete, but some sectors could not be read. They are zeros in the image.
`tpt-usb-rescue mapfile recovery.img.map` lists the bad runs. Files that overlap them will be
damaged; the rest are fine. The tool already retried each bad sector several times and trimmed the
edges of every bad run, so running it again with `--resume` will not recover more. If you want
another attempt (for example after letting the drive rest), image into a new file with a new
progress file and compare the two results.

## `analyze` finds no volumes

This is normal for badly damaged drives or ones that hold no FAT32, exFAT or NTFS filesystem. Use
`carve`, which does not need a filesystem or a partition table.

## Carved files are broken, or there are lots of junk files

`carve` finds files by their first bytes, so random data sometimes looks like a file. The score in
`index.csv` and the carve output (1.00 = structure checks out, lower = damaged or fake) tells them
apart. Add `--validate-media` to drop candidates that fail the structure checks. Large files that
were stored in pieces across the drive come out truncated or damaged: carving assumes a file is in
one piece.

## `verify` says MISMATCH, MISSING or IMAGE CHANGED

- `MISMATCH` or `MISSING`: a carved file differs from, or is gone from, the carved folder it was
  written to. Carve again into a fresh folder.
- `IMAGE CHANGED`: the image no longer matches the hash recorded with `image --hash`. Something
  modified the file after it was made. Do not trust it; use a backup copy or image again.

## Windows says the program is from an unknown publisher

The release files are not code-signed, so SmartScreen may warn. Check the download against
`SHA256SUMS` first, then use "More info" and "Run anyway" if it matches.

## Still stuck

Open an issue on GitHub and include: the exact command, the tool version (`--version`), your
operating system, and the output. Do not post file contents recovered from private drives.
For security problems, follow [SECURITY.md](../SECURITY.md) instead.
