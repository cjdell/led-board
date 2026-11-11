# AGENTS.md — led-board

Rust firmware for a Raspberry Pi Pico 2 W LED art board (embassy, `no_std`)
with an optional ESP32-C3 slave-radio mode. Read this before building or
flashing anything; most failure modes here are tooling traps, not code bugs.

## Repository layout

| Path | What it is |
|---|---|
| `led_core/` | **All system logic** (`no_std`): picoserve web app, WebSocket pump, display worker, heartbeat, config, screen model. Platform boundary = traits (`Board`, `OtaBackend`, `LocalFsTrait`, `Ws2812ParallelDriver`). Compiles for both thumbv8m and the host (`--features std`). |
| `firmware/` | Pico 2 W (RP2351) hardware impl: bin target `ws`, PIO WS2812 driver, flash stores, `Board` impls (radio/cyw43), embassy-net + C3-tunnel transports. Feature matrix below. |
| `sim/` | macOS full-system simulator (`just sim` — dot-matrix window + real web UI on :8080) + old `animation_dev` scratchpad. |
| `radio_link/` | Shared `no_std` framed-protocol crate (Pico ↔ ESP32 slave radio over UART). |
| `slave_radio/` | ESP32-C3 firmware (esp-hal, RISC-V). **Detached workspace** — excluded from the root workspace and has its own `Cargo.lock` + `.cargo/config.toml`. |
| `embedded_tools/` | `LocalFs`/`ConfigFile` + the **log facade** (`use embedded_tools::{info,warn,error,debug,Debug2Format}`; defmt on embedded, println on host — feature `defmt` xor `std`). |
| `ws_client/` | Host-side client. |
| `web/` | SPA (Solid + TypeScript, Deno/vite, no Bootstrap — custom theme in `src/theme.scss`). `web/compile.sh` → `web/bundle/index.html.gz` (gitignored, include_bytes'd into firmware + sim). `deno task dev` = :3000 dummy API; `deno task check` / `fmt`. |
| `docs/slave-radio.md` | ESP32 slave radio: wiring, architecture, provisioning/portal behaviour. |
| `README.md` | Authoritative pinout table + architecture diagrams (Mermaid). |

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

## led_core + sim: the firmware logic on the Mac (Nov 2026)

`led_core` holds the whole app; `firmware` and `sim` are two platform
impls. Run the board without hardware:

```sh
just sim      # dot-matrix window + http://localhost:8080 (PORT=… to change)
cargo check -p sim                                   # host typecheck
cargo check -p led-core --target thumbv8m.main-none-eabihf         # embedded build
cargo check -p led-core --no-default-features --features std       # host build
```

Traps in this layer (all verified):

- **defmt does not link on the host.** Shared crates log through the
  embedded_tools facade (`use embedded_tools::{info, warn, error, debug,
  Debug2Format};`), never `use defmt::*`. Feature split: embedded_tools
  `defmt` xor `std`; led-core default = embedded (defmt), sim builds it
  `default-features = false, features = ["std"]`. Firmware bins keep
  plain `use defmt::*` — they're embedded-only.
- **embassy timers are awaited from two runtimes on the sim** (picoserve's
  ws timeouts poll on the tokio thread) → `embassy-time` needs
  `generic-queue-8` there, else it panics "waker not created by the
  Embassy executor".
- **`#[embassy_executor::task]` pools cannot be generic** — led-core
  exposes plain async fns (`display::run`, `pump::run`, `heartbeat::run`,
  `http::web_task_body`) and each platform spawns concrete wrapper tasks
  (`ActiveBoard` alias keeps them one type per binary).
- **std `Executor::run` demands `&'static mut self`** — leak it on its
  thread. The sim's display driver awaits a 2 ms timer per frame: without
  a yield in `WindowDisplay::write` the animation loop monopolises the
  cooperative executor (this is free on the Pico, where the PIO DMA wait
  paces the loop).
- The sim hands the leaked `&'static AppRouter` to the tokio thread as a
  raw pointer (mpsc `usize`) — same pattern as the Pico's TUNNEL_APP; the
  router is single-owner afterwards, so it needs no `Sync`.
- **The sim serves at `http://localhost:8080/` — same host as the dummy-API
  rule in `web/src/lib/device/index.ts`.** A page opened at `localhost` with a
  port **other than 3000** now uses the *real* API; only `localhost:3000`
  (vite dev) stays on dummy data. `?device=real|dummy` overrides.
- The sim **defaults to full brightness** (posts `PowerLimit(1.0)`, unlike the
  dark-guarded device); `SIM_POWER_LIMIT=0.1 just sim` restores device behaviour.
- OTA on the sim is real end-to-end: `curl -X POST --data-binary @img
  "http://localhost:8080/api/ota?$(sha256sum img)"` → verified write to
  `tmp/host_board_firmware.bin`, then `/api/reboot` exits the sim.
- picoserve (cjdell fork) already ships a **`tokio` feature**
  (`Server::new_tokio`, `Socket for tokio::net::TcpStream`) — that's the
  sim's transport; `CONFIG`/`WEB_TASK_POOL_SIZE` are re-exported from
  `led_core::tasks::http` so both platforms share the timeouts.

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

OTA over the network: `just ota` (builds, then a paced `curl POST /api/ota`).

**Recovery hatches (Nov 2026, no BOOTSEL button needed):**
- `POST /api/bootsel` reboots into the ROM bootloader over the network
  (`Board::bootsel`, `rom_data::reset_to_usb_boot`). Works whenever the app is
  alive.
- `just bootsel` (the vendor ResetIface in `usb_dbg`) was verified to work
  **even when the executor is wedged** — the USB stack still services the
  control request. Try it before asking for a physical BOOTSEL.
- `no_display` feature: boots WiFi + web + USB and never starts the panel
  worker (`cargo objcopy … --features wifi,defmt_tcp,console,no_display`).
  The safety image when a display/DMA fault would otherwise hang boot.
- The WebSocket/status feeds stay at defaults in that image (no display
  worker), so `/api/status` can time out — `/api/version`, `/api/ota`, and
  `/api/bootsel` are the ones that matter.
- **Do not enable `usb_core1`**: moving the console to core 1 froze the
  display path right after the first frame (reproduced twice; bisected — the
  same code on core 0 runs at 90 fps). The feature exists only as opt-in.

**OTA reboot traps (datasheet 5.4.8.24 / RP2350, verified on the bench):**
`rom_data::reboot()` is *asynchronous* — it arms the **watchdog** and returns;
the boot path reads `p0`/`p1` from watchdog scratch registers after the reset.
Consequences, both of which silently left the board running the old image:
1. Any task that keeps feeding the watchdog (`watchdog_runner`, every 5 s)
   reloads the counter and **cancels the pending reboot**. Reboot paths set
   `rp235x_ota::REBOOTING`; feeders must skip feeding while it is set.
2. For `REBOOT_TYPE_FLASH_UPDATE` (0x4), `p0` is *the start address of the
   updated flash region*, compared against the partition table's **storage**
   sector addresses (A = 0x1000, B = 0x100000 — `get_first_last_bytes()`
   already returns these; do **not** add `FLASH_BASE`/XIP). Passing an XIP
   address (0x10100000) matches nothing, so the ROM normal-boots the old slot.
Working signature: `reboot(0x4 | 0x100, 1000, partition_start_addr, 0)`; the
reset shows as `Reset Reason: Some(TimedOut)` followed by
`Rp235xOta: Marking this firmware as good...`.

**OTA upload reliability (Nov 2026, all four bugs fixed):**
- `embassy_rp::flash::blocking_write` **hangs** on sub-page tails (<256 B):
  the program routine never returns with interrupts off, and the watchdog
  resets 15 s later (`TimedOut`, no panic, B's first sector erased). Fix:
  `Rp235xOta::write_chunk` always stages a full 0xFF-padded 4096-byte sector
  and advances `position` by the real byte count, so the SHA still covers the
  image only. Never hand `blocking_write` a short chunk.
- The handler must **never call `read` past `Content-Length`**: picoserve's
  body reader blocks on the socket after the declared body is consumed when
  the client keeps the connection alive, which wedged a web task forever.
  Bound every read loop by `reader.content_length()`.
- Flash sectors erase with interrupts off for ~30 ms. Pace uploads: server
  yields 20 ms per sector, and the *client* must be rate-limited —
  `curl --limit-rate 24k` (`just ota` does this). An unpaced 600 KB POST
  overruns the cyw43 RX path in this embassy pin and the TCP stream stalls
  after ~12 KB, permanently.
- **`z=raw` (DEFLATE upload) is disabled**: `InflateState` is ~44 KB and its
  constructor builds it on the stack before boxing, which resets the chip
  before any log can escape (looks like a watchdog reset, no panic record).
  POST the raw image. Revisit only with a static/boxed state allocated off
  the task stack.
- `just ota` posts `tmp/ws.bin` raw at 24 KB/s: ~28 s for a 666 KB image.

**Crash forensics**: `console` builds keep the last panic/hard-fault text in
`.uninit` RAM (survives a reset) and print `prev boot: …` on the next start.
`usb_dbg`'s `HardFault` handler records a marker too — a stack overflow
otherwise just hangs and the watchdog blames itself.

**Heap telemetry**: `watchdog_runner` logs `heap: free=… used=…` over the USB
ring every 30 s. Steady state with display + 5 web tasks is ~47 KB free of
192 KB. `defmt_tcp`'s frame channel is 256 slots (was 2000): with no
`defmt-print` client nothing drains it and 2000 queued `Vec`s pinned ~110 KB.

Keep `tmp/ws.elf` (copy of the release ELF) for `probe-rs attach`/`defmt-print`.

## Flashing — ESP32-C6 / C3 (`slave_radio/`)

Wiring, architecture and provisioning behaviour:
[`docs/slave-radio.md`](docs/slave-radio.md).

```sh
just radio-flash    # build + espflash flash --partition-table partitions.csv (explicit!)
```

- **Never trust `cargo flash` here**: the `runner`'s `--partition-table` flag is
  silently ignored, and the chip gets the *default* table (factory app at
  `0x10000`) — which **collides with the `cfg` journal** also at `0x10000`.
  `partitions.csv` gotchas: esp-idf-part rejects custom data subtypes like
  `0x40` (use `undefined`); `espflash erase-region` takes positional
  `<addr> <size>`; wipe stored WiFi config with
  `espflash erase-region 0x10000 0x4000`.
- Flashes over the C6's **native USB-JTAG** port (`/dev/cu.usbmodem…`,
  `espflash list-ports` finds it). ~7 s for the ~690 KB image.
- `espflash board-info -p <port>` is a safe health check; it ends with a
  hard-reset back into the app.
- Config store lives in flash at `0x10000` (`cfg` partition). Factory config
  boots into AP **"LED Board"** at `192.168.2.1` (mDNS `led-board.local`),
  captive portal included; saved-network failures fall back to the same AP.

### A/B partition table + radio OTA (Nov 2026)

`partitions.csv`: nvs(0x9000) / otadata(**0xd000**) / phy / cfg(0x10000,
unchanged) / **ota_0@0x20000** / **ota_1@0x210000**, each `0x1F0000` (~2 MB);
no factory app. espflash confirms the custom table with app capacity
**2,031,616** (= `0x1F0000`).

USB flash on this table **must name the slot** and clear otadata for a
deterministic boot (both are built into `scripts/flash_both.sh` and the
`radio-flash`/`radio-c6-flash` just recipes):

```sh
espflash flash --partition-table partitions.csv --target-app-partition ota_0 …
espflash erase-region -p <port> 0xd000 0x2000   # empty otadata → boots ota_0
espflash reset -p <port>
```

**OTA over the network**: `just radio-ota` — builds the C6 image,
`espflash save-image --chip esp32c6 <ELF> tmp/radio.bin`, then
`curl POST /api/ota-radio` to the board's IP. The C6 streams the image into
the *inactive* slot (`src/ota.rs`), flips otadata, reboots; `GET
/api/ota-radio` returns `{version, slot, update_writes_to}` — check `slot`
flipped. STA acceptor peeks the request head and serves the ota-post
*locally even when the Pico is offline*; `/api/ota-radio` is portal-claimed
so it also works on the fallback AP (POST via phone won't work — needs the
ELF — but GET status does).

- **Boot confirm/rollback hatch**: the app marks the running slot `Valid` at
  boot (after `esp_rtos::start`, before anything touches flash).
  The prebuilt bootloader has **no auto-rollback**, so a bootable-but-broken
  image stays booted — the manual recovery is to **hold GPIO9 (C6 BOOT strap,
  input-pull-up after boot) LOW across a reset**: the boot guard then flips
  otadata to the other slot and reboots. C6-Zero may have no physical button
  — bridge GPIO9→GND before power-up/reset, or just re-flash over USB.
- The OTA write holds the **store mutex** for the whole upload (~30 s on
  WiFi): the journal and portal API block meanwhile — by design; the chip
  reboots at the end regardless.
- Don't `erase-region 0xd000` while inspecting slot state after an OTA —
  that resets selection to ota_0.

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

## Hardware pins (see README.md pinout table for truth)

Radio-link wiring and the C3/C6 side of the link:
[`docs/slave-radio.md`](docs/slave-radio.md).

- Pico UART0 → C3 link: **GP12 = TX → C3 GPIO20**, **GP13 = RX ← C3 GPIO21**
  (C3 uses UART1 there; its GPIO matrix allows other free GPIOs too).
  **460800 baud** (`radio_link::BAUD`) — 921600 tested clean-by-CRC at single
  loads but tripped ~1 mid-frame corruption per 10–15 loads on this bench's
  jumpers; re-test after wiring rework before raising it. GP0/GP1 are
  deliberately left free for a debug console.
- GP2–GP9: WS2812 STRIP 0–7 (PIO1). GP23/24/25/29: cyw43 (wifi builds only).
- C3/C6 GPIO8: status LED (active-low). GPIO18/19: USB (console). C6 **GPIO9**
  is the OTA boot-guard input (BOOT strap; held low across reset = flip A/B
  slot) — do not wire elsewhere. Avoid GPIO 2/8/11 for new wiring (boot
  strapping) and 26–31 (flash).
- RP2351 legal UART pins are fixed-function; the authoritative list is the
  `impl_pin!` table in the locked embassy-rp checkout (`uart/mod.rs`).

## Dependency state (Nov 2026 audit)

- **embassy is pinned at `9ae6c57` (2026-07-12) and cannot float to main**:
  embassy-net has been rewritten onto the new `xarxa` stack (smoltcp-era
  features like `icmp`/`raw` gone) — blocks the picoserve + edge-net forks and
  our `StackResources` usage. The October cyw43 robustness fixes (join-cleanup,
  DIO-low reset, pool sizing) sit behind that line; revisit when the forks
  catch up. Our pin *does* contain the two perf fixes: #4792 (gSPI PIO program
  selection) and #6001 (RP2350 RX DMA corruption).
- **cyw43 gSPI now runs `DEFAULT_CLOCK_DIVIDER`** (÷2 → 75 MHz SM → 37.5 MHz
  GSPI: the programs cost 2 SM cycles per SPI bit, so GSPI = clk/div/2;
  in-spec, integer divider → sample-on-low high-speed program). The old
  `RM2_CLOCK_DIVIDER` (25 MHz GSPI) was a workaround for issue #3960, written
  before the pin contained #4792/#6001. See fletcher.cool/blog/pico-2w-wifi-throughput.
- **wifi web tasks use 8 KB TX / 4 KB RX picoserve buffers** (was 1 KB — that
  was a buffer/RTT cap around 1 Mbit/s regardless of bus speed). Heap is fixed
  at 192 KB (`embedded_alloc::init!`) — web pool × buffers must fit in it.
- **The app slot is 1020 K (`memory.x`), not 4 MB** — the A/B partition table
  (`partitions.json`) now gives each slot `0xFF000` (1,044,480 B) and the linker
  region matches. The 508 K slot was the old constraint; it is gone. The
  partition table is physical flash state, so **run `just setup_flash` once
  after pulling this change** (A/B start/size changed) before flashing or OTAing
  the larger image — an old 508 K table will reject anything over 508 K.
  Wifi builds are `wifi,defmt_tcp` and logs come via
  `defmt-print ... tcp --port 19021`. `scripts/stress_wifi.sh` captures it and
  reports per-load throughput with the stress.sh acceptance bar.
- **Bundle size note (Nov 2026, festival scene set)**: `web/compile.sh` now
  produces a **45,790 B** gzipped single-file bundle (40,566 B before the new
  animation previews). Re-run `web/compile.sh` before flashing UI changes; the
  sim embeds the same gz.
- **Wi-Fi acceptance PASSED (Nov 2026)**: `HOST=http://<ip>/ ./scripts/stress_wifi.sh`
  → 15/15 identical 73,541 B loads (pre-truncate-fix number; loads are now
  40,566 B after the UI rewrite — re-run acceptance at the new size), one sha256 group, 0 curl failures, 0 guard
  trips. Singles ≈0.16 s (≈3.3–3.7 Mbit/s), 4-bursts ≈0.3–0.5 s (≈1.2–2 Mbit/s)
  — 7–10× the radio relay (≈46 KB/s ≈ 0.37 Mbit/s at 460800 UART), so the slave
  radio is demoted to a provisioning/legacy option, not the throughput path.
  Burst-load defmt shows `failed to push rxd packet` + `unexpected ethernet
  type 0x0000` warnings = cyw43 RX pool pressure (the retransmit tail); the
  October embassy pool fixes address exactly this, still behind the xarxa line.
- **Wifi builds provision natively + web UI reworked (Nov 2026)**: the SPA's
  `/wifi` route is enabled (nav tab "WiFi"): auto-scan on open, signal bars,
  inline password entry, manual-SSID fallback, saved-network list with
  Forget, and a status card polling `/api/join-status` every 4 s (attempts,
  last `JoinError`, IP, AP-fallback) — readable from a phone browser in AP
  fallback, the only join-forensics channel without a console. Backend:
  `GET /api/wifi` (cyw43 scan), `POST /api/wifi` (persist + reboot into the
  join loop), `GET /api/networks` (SSIDs only), `POST /api/forget`, all in
  `firmware/src/tasks/http/wifi.rs`. `add_known_wifi_network` always sets
  `wifi_mode: Station` (pulls an AP-mode board back to joining on reboot).
  Forget takes effect next boot — the join loop reads config once at boot.
- **Flaky-cyw43 join policy (Nov 2026)**: the station phase now makes **five
  passes** over `config.known_wifi_networks` (10 s per network, 500 ms between
  attempts) before falling back to the "LED Board" AP; each pass logs/increments
  the old attempt counters. Status cards are `DisplayWorkerMessage::StatusTimed`
  now: join/lease/link-lost cards clear after 10 s, AP cards after 15 s, so the
  panel always returns to the festival playlist even when the network never
  comes up. The AP itself stays up for provisioning; `/api/join-status` still
  reports `ap_fallback`.
- **esp-hal stays at tag `esp-hal-v1.1.2`**; v1.2.0 exists (Sep 2026) but the
  port is a full radio session with the bench online: typed `Ssid`/`Password` +
  `AuthenticationMethodConfig` configs, `DmaAlignedMut` descriptor buffers,
  `SoftwareInterruptControl` removed, `esp_radio::wifi::new` renamed,
  `Interface` lifetime changes. Supervisor-only edits do not compile.

## Firmware sizes (release, Nov 2026)

Pico wifi ≈ 484 KB, Pico radio ≈ 451 KB (old 508 K slot). After the led-core
refactor + clean bundle: radio 447,144 B, wifi 485,796 B. After the control-
deck UI rewrite (40.5 KB gz bundle): **wifi 500,288 B, radio 423,812 B**.
After the frame-pipeline work (pipelined DMA driver, `animations` crate
`opt-level = "s"` — deliberately per-crate, `opt-level = 2` costs +7.6 K):
**wifi 504,056 B, radio 425,468 B**. Version-stamp build-info + boot splash
and the latch-safe driver revision: **wifi 507,180 B, radio 429,100 B** (the
old 508 K slot was the hard ceiling here). After the Nov 2026 festival-scene
set + enlarged 1020 K A/B slots: **wifi 573,428 B (~471 K headroom), radio
532,016 B**, and the bench **wifi,defmt_tcp,console** image is 663,000 B
(45.8 KB gz bundle). After the Nov 2026 OTA/recovery work (crash log,
sector-padded OTA, heap telemetry, paced upload): **wifi 576,696 B, bench
console 667,480 B**. The old wifi tightness is gone; the next ceiling is the
1020 K slot, not the SPA. Console builds now fit comfortably again.
C3 ≈ 690 KB (~17% of app partition, ~338 KB of ~400 KB RAM). No size pressure
anywhere else.

## Protocol notes

- **Panel gamma (Nov 2026)**: animation buffers are authored in sRGB, but
  SK6812-class strips are linear-light devices. Hardware builds now
  gamma-encode (γ 2.0) inside `apply_power_limit` (the `std` sim skips it
  because a monitor already applies sRGB). The same function sums the *PWM*
  values for the power limit, so a 10% limit no longer washes the mid-tones
  out. The festival scenes are authored with dark backgrounds and bright
  subjects directly; if they look too bright on the panel, fix the authored
  colours — do not stack another gamma/contrast stage.
- **Panel frame pipeline (Nov 2026)**: `ws2812p::write` is pipelined fire-and-
  forget — transpose into the idle DMA buffer (overlaps the previous frame on
  the wire), busy-poll the DMA channel for completion, **wait the enforced
  1.5 ms latch gap**, then arm the next `dma_push` and **`mem::forget` the
  returned `Transfer`** (dropping it runs the channel-abort path and kills
  the frame — do not "fix" the leak; the completion signal is the busy-bit
  poll). The latch gap must be CPU-enforced: **these strips are SK6812-class
  clones needing ≳1 ms LOW to latch, not the WS2812B 50 µs** — a purely
  in-band 328 µs pad made the whole 104-fps stream look like one never-
  latching data frame (visible refresh ≈ 1 Hz, top-of-frame-only partials,
  static content deceptively perfect; field forensics 2026-11-11: status
  clock real-time + ledger 1047 frames/10 s while the panel crawled). The
  old serial driver only "worked" because its render+transpose pause left
  4–9 ms LOW at every boundary. The buffer tail keeps 8 pad words (41 µs)
  purely as FIFO-drain slop absorber. The PIO program keeps its explicit
  loop-top `PULL` (BLOCK) at one per OSR word: 41 cycles per 4 bit periods
  (~780 kHz, 9.23 ms wire; + pad + gap ≈ 10.8 ms/frame ⇒ ~92 fps ceiling),
  frame cycle = max(wire+pad+gap, CPU). **Do not switch to auto_fill/PULL-less
  loops**: the `OUT` is the only stalling instruction left and it sits right
  after the T1 all-high pulse — an empty FIFO parks the pins HIGH, and a
  >50 µs high pulse is data, not reset: the Nov 2026 optimisation attempt did
  exactly that and the panel showed artefacting / out-of-step rows until
  reverted. If render+transpose ever exceeds wire time the result is a torn
  frame, not a hang. The 10 s ledger line reports `draw … us/frame, panel
  … us/frame` to tell which side is the clock. `LedScreen::copy_eg_buffer`
  takes `&[Rgb888; TOTAL_PIXELS]` (the per-frame 7.2 KB `collect()` is gone).
- **Web live-status protocol (Nov 2026)**: the display worker is the single
  source of truth and publishes `StatusWatch`/`PlaylistWatch` (pre-serialised
  JSON `Arc<str>`s in `led_core::types`); the WS pump pushes them every 1 s +
  on change as tagged frames `{"status":{…}}` / `{"playlist":{…}}`, and
  `GET /api/status` serves the same snapshot. Client→server WS messages:
  `"Ping"|"Next"|"Reset"|"ResumeNow"|{Pause:bool}|{JumpTo:u32}|
  {Animation:[anim,ms]}|{Playlist:{playlist,save}}|{PowerLimit:f32}`.
  REST: `/api/playlists[/<name>]` CRUD (named library = plain
  `playlists/<name>.json` files), `/api/info/playlist` = **live** playlist
  `{playlist,source,index}`, source `Saved|Default|Live`. `power_limit_pct`
  is a u8 in JSON. `PlaylistItem` = `{animation, duration_ms, label?}` with
  a serde visitor still accepting legacy `[animation, ms]` tuples (old flash
  files + ws_client parse unchanged); durations clamp to 0.5 s–15 min.

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
  The global allocator is **TLSF** (`ESP_ALLOC_CONFIG_HEAP_ALGORITHM` in
  `.cargo/config.toml`) — the default LLFF list allocator OOM-panicked mid-
  suite while `heap_free` still read ~10 KB (fragmentation: a 1 KB flash-write
  scratch found no contiguous block). Changing the value needs `cargo clean -p
  esp-alloc`. `heap_free` is a *sum* — it can lie while the largest free block
  is tiny; suspect fragmentation first when small allocs fail.
- **read_head buffering rules** (`slave_radio/src/httpd.rs`): the acceptor
  peeks the request head on every connection (STA *and* AP) so `/api/ota-radio`
  is served by the radio itself. Consequences to remember: (1) `parse_head`
  must slice the head in BYTES before `from_utf8` — a POST prefix carries
  binary body and UTF-8-failing-the-whole-buffer silently became `GET /` and
  relayed firmware uploads to the Pico (404); (2) bodies > 4096 B are NOT
  buffered into `prefix` (a growing prefix realloc OOM-panicked the chip
  mid-upload; `ota::update` and the relay stream the rest from the socket);
  (3) connections not starting with an HTTP method token are RST'd — stale
  same-4-tuple retransmits from an aborted upload otherwise land as the next
  connection's head (poisoned `GET /` relays, identical truncated responses
  run after run). Big uploads: curl POSTs to a flaky link land in `just
  radio-ota`'s retry loop by design (writes to the inactive slot are
  idempotent from offset 0).
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
  works even with the Pico unpowered. AP-mode routing: the **Pico app owns
  `/` whenever it is online** (full web UI over the relay in AP mode too);
  provisioning lives at `/setup`, and captive probes redirect there — or
  straight to the app when mode is explicitly `AccessPoint` (the AP exists to
  serve the UI then; fallback-AP pops still land on setup for repair). With the
  board offline, the portal reclaims `/` for recovery. (`portal::route` claims
  only probes/OPTIONS/`/setup`/`/favicon.ico`/the five API paths — anything
  else relays, including WebSockets and `/api/ota`.) In pure `AccessPoint`
  mode the supervisor never joins: `/setup` "Connect" only saves credentials;
  joining resumes when mode returns to Station.
