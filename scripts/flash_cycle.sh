#!/usr/bin/env bash
# One-shot bench flash cycle: waits for BOOTSEL, writes the app image to the
# partition-table slot, reboots, then captures the USB console.
#
#   scripts/flash_cycle.sh [image.bin]
#
# Assumes the board is (or will be) in BOOTSEL mode — buttonless entry is
# `printf BOOTSEL > /dev/cu.usbmodembench1` (serial command) or
# `picotool reboot -u -f --vid 5824 --pid 1171` (vendor request).
set -uo pipefail
cd "$(dirname "$0")/.."

IMG="${1:-tmp/ws_festival.bin}"
PORT=/dev/cu.usbmodembench1
OFFSET=0x10001000

if [ ! -f "$IMG" ]; then
    echo "missing image: $IMG" >&2
    exit 1
fi
echo "== image: $IMG ($(stat -f '%z' "$IMG") bytes)"

echo "== waiting for BOOTSEL (up to 180 s)..."
for _ in $(seq 1 90); do
    if picotool info >/dev/null 2>&1; then
        echo "   BOOTSEL up"
        break
    fi
    sleep 2
done
if ! picotool info >/dev/null 2>&1; then
    echo "BOOTSEL never enumerated — hold BOOTSEL while replugging, then rerun" >&2
    exit 1
fi

echo "== load @ $OFFSET"
if ! picotool load --offset "$OFFSET" "$IMG"; then
    echo "LOAD FAILED" >&2
    exit 1
fi

echo "== reboot to app"
picotool reboot -a -f
sleep 2

echo "== console capture (35 s)"
pkill -f "cat $PORT" 2>/dev/null
cat "$PORT" > /tmp/boot_capture.txt 2>/dev/null &
CP=$!
sleep 35
kill "$CP" 2>/dev/null
pkill -f "cat $PORT" 2>/dev/null
wc -c /tmp/boot_capture.txt
cat /tmp/boot_capture.txt
