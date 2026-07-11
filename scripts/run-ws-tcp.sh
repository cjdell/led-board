#!/usr/bin/env bash
set -euo pipefail

set -o allexport
source .env
set +o allexport

cargo objcopy --release --bin ws -p firmware --features=defmt_tcp --target thumbv8m.main-none-eabihf -- -O binary tmp/firmware_ws.bin

picotool load --offset 0x10001000 tmp/firmware_ws.bin
picotool reboot

cp target/thumbv8m.main-none-eabihf/release/ws tmp/firmware_ws.elf
probe-rs attach --chip RP235x tmp/firmware_ws.elf
