# Put rustup's shims first so every recipe honors rust-toolchain.toml.
# Homebrew's /opt/homebrew/bin/cargo is a plain stable toolchain: it ignores
# the pin and dies on `#![feature(...)]` (E0554) with no embedded targets.
export PATH := home_directory() / ".cargo/bin:" + env_var("PATH")

default:
    @ {{just_executable()}} --list --justfile {{justfile()}} --unsorted
    
# Full-system simulator: dot-matrix window + the real web UI (http://localhost:8080)
sim:
    cargo run -r -p sim --bin sim

# Old animation-only scratchpad (keyboard, no web app)
sim-animations:
    cargo run -r -p sim --bin animation_dev

ws_client:
    RUST_LOG=debug cargo run -r -p ws_client

main:
    #!/usr/bin/env bash
    set -euo pipefail

    transport=$(printf '%s\n' "defmt_rtt" "defmt_tcp" | fzf --prompt="Transport> " --height=~10 --header="Select logging transport")
    network=$(printf '%s\n' "wifi" "usb_ethernet" "ppp" "radio" | fzf --prompt="Network> " --height=~10 --header="Select network mode")

    echo "Building with features: ${transport},${network}"
    cargo objcopy --release -p firmware --features="${transport},${network}" --bin ws --target thumbv8m.main-none-eabihf -- -O binary tmp/ws.bin

    picotool load --offset 0x10001000 tmp/ws.bin
    picotool reboot

    cp target/thumbv8m.main-none-eabihf/release/ws tmp/ws.elf
    sleep 0.5
    probe-rs attach --chip RP235x tmp/ws.elf

ota:
    #!/usr/bin/env bash
    set -euo pipefail

    transport=$(printf '%s\n' "defmt_rtt" "defmt_tcp" | fzf --prompt="Transport> " --height=~10 --header="Select logging transport")
    network=$(printf '%s\n' "wifi" "usb_ethernet" "ppp" "radio" | fzf --prompt="Network> " --height=~10 --header="Select network mode")
    host=$(printf '%s\n' "led-board.local" "192.168.7.10" "192.168.1.1" | fzf --prompt="Host> " --height=~10 --header="Select host")

    echo "Building with features: ${transport},${network}"
    cargo objcopy --release -p firmware --features="${transport},${network}" --bin ws --target thumbv8m.main-none-eabihf -- -O binary tmp/ws.bin

    cp target/thumbv8m.main-none-eabihf/release/ws tmp/ws.elf

    sum=$(sha256sum tmp/ws.bin | awk "{print \$1}")

    # Raw-DEFLATE the image (~2.5x smaller): the /api/ota handler inflates
    # streams when the query carries z=raw (link runs at 115200, see notes).
    python3 -c "import zlib; d=open('tmp/ws.bin','rb').read(); c=zlib.compressobj(9,zlib.DEFLATED,-15); open('tmp/ws.def','wb').write(c.compress(d)+c.flush())"

    echo "Uploading... ($(stat -f%z tmp/ws.bin) -> $(stat -f%z tmp/ws.def) bytes on the wire)"

    curl -# -X POST --data-binary @tmp/ws.def "http://$host/api/ota?$sum&z=raw"

attach_rtt:
    probe-rs attach --chip RP235x tmp/ws.elf

attach_tcp:
    #!/usr/bin/env bash
    set -euo pipefail

    host=$(printf '%s\n' "led-board.local" "192.168.7.10" "192.168.1.1" | fzf --prompt="Host> " --height=~10 --header="Select host")
    
    defmt-print -e target/thumbv8m.main-none-eabihf/release/ws tcp --host $host

pppd_watch:
    #!/usr/bin/env bash
    set -euo pipefail

    DEVICE_PATTERN="/dev/cu.usbmodem123456781"
    BAUD="115200"
    LOCAL_IP="192.168.7.1"
    REMOTE_IP="192.168.7.10"

    echo "Waiting for USB serial device ($DEVICE_PATTERN)..."
    while true; do

        # Wait until at least one device appears
        while [ -z "$(ls $DEVICE_PATTERN 2>/dev/null | head -1)" ]; do
            echo "No device found. Waiting 2 seconds..."
            sleep 2
        done

        DEVICE_PATH=$(ls $DEVICE_PATTERN 2>/dev/null | head -1)
        echo "Device detected: $DEVICE_PATH"

        # Start pppd in background
        sudo pppd \
            "$DEVICE_PATH" \
            "$BAUD" \
            "$LOCAL_IP:$REMOTE_IP" \
            ms-dns 8.8.4.4 \
            ms-dns 8.8.8.8 \
            nodetach \
            debug \
            local \
            persist \
            holdoff 5 \
            maxfail 0 \
            silent \
            proxyarp \
            noauth \
            &

        PPPD_PID=$!

        echo "pppd started with PID $PPPD_PID"

        # Monitor device file — if it disappears, kill pppd
        while [ -c "$DEVICE_PATH" ] && kill -0 $PPPD_PID 2>/dev/null; do
            sleep 1
        done

        # If we get here, either:
        # 1. Device was unplugged (file gone), OR
        # 2. pppd crashed/exited (PID no longer running)

        if [ ! -c "$DEVICE_PATH" ]; then
            echo "Device $DEVICE_PATH disappeared. Killing pppd (PID $PPPD_PID)..."
            kill $PPPD_PID 2>/dev/null || true
        else
            echo "pppd exited on its own (PID $PPPD_PID)."
        fi

        # Wait a moment before retrying
        echo "Waiting 3 seconds before retrying..."
        sleep 3

    done

setup_flash:
    #!/usr/bin/env bash
    set -euo pipefail

    picotool partition create partitions.json tmp/partition_table.bin
    picotool load --offset 0x10000000 tmp/partition_table.bin
    picotool partition info

upload_wifi_firmware:
    #!/usr/bin/env bash
    set -euo pipefail

    # probe-rs download firmware/blobs/43439A0.bin --binary-format bin --chip RP235x --base-address 0x10200000
    picotool load --offset 0x10200000 firmware/blobs/43439A0.bin

# ESP32-C3 slave radio firmware
radio-build:
    cd slave_radio && cargo build --release

# NOTE: `cargo flash` silently ignores the runner's --partition-table flag and
# flashed the DEFAULT table (app at 0x10000, colliding with the cfg partition).
# Always flash explicitly and check the reported partition capacity
# (factory app is 4,063,232 bytes = 0x3e0000 when the table is applied).
radio-flash:
    cd slave_radio && cargo build --release && espflash flash --partition-table partitions.csv --target-app-partition ota_0 target/riscv32imc-unknown-none-elf/release/slave_radio && espflash erase-region 0xd000 0x2000 && espflash reset

# ESP32-C6 variant (rv32imac target + esp32c6 feature; same 4MB partition table).
# USB flash always writes ota_0 and clears otadata so booting is deterministic
# (empty otadata → first app partition); OTA updates flip slots themselves.
radio-c6-flash:
    cd slave_radio && cargo build --release --no-default-features --features esp32c6 --target riscv32imac-unknown-none-elf && espflash flash --partition-table partitions.csv --target-app-partition ota_0 target/riscv32imac-unknown-none-elf/release/slave_radio && espflash erase-region 0xd000 0x2000 && espflash reset

# OTA the radio itself over the LAN: streams the image into the inactive OTA
# slot via /api/ota-radio, flips otadata, reboots (~20 s outage). Override
# target host with RADIO_HOST=... just radio-ota
radio-ota:
    #!/usr/bin/env bash
    set -euo pipefail
    export PATH="$HOME/.cargo/bin:$PATH"
    (cd slave_radio && cargo build --release --no-default-features --features esp32c6 --target riscv32imac-unknown-none-elf)
    espflash save-image --chip esp32c6 slave_radio/target/riscv32imac-unknown-none-elf/release/slave_radio tmp/radio.bin
    echo "radio image: $(stat -f%z tmp/radio.bin) bytes sha256 $(sha256sum tmp/radio.bin | awk '{print $1}')"
    curl -sS -X POST --data-binary @tmp/radio.bin "http://${RADIO_HOST:-192.168.49.137}/api/ota-radio"
    echo
    echo "radio rebooting; rejoined + re-linked in ~20 s (check: GET /api/ota-radio slot)"

# Native USB-JTAG disconnects when the chip resets, so monitor with --after no-reset
# while the app is running (boot logs are lost without a listener attached at boot —
# the console FIFO drops when nothing is connected, which is by design).
radio-monitor:
    espflash monitor --after no-reset

# Jump the running Pico app into BOOTSEL over USB — no button press. Works
# because the `console` build exposes the pico-sdk vendor reset interface
# (see firmware/src/tasks/usb_dbg.rs). The --vid/--pid flags are required:
# picotool's default scan only auto-classifies VID 2E8A, and app-mode 2E8A
# devices are grabbed by a DriverKit personality on this Mac and never bind.
bootsel:
    picotool reboot -u -f --vid 5824 --pid 1171
