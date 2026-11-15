# Recovery record

Fill this in while you work. It records what was done, with what, and what came out, so someone
else can check the result later. The tool's own output supplies most of the values below.

## Case

| | |
|---|---|
| Case or ticket | |
| Examiner | |
| Date and time started (with time zone) | |
| Where the work was done | |

## The device

| | |
|---|---|
| Make and model | |
| Serial number (from the label) | |
| Stated capacity | |
| Device path used (`list-devices`) | |
| Size reported by the tool | |
| Condition and symptoms (not detected, slow, errors, physical damage) | |
| Who handed it over, and when | |

## The tool

| | |
|---|---|
| Version (`tpt-usb-rescue --version`) | |
| Where it came from (release file name) | |
| Download checked against `SHA256SUMS`? | yes / no |
| Computer and operating system | |

## What was run

Paste each command exactly as typed, with the exit code.

```
$ tpt-usb-rescue image ...
exit code:
```

```
$ tpt-usb-rescue carve ...
exit code:
```

```
$ tpt-usb-rescue verify ...
exit code:
```

## Results

| | |
|---|---|
| Image file and where it is stored | |
| Image SHA-256 (from `recovery.img.sha256`) | |
| Image BLAKE3 (printed by `image --hash` and `verify`) | |
| Bytes recovered / bad / not yet read (`tpt-usb-rescue mapfile recovery.img.map`) | |
| Exit code of `image` (0 complete, 5 finished with unreadable sectors, 3 stopped) | |
| Interruptions and resumes (what happened, when) | |
| Files carved, and how many verified (`verify` summary line) | |
| Files read from a filesystem (`analyze`) | |

## Handling

| Date and time | Who | What happened to the image or the recovered files (copied, moved, handed over) |
|---|---|---|
| | | |
| | | |

Keep the original device untouched after imaging, and keep `recovery.img`, `recovery.img.map` and
`recovery.img.sha256` together. To confirm the image has not changed later, run
`tpt-usb-rescue verify <carved-folder> recovery.img`: it checks the image against the recorded hash
and reports `IMAGE CHANGED` if it differs.

Signed: ______________________  Date: ______________
