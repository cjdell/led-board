#!/usr/bin/env bash
# Build the WiFi/web image and update a running board over OTA — no button,
# no BOOTSEL. The board reboots itself into the new slot (see the OTA reboot
# traps in AGENTS.md) and marks it good on the way up.
#
#   scripts/ota.sh [host]        # host defaults to 10.3.2.212
#
# Progress is visible on both sides: curl reports the upload, and the boot
# ledger can be watched with
#   defmt-print -e tmp/ws.elf tcp --host <host>
set -euo pipefail
cd "$(dirname "$0")/.."

HOST="${1:-10.3.2.212}"
export PATH="$HOME/.cargo/bin:$PATH"

echo "== build (wifi,defmt_tcp)"
cargo objcopy --release -p firmware --features wifi,defmt_tcp --bin ws \
    --target thumbv8m.main-none-eabihf -- -O binary tmp/ws.bin
cp target/thumbv8m.main-none-eabihf/release/ws tmp/ws.elf
echo "   image: $(stat -f%z tmp/ws.bin) bytes"

sum=$(shasum -a 256 tmp/ws.bin | awk '{print $1}')

echo "== upload to $HOST"
# The connection is expected to drop mid-response: the board resets itself
# about a second after verifying the image.
curl -sS -m 240 -X POST --data-binary @tmp/ws.bin \
    "http://$HOST/api/ota?$sum" 2>&1 | tail -2 || true

echo
echo "== waiting for the board to come back"
sleep 5
for _ in $(seq 1 20); do
    if curl -s -m 3 -o /dev/null "http://$HOST/api/join-status"; then
        echo "   back: $(curl -s -m 3 "http://$HOST/api/join-status")"
        exit 0
    fi
    sleep 3
done

echo "board did not answer after 60 s — check power/network" >&2
exit 1
