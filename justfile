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

# Flash the image in tmp/ws.bin to BOTH slots, then verify what actually runs.
# Both slots on purpose: the ROM boots the highest IMAGE_DEF version, so a
# stale newer build in the other slot silently wins and your flash "does
# nothing". Identical slots make the boot deterministic. (OTA still works
# normally: it writes the inactive slot and reboot2 forces it.)
flash:
    #!/usr/bin/env bash
    set -euo pipefail

    if [ ! -f tmp/ws.bin ]; then
        echo "no tmp/ws.bin — run 'just main' (build + flash + attach) or build first" >&2
        exit 2
    fi

    just bootsel >/dev/null 2>&1 || true
    sleep 2
    picotool load --offset 0x10001000 tmp/ws.bin
    picotool load --offset 0x10100000 tmp/ws.bin
    picotool reboot -a -f
    sleep 12
    just verify

# Everything that must pass before a commit.
ci:
    cargo check -p sim
    cargo check -p led-core --no-default-features --features std
    cargo check -p firmware --features wifi,defmt_tcp,console --target thumbv8m.main-none-eabihf
    cargo test -p sim
    cargo test -p radio_link
    cd web && deno task check && deno fmt --check src

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

    echo "Uploading raw image ($(stat -f%z tmp/ws.bin) bytes, paced)"

    # Pace the upload: an unpaced 600 KB POST overruns the cyw43 RX path in
    # this embassy pin and the TCP stream stalls after ~12 KB. 24 KB/s keeps
    # the flash-sector blackouts survivable; a 666 KB image takes ~28 s.
    curl -# --limit-rate 24k -X POST --data-binary @tmp/ws.bin "http://$host/api/ota?$sum"

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

    # One-off partition table layout (A/B = 0xFF000 each, 1020 K, Nov 2026).
    # Re-run this after changing partitions.json, then reboot before flashing
    # an app image; OTA targets the active slot from this table.
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
#
# Verified to answer **even with a wedged executor**, so try this before
# asking for a physical BOOTSEL.
bootsel:
    picotool reboot -u -f --vid 5824 --pid 1171

# Post-flash sanity check. This exists because hours were lost to a trap:
# the RP2350 boots the HIGHEST-version slot, so flashing A while B still holds
# a newer build silently runs B — "my fix didn't work" when it was never
# booted. This compares what the board reports against the image on disk and
# proves the app is actually alive (uptime + playlist advancing).
verify:
    #!/usr/bin/env bash
    set -uo pipefail

    host=${HOST:-10.3.2.212}
    image=${IMAGE:-tmp/ws.bin}
    fail=0

    note() { printf '%s\n' "$*"; }
    ok()   { printf '  [ok]   %s\n' "$*"; }
    bad()  { printf '  [FAIL] %s\n' "$*"; fail=1; }

    if [ ! -f "$image" ]; then
        note "no $image — build first (just main) or pass the file via IMAGE=..."
        exit 2
    fi

    # build_time is the only timestamp in the binary with seconds; the commit
    # time is `YYYY-MM-DDTHH:MMZ`.
    want=$(strings "$image" | grep -E '^20[0-9]{2}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z$' | tail -1)

    note "verifying $host against $image"
    [ -n "$want" ] && note "  image build_time: $want"

    if ! body=$(curl -sS --max-time 5 "http://$host/api/version"); then
        bad "board not answering /api/version at $host (wedged? wrong IP?)"
        exit 1
    fi
    ok "board reachable at $host"

    have=$(printf '%s' "$body" | python3 -c 'import sys,json; print(json.load(sys.stdin).get("build_time",""))')
    if [ -z "$want" ]; then
        note "  [warn] could not read build_time from $image (skipping slot check)"
    elif [ "$want" = "$have" ]; then
        ok "running the image you built ($have)"
    else
        bad "board is running $have, but $image was built $want"
        note "         The RP2350 boots the HIGHEST-version slot: the other slot"
        note "         is newer, so your flash to A was ignored. Use 'just flash'"
        note "         (writes both slots) or OTA the new image."
    fi

    # Liveness: uptime must advance and the display playlist must be moving.
    s1=$(curl -s --max-time 5 "http://$host/api/status" || true)
    sleep 2
    s2=$(curl -s --max-time 5 "http://$host/api/status" || true)
    if [ -z "$s1" ] || [ -z "$s2" ]; then
        note "  [warn] /api/status unavailable (expected on no_display recovery builds)"
    else
        u1=$(printf '%s' "$s1" | python3 -c 'import sys,json; print(json.load(sys.stdin).get("uptime_s",0))')
        u2=$(printf '%s' "$s2" | python3 -c 'import sys,json; print(json.load(sys.stdin).get("uptime_s",0))')
        if [ "$u2" -gt "$u1" ]; then ok "uptime advancing (${u1}s -> ${u2}s)"; else bad "uptime frozen at ${u1}s — executor wedged"; fi

        i1=$(printf '%s' "$s1" | python3 -c 'import sys,json; d=json.load(sys.stdin); print(d.get("playlist_index",0))')
        e1=$(printf '%s' "$s1" | python3 -c 'import sys,json; d=json.load(sys.stdin); print((d.get("now_playing") or {}).get("elapsed_ms",0))')
        i2=$(printf '%s' "$s2" | python3 -c 'import sys,json; d=json.load(sys.stdin); print(d.get("playlist_index",0))')
        e2=$(printf '%s' "$s2" | python3 -c 'import sys,json; d=json.load(sys.stdin); print((d.get("now_playing") or {}).get("elapsed_ms",0))')
        if [ "$i2" -ne "$i1" ] || [ "$e2" -gt "$e1" ]; then
            ok "display worker ticking (slot $i1 -> $i2, elapsed ${e1}ms -> ${e2}ms)"
        else
            bad "display worker not advancing (slot $i1, elapsed ${e1}ms -> ${e2}ms)"
        fi
    fi

    if ls /dev/cu.usbmodembench* >/dev/null 2>&1; then
        ok "USB console present ($(ls /dev/cu.usbmodembench* | head -1))"
    else
        note "  [warn] no /dev/cu.usbmodembench* — USB cable unplugged, or the CDC"
        note "         link dropped. Recovery still works: POST /api/bootsel, or"
        note "         'just bootsel' once the port is back."
    fi

    if [ "$fail" -ne 0 ]; then
        note "RESULT: FAIL"
        exit 1
    fi
    note "RESULT: PASS"
