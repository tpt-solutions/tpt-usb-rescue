#!/usr/bin/env bash
# A complete recovery in one command: image the drive, look for filesystems, carve files, verify.
#
#   ./recover.sh <device-or-image> <work-folder> [extra options for `image`]
#
#   sudo ./recover.sh /dev/sdb ~/rescue/usb1
#   sudo ./recover.sh /dev/sdb ~/rescue/usb1 --timeout 10 --chunk-kib 256
#
# It is safe to run again after an interruption: if a progress file exists, the copy resumes.
# Set TPT to the path of the tool if it is not on your PATH (TPT=./tpt-usb-rescue ./recover.sh ...).
set -u

if [ $# -lt 2 ]; then
    sed -n '2,10p' "$0" | sed 's/^# \{0,1\}//'
    exit 2
fi
src="$1"; work="$2"; shift 2
tool="${TPT:-tpt-usb-rescue}"

mkdir -p "$work"
img="$work/recovery.img"

resume=()
if [ -e "$img.map" ]; then
    echo "== found an earlier run, resuming"
    resume=(--resume)
fi

echo "== 1/4 imaging $src -> $img"
"$tool" image "$src" "$img" --hash ${resume[@]+"${resume[@]}"} "$@"
code=$?
case $code in
    0) ;;
    5) echo "!! finished, but some sectors could not be read. They are zero-filled in the image." ;;
    3) echo "!! stopped early. Run this script again to resume."; exit 3 ;;
    *) echo "!! imaging failed (exit $code)"; exit "$code" ;;
esac

echo "== 2/4 looking for filesystems"
"$tool" analyze "$img" --extract "$work/volumes" || echo "(nothing extracted)"

echo "== 3/4 carving files by signature"
"$tool" carve "$img" "$work/carved" --validate-media || exit $?

echo "== 4/4 verifying carved files against the image"
"$tool" verify "$work/carved" "$img"
vcode=$?

echo
echo "Done. Everything is in $work:"
echo "  recovery.img      the raw image (a copy; the drive was only read)"
echo "  recovery.img.map  which sectors were recovered or bad"
echo "  recovery.img.sha256  the image's hash"
echo "  volumes/          files read from the filesystem, if one was found"
echo "  carved/           files found by signature, with index.csv"
[ "$code" -eq 5 ] && exit 5
exit "$vcode"
