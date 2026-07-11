#!/usr/bin/env bash
set -euo pipefail

set -o allexport
source .env
set +o allexport

mkdir -p tmp

cargo objcopy --release --bin ws -p firmware --features=defmt_tcp --target thumbv8m.main-none-eabihf -- -O binary tmp/firmware_ws.bin

# dns-sd -G v4 mypico2w.local

# PICO_IP=192.168.49.39
PICO_IP=mypico2w.local
# PICO_IP=192.168.1.1
# PICO_IP=10.3.2.212
SUM=$(sha256sum tmp/firmware_ws.bin | awk "{print \$1}")

echo SUM=$SUM

ls -l tmp/firmware_ws.bin

curl -# -X POST --data-binary @tmp/firmware_ws.bin "http://$PICO_IP/api/ota?$SUM"
