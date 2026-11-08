#!/bin/bash
# Flash C6 then Pico. Usage: tmp/flash_both.sh [pico.bin]
set -e
export PATH="$HOME/.cargo/bin:$PATH"
BIN=${1:-tmp/ws_w4.bin}
pkill -f 'cat /dev/cu' 2>/dev/null || true
sleep 1

C6PORT=$(ls /dev/cu.usbmodem* 2>/dev/null | grep -v bench | head -1)
[ -z "$C6PORT" ] && { echo "no C6 port"; exit 1; }
(cd slave_radio && espflash flash --partition-table partitions.csv --target-app-partition ota_0 -p "$C6PORT" \
    target/riscv32imac-unknown-none-elf/release/slave_radio 2>&1 | grep -E 'Flashing has completed|error')
# Deterministic A/B after USB flash: clear otadata → bootloader boots ota_0.
espflash erase-region -p "$C6PORT" 0xd000 0x2000 >/dev/null 2>&1 || true
espflash reset -p "$C6PORT" >/dev/null 2>&1 || true

just bootsel
# Wait for BOOTSEL to become accessible (can lag enumeration by seconds).
for i in $(seq 1 20); do
  sleep 2
  if picotool info --all >/dev/null 2>&1; then break; fi
done
picotool load --offset 0x10001000 "$BIN" 2>&1 | tr '\r' '\n' | grep -E '100%|error' | tail -1
# Auto-reset sometimes leaves it in BOOTSEL; force an app boot and wait.
for i in $(seq 1 10); do
  sleep 3
  ls /dev/cu.usbmodem* 2>/dev/null | grep -q bench && break
  picotool reboot -a -f >/dev/null 2>&1 || true
done
ls /dev/cu.usbmodem* && echo FLASH_OK
