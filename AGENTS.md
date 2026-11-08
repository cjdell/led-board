# AGENTS.md — led-board

Rust firmware for a Raspberry Pi Pico 2 W LED art board (embassy, `no_std`)
with an optional ESP32-C3 slave-radio mode. Read this before building or
flashing anything; most failure modes here are tooling traps, not code bugs.

## Repository layout

| Path | What it is |
|---|---|
| `firmware/` | Pico 2 W (RP2351) app. Bin target `ws`. Feature matrix below. |
| `radio_link/` | Shared `no_std` framed-protocol crate (Pico ↔ ESP32 slave radio over UART). |
| `slave_radio/` | ESP32-C3 firmware (esp-hal, RISC-V). **Detached workspace** — excluded from the root workspace and has its own `Cargo.lock` + `.cargo/config.toml`. |
| `sim/`, `ws_client/` | Host-side simulator and client. |
| `README.md` | Authoritative pinout + wiring + architecture diagrams. |

## Toolchain traps (read first)

- **Always** `export PATH="$HOME/.cargo/bin:$PATH"` before cargo commands.
  The interactive shell otherwise picks up Homebrew `/opt/homebrew/bin/rustc`,
  which has no embedded targets.
- **Always** pass `--target thumbv8m.main-none-eabihf` for `firmware` builds —
  `[build] target` in `.cargo/config.toml` is commented out. Without it, cargo
  builds for the host and you get confusing `cortex-m` E0425 / "invalid
  register r0" errors.
- Run host tests from the **repo root**: `cargo test -p radio_link`. From
  inside `slave_radio/` its `.cargo/config.toml` forces the RISC-V target and
  everything fails to link.
- `rustfmt` runs on save and reorders imports — re-read a file immediately
  before `edit_file` matches or fuzzy-match edits will miss.
- Rust size/objcopy tooling: Apple's `size` can't read ELF. Use
  `$(rustc --print sysroot)/lib/rustlib/<host>/bin/llvm-size` and
  `llvm-objcopy` (or `cargo objcopy` / `cargo-binutils`).

## Building

Feature matrix for `firmware`: pick one transport + one network.

- Transports: `defmt_rtt` (needs an external debug probe), `defmt_tcp`
- Networks: `wifi` (cyw43, flaky — legacy), `usb_ethernet`, `ppp`, `radio` (ESP32-C3)

```sh
export PATH="$HOME/.cargo/bin:$PATH"
cargo check -p firmware --features radio,defmt_rtt --target thumbv8m.main-none-eabihf
```

C3 firmware (its `.cargo/config.toml` pins the C3 target). For the **C6**
build pass the target explicitly — its `.cargo/config.toml` default does not
match (C6 is `imac`, C3 is `imc`):

```sh
cd slave_radio && cargo build --release                    # C3
cd slave_radio && cargo build --release --no-default-features \
    --features esp32c6 --target riscv32imac-unknown-none-elf  # C6-Zero
```

A Nix flake is provided (`nix develop`, fenix toolchain with all targets) but
the plain rustup toolchain from `rust-toolchain.toml` works too.

## Flashing — Pico 2 W

The board uses RP2350 **partition-table slots**; the app image loads at
offset `0x10001000` (set up once via `just setup_flash`, already done on this
bench). Normal flow:

```sh
just main          # interactive fzf picker: transport, network → build → flash → probe-rs attach
```

Or manually:

```sh
cargo objcopy --release -p firmware --features radio,defmt_rtt --bin ws \
    --target thumbv8m.main-none-eabihf -- -O binary tmp/ws.bin
picotool load --offset 0x10001000 tmp/ws.bin    # BOOTSEL not required for this image
picotool reboot
picotool info      # shows detected partitions/slots
```

### Buttonless flash cycle (no BOOTSEL button on the art board)

The app (`usb_dbg.rs` `ResetIface`) exposes a vendor control interface on the
USB console (VID/PID **16C0:0493**). Full working cycle:

```sh
just bootsel                     # picotool reboot -u -f --vid 5824 --pid 1171
picotool info --all              # poll until exit 0 (BOOTSEL can lag enumeration seconds)
picotool load --offset 0x10001000 tmp/ws.bin   # auto-resets the chip on success
picotool reboot -a -f            # no vid flags needed for RP2350 Boot (2E8A:000F)
```

`scripts/flash_both.sh` automates C6 + Pico with the polling. Traps:

- The `-u` reboot **must** pass `--vid 5824 --pid 1171`: picotool 2.3.0 only
  scans Raspberry Pi VID unless given explicit ids, and the console lives on a
  Generic (16C0) id. Keep console VID 16C0:0493 — app-mode **2E8A:* devices
  bind nowhere** on this Mac (driver claimed by probe-rs/DriverKit → device
  invisible; verified the hard way).
- If after `load` no `/dev/cu.usbmodembench*` (Pico console, serial "bench")
  appears within ~5 s: `picotool reboot -a -f`. If ioreg shows "RP2350 Boot"
  but picotool says "no accessible devices", the ROM is wedged — power-cycle
  the Pico (hub power-cycle if both micros vanished from `ioreg -p IOUSB`).

OTA over the network: `just ota` (builds, then `curl POST /api/ota`).

Keep `tmp/ws.elf` (copy of the release ELF) for `probe-rs attach`/`defmt-print`.

## Flashing — ESP32-C3 (`slave_radio/`)

```sh
just radio-flash    # build + espflash flash --partition-table partitions.csv (explicit!)
```

- **Never trust `cargo flash` here**: the `runner`'s `--partition-table` flag is
  silently ignored, and the chip gets the *default* table (factory app at
  `0x10000`) — which **collides with the `cfg` journal** also at `0x10000`.
  Verify a flash used the custom table: espflash reports app partition capacity
  `4,063,232` (= `0x3e0000`) when applied, vs `4,128,768` for the default.
  `partitions.csv` gotchas: esp-idf-part rejects custom data subtypes like
  `0x40` (use `undefined`); `espflash erase-region` takes positional
  `<addr> <size>`; wipe stored WiFi config with
  `espflash erase-region 0x10000 0x4000`.
- Flashes over the C3's **native USB-JTAG** port (`/dev/cu.usbmodem…`,
  `espflash list-ports` finds it). ~7 s for the ~690 KB image.
- `espflash board-info -p <port>` is a safe health check; it ends with a
  hard-reset back into the app.
- Config store lives in flash at `0x10000` (`cfg` partition). Factory config
  boots into AP **"LED Board"** at `192.168.2.1` (mDNS `led-board.local`),
  captive portal included; saved-network failures fall back to the same AP.

## Monitoring — C3 (important quirks)

- Console is USB-JTAG (`esp-println` `jtag-serial` feature — do **not** move
  it to UART0: UART0's default pins GPIO20/21 are the link pins to the Pico).
- Native USB-JTAG **enumeration drops on every chip reset**. A monitor
  attached across a reset sees nothing. **Opening** the port also resets the
  chip (DTR auto-reset circuit) — exploit it: with `cat` attached, every
  reconnect replays the boot sequence live.
- Logs emitted while no host is attached are dropped (esp-println is
  non-blocking by design, 64-byte FIFO) — and second-stage bootloader logs
  fill that FIFO before the app prints anything. Consequence: **panics are
  invisible on a cold boot**. Bring-up scaffolding exists in
  `src/bin/slave_radio.rs`: a 6 s boot delay + `stage:` `println!` breadcrumbs
  + a 5 s `hb:` heartbeat (mode/ap_up/joined/pico_online/rx counters/heap_free)
  so attaching a listener shows exactly where boot stalls. Capture pattern:
  `cat /dev/cu.usbmodemNNN & sleep 16; kill %1`.
- mDNS: `led-board.local` takes ~5 s to resolve from this Mac (unrelated to
  the link) — script against the C6's DHCP IP instead.
- **Never touch the Mac's WiFi** (`networksetup -setairportnetwork` …) — it is
  the agent's own uplink; AP-mode tests run from the user's phone while the
  Mac stays on the LAN.
- Log level is baked at build time from `ESP_LOG` in
  `slave_radio/.cargo/config.toml` `[env]` (= info). Changing it requires a
  rebuild.
- `espflash monitor` needs a real TTY (fails when piped). For scripted capture
  use `script -q out.log <cmd>` or `/usr/bin/expect`.
- Killing the terminal mid-`cat /dev/cu.usbmodem…` can orphan a `cat` that
  holds the port; `pkill -f 'cat /dev/cu'` if espflash can't open it.

## Hardware pins (see README.md diagram for truth)

- Pico UART0 → C3 link: **GP12 = TX → C3 GPIO20**, **GP13 = RX ← C3 GPIO21**
  (C3 uses UART1 there; its GPIO matrix allows other free GPIOs too).
  **460800 baud** (`radio_link::BAUD`) — 921600 tested clean-by-CRC at single
  loads but tripped ~1 mid-frame corruption per 10–15 loads on this bench's
  jumpers; re-test after wiring rework before raising it. GP0/GP1 are
  deliberately left free for a debug console.
- GP2–GP9: WS2812 STRIP 0–7 (PIO1). GP23/24/25/29: cyw43 (wifi builds only).
- C3 GPIO8: status LED (active-low). GPIO18/19: USB (console). Avoid
  GPIO 2/8/9/11 for new wiring (boot strapping) and 26–31 (flash).
- RP2351 legal UART pins are fixed-function; the authoritative list is the
  `impl_pin!` table in the locked embassy-rp checkout (`uart/mod.rs`).

## Firmware sizes (release, Nov 2026)

Pico wifi ≈ 501 KB, Pico radio ≈ 408 KB (4 MB flash); C3 ≈ 690 KB
(~17% of app partition, ~338 KB of ~400 KB RAM). No size pressure anywhere.

## Protocol notes

- Embassy-net socket budgets: `StackResources<N>` in `src/bin/slave_radio.rs`
  is the smoltcp **max socket count per stack** (AP: dhcp + dns + mdns + 2
  acceptors + clients → 12; STA: mdns + 3 acceptors + clients → 16). Exceeding
  it panics (`SocketSet full`) — size new socket-using services before adding them.
- UART frames: `[0xBEEF][type u8][stream u16][len u16][crc16 LE][serde_json]`,
  defined once in `radio_link/` — change both sides together and bump
  `HelloInfo` version checks if the wire format changes. Protocol v3:
  `STREAM_ACK` frames (payload: cumulative u32 LE **frames** consumed) window
  stream data at `radio_link::STREAM_WINDOW_FRAMES` (4 frames) un-acked per
  stream per direction. The window counts **frames, not bytes** (v2's 8 KB
  byte window let small frames — 15 B HTTP header writes — pipeline more
  frames than the queue has slots → burst overruns and truncation): queue
  depth is derived from the same constant on both sides, so overflow is
  unreachable for any frame sizes. 4 frames ≈ 90 ms of buffering at 460800 —
  throughput stays line-rate bound. The link is **lossless by construction**:
  `FRAME_TX.send` blocks (never drops), and any anomaly (CRC fail, overrun,
  30 s without acks) closes *that stream* — one retriable request, never
  silent corruption. Don't re-add timeout/drop heuristics; that was the
  pre-v2 bug (pages truncated at ~16 KB).
- **Heap bounds on the radio**: stream queues are per-frame heap `Vec`s inside
  the C3/C6's 112 KB heap. Worst case (slots × pool × 1 KB + `FRAME_TX`
  backlog) once OOM-panicked the chip — a WiFi disconnect mid-load brought up
  the fallback AP, and the APSTA combo mode pins tens of KB (measured: idle
  AP+STA `heap_free` ≈ 9 KB vs 19 KB STA-only). The supervisor now **disables
  the fallback AP after a successful STA join** (Station mode; ApStation keeps
  it) via an APSTA→STA `set_config`. The `hb:` heartbeat logs `heap_free` —
  watch it under load; floor during the 4-way burst suite ≈ 14 KB.
- Acceptance harness: `./scripts/stress.sh` — captures both consoles, waits for the
  STA lease to answer (preflight), runs 3 single + 3×4-burst page loads
  (73,541 B), then requires one sha256 group, 0 curl failures, and 0
  `corrupt|gap|overran|timeout|abort` trips in either log. Pico build must use
  `--features radio,defmt_tcp,console` (the USB console carries the guard
  logs; C6 port = the non-bench `/dev/cu.usbmodem*`, renumbering is handled).
  C6 console `cat` resets the chip on open — the 18 s "booting..." window
  covers the rejoin.
- Pico runs no IP stack in `radio` mode; the C3 terminates TCP and relays
  sockets to picoserve on the Pico. Provisioning API (`/api/wifi`, `/api/status`,
  `/api/networks`, `/api/forget`) is answered by the C3 itself so the portal
  works even with the Pico unpowered.
