#!/usr/bin/env bash
# Checks tpt-usb-rescue against real Linux block devices, with no failing hardware needed.
#
#   sudo BIN=./target/release/tpt-usb-rescue tools/linux-validate.sh
#
# It builds loop devices and device-mapper devices (clean, with unreadable sectors, with hung
# reads, and slow) and checks that the tool behaves as documented: byte-exact images, bad sectors
# reported and zero-filled, hung reads survived, Ctrl-C and resume, and the safety guards
# (output on the imaged disk or one of its partitions, imaging the disk that hosts /).
#
# Needs: root, losetup, dmsetup (targets: linear, error, delay), sfdisk, mkfs.ext4, python3.
# Everything is created under /tmp/tpt-val and removed afterwards. Nothing touches real disks:
# the only real disk it names is the one hosting /, and only to check that it is refused.
export PATH="$HOME/.cargo/bin:$PATH"
BIN="${BIN:-tpt-usb-rescue}"
W=/tmp/tpt-val
SECT=65536   # 32 MiB source
pass=0; fail=0
ok()   { echo "PASS  $1"; pass=$((pass+1)); }
bad()  { echo "FAIL  $1"; fail=$((fail+1)); }
check() { # name, condition-exit-code
    if [ "$2" -eq 0 ]; then ok "$1"; else bad "$1"; fi
}

cleanup() {
    cd /
    for m in /mnt/tpt-m2 /mnt/tpt-m3; do mountpoint -q "$m" 2>/dev/null && umount "$m"; done
    for d in flaky hang slow; do dmsetup remove --retry "$d" 2>/dev/null; done
    for l in $(losetup -a | grep "$W" | cut -d: -f1); do losetup -d "$l" 2>/dev/null; done
    rm -rf "$W" /mnt/tpt-m2 /mnt/tpt-m3
}
trap cleanup EXIT
cleanup
mkdir -p "$W" /mnt/tpt-m2 /mnt/tpt-m3
cd "$W" || exit 1

echo "binary: $BIN"; "$BIN" --version
head -c $((SECT*512)) /dev/urandom > src.bin
SRC_SHA=$(sha256sum src.bin | cut -d' ' -f1)
LSRC=$(losetup -f --show src.bin)
echo "source loop device: $LSRC"
echo "== list-devices"; "$BIN" list-devices | head -5

echo
echo "== T1 clean loop device"
"$BIN" image "$LSRC" t1.img --hash -q > t1.out 2> t1.err; code=$?
check "T1 exit code 0 (got $code)" $([ $code -eq 0 ]; echo $?)
cmp -s t1.img src.bin; check "T1 image is byte-identical to the source" $?
[ "$(cut -d' ' -f1 t1.img.sha256)" = "$SRC_SHA" ]; check "T1 sidecar sha256 equals sha256sum of the source" $?
head -3 t1.out

echo
echo "== T2 safety guards"
dd if=/dev/zero of=fs2.img bs=1M count=64 status=none
L2=$(losetup -f --show fs2.img); mkfs.ext4 -q "$L2"; mount "$L2" /mnt/tpt-m2
"$BIN" image "$L2" /mnt/tpt-m2/out.img -q > t2a.out 2> t2a.err; code=$?
[ $code -eq 1 ] && grep -q "on the device being imaged" t2a.err; check "T2a output (a new file) on the imaged device is refused (exit $code)" $?
[ ! -e /mnt/tpt-m2/out.img ]; check "T2a nothing was written to the refused output" $?
cat t2a.err | head -2

dd if=/dev/zero of=disk3.img bs=1M count=64 status=none
printf 'label: dos\nstart=2048, type=83\n' | sfdisk -q disk3.img
L3=$(losetup -f --show -P disk3.img)
mkfs.ext4 -q "${L3}p1"; mount "${L3}p1" /mnt/tpt-m3
"$BIN" image "$L3" /mnt/tpt-m3/out.img -q > t2b.out 2> t2b.err; code=$?
[ $code -eq 1 ] && grep -q "on the device being imaged" t2b.err; check "T2b output on a PARTITION of the imaged disk is refused (exit $code)" $?
head -2 t2b.err

ROOTDEV=$(findmnt -n -o SOURCE /)
echo "root filesystem device: $ROOTDEV"
timeout 20 "$BIN" image "$ROOTDEV" /mnt/tpt-m2/sys.img --pass 1 -q > t2c.out 2> t2c.err; code=$?
[ $code -eq 1 ] && grep -q "hosts the running system" t2c.err; check "T2c imaging the disk that hosts / is refused (exit $code)" $?
head -2 t2c.err
rm -f /mnt/tpt-m2/sys.img

echo
echo "== T3 real unreadable sectors (dm error target, 8 sectors at 1003..1010)"
dmsetup create flaky <<EOF
0 1003 linear $LSRC 0
1003 8 error
1011 $((SECT-1011)) linear $LSRC 1011
EOF
"$BIN" --json image /dev/mapper/flaky t3.img -q > t3.json 2> t3.err; code=$?
check "T3 exit code 5 = finished with unrecovered data (got $code)" $([ $code -eq 5 ]; echo $?)
echo "     $(cat t3.json)"
grep -q '"bad_bytes":4096' t3.json; check "T3 exactly 8 sectors (4096 bytes) reported bad" $?
python3 - <<'PYEOF'
import sys
src=open("src.bin","rb").read(); out=open("t3.img","rb").read()
if len(src)!=len(out): print("length differs", len(src), len(out)); sys.exit(1)
badsec=set(range(1003,1011))
for s in range(len(src)//512):
    a=src[s*512:(s+1)*512]; b=out[s*512:(s+1)*512]
    if s in badsec:
        if b!=bytes(512): print("sector",s,"should be zero"); sys.exit(1)
    elif a!=b: print("sector",s,"differs"); sys.exit(1)
print("all good sectors identical, all 8 bad sectors zero-filled")
PYEOF
check "T3 content: good sectors identical, bad sectors zero" $?
"$BIN" mapfile t3.img.map | tail -6

echo
echo "== T4 hung reads (dm delay target: 8 sectors answer after 3 s; --timeout 1)"
dmsetup create hang <<EOF
0 20000 linear $LSRC 0
20000 8 delay $LSRC 20000 3000
20008 $((SECT-20008)) linear $LSRC 20008
EOF
resumes=0; code=99
start=$(date +%s)
"$BIN" image /dev/mapper/hang t4.img --timeout 1 -q > t4.out 2> t4.err; code=$?
echo "     first run: exit $code, $(( $(date +%s) - start )) s"; tail -2 t4.err
while [ $code -eq 3 ] && [ $resumes -lt 30 ]; do
    resumes=$((resumes+1))
    "$BIN" image /dev/mapper/hang t4.img --timeout 1 --resume -q > t4.out 2> t4.err; code=$?
done
echo "     finished after $resumes resume(s), last exit $code, total $(( $(date +%s) - start )) s"
check "T4 the run finishes on its own without manual resumes (resumes needed: $resumes)" $([ $resumes -eq 0 ]; echo $?)
check "T4 final exit code is 0 or 5 (got $code)" $([ $code -eq 0 ] || [ $code -eq 5 ]; echo $?)
"$BIN" mapfile t4.img.map | tail -5

echo
echo "== T5 Ctrl-C then resume (dm delay of 150 ms on every IO)"
dmsetup create slow <<EOF
0 $SECT delay $LSRC 0 150
EOF
timeout --preserve-status -s INT 3 "$BIN" image /dev/mapper/slow t5.img -q > t5.out 2> t5.err; code=$?
check "T5 SIGINT stops the run with exit code 3 (got $code)" $([ $code -eq 3 ]; echo $?)
tail -2 t5.err
"$BIN" image /dev/mapper/slow t5.img --resume -q > t5b.out 2> t5b.err; code=$?
check "T5 resume completes with exit 0 (got $code)" $([ $code -eq 0 ]; echo $?)
cmp -s t5.img src.bin; check "T5 resumed image is byte-identical to the source" $?

echo
echo "RESULT: $pass passed, $fail failed"
exit $fail
