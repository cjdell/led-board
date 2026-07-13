default:
    @ {{just_executable()}} --list --justfile {{justfile()}} --unsorted
    
sim:
    cargo run -r -p sim

ws_client:
    RUST_LOG=debug cargo run -r -p ws_client

main:
    #!/usr/bin/env bash
    set -euo pipefail

    transport=$(printf '%s\n' "defmt_rtt" "defmt_tcp" | fzf --prompt="Transport> " --height=~10 --header="Select logging transport")
    network=$(printf '%s\n' "wifi" "usb_ethernet" "ppp" | fzf --prompt="Network> " --height=~10 --header="Select network mode")

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
    network=$(printf '%s\n' "wifi" "usb_ethernet" "ppp" | fzf --prompt="Network> " --height=~10 --header="Select network mode")
    host=$(printf '%s\n' "mypico2w.local" "192.168.7.10" "192.168.1.1" | fzf --prompt="Host> " --height=~10 --header="Select host")

    echo "Building with features: ${transport},${network}"
    cargo objcopy --release -p firmware --features="${transport},${network}" --bin ws --target thumbv8m.main-none-eabihf -- -O binary tmp/ws.bin

    sum=$(sha256sum tmp/ws.bin | awk "{print \$1}")

    curl -# -X POST --data-binary @tmp/ws.bin "http://$host/api/ota?$sum"

attach_tcp:
    #!/usr/bin/env bash
    set -euo pipefail

    host=$(printf '%s\n' "mypico2w.local" "192.168.7.10" "192.168.1.1" | fzf --prompt="Host> " --height=~10 --header="Select host")
    
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

    picotool partition create partitions.json partition_table.bin
    picotool load --offset 0x10000000 partition_table.bin
    picotool partition info

upload_wifi_firmware:
    #!/usr/bin/env bash
    set -euo pipefail

    # probe-rs download firmware/blobs/43439A0.bin --binary-format bin --chip RP235x --base-address 0x10200000
    picotool load --offset 0x10200000 firmware/blobs/43439A0.bin
