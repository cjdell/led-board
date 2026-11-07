# AGENTS.md — led-board

Rust firmware for a Raspberry Pi Pico 2 W LED art board (embassy, `no_std`)
with an optional ESP32-C3 slave-radio mode. Read this before building or
flashing anything; most failure modes here are tooling traps, not code bugs.

## Repository layout

| Path | What it is |
|---|---|
| `firmware/` | Pico 2 W (RP2351) app. Bin target `ws`. Feature matrix below. |
| `radio_link/` | Shared `no_std` framed-protocol crate (Pico ↔ C3 over UART). |
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

C3 firmware (always fine, its config pins the target):

```sh
cd slave_radio && cargo build --release
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
  + a 5 s `hb:` heartbeat (mode/ap_up/joined/pico_online) so attaching a
  listener shows exactly where boot stalls. Capture pattern:
  `cat /dev/cu.usbmodemNNN & sleep 16; kill %1`.
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
  921600 baud. GP0/GP1 are deliberately left free for a debug console.
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
  `HelloInfo` version checks if the wire format changes.
- Pico runs no IP stack in `radio` mode; the C3 terminates TCP and relays
  sockets to picoserve on the Pico. Provisioning API (`/api/wifi`, `/api/status`,
  `/api/networks`, `/api/forget`) is answered by the C3 itself so the portal
  works even with the Pico unpowered.
