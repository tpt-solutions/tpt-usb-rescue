#!/usr/bin/env bash
# Builds a small synthetic disk image so you can try tpt-usb-rescue without any hardware.
#
# The image is random noise with two tiny PNGs and one GIF hidden inside it, so `carve` has
# something real to find. It needs only bash, base64 and /dev/urandom.
#
#   ./make-test-image.sh [output-file]        (default: test.img)
set -euo pipefail

out="${1:-test.img}"

# A 1x1 PNG and a 1x1 GIF, as base64.
png='iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg=='
gif='R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7'

{
    head -c 262144 /dev/urandom
    echo "$png" | base64 -d
    head -c 100000 /dev/urandom
    echo "$gif" | base64 -d
    head -c 65536 /dev/urandom
    echo "$png" | base64 -d
    head -c 131072 /dev/urandom
} > "$out"

echo "wrote $out ($(wc -c < "$out") bytes) with 2 PNGs and 1 GIF hidden in it"
echo "try:  tpt-usb-rescue image $out recovered.img"
