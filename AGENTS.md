# AGENTS.md — led-board

Rust firmware for a Raspberry Pi Pico 2 W LED art board (embassy, `no_std`).
Networking: cyw43 Wi-Fi (`wifi` — the primary path), `usb_ethernet` or `ppp`.
The ESP32-C3/C6 **slave-radio mode is retired (Nov 2026)** — see "Legacy" at
the end; never build against it for a real board. Read this before building or
flashing anything; most failure modes here are tooling traps, not code bugs.

## Repository layout

| Path | What it is |
|---|---|
| `led_core/` | **All system logic** (`no_std`): picoserve web app, WebSocket pump, display worker, heartbeat, config, screen model. Platform boundary = traits (`Board`, `OtaBackend`, `LocalFsTrait`, `Ws2812ParallelDriver`). Compiles for both thumbv8m and the host (`--features std`). |
| `animations/` | **The one animation implementation**: every `AnimationEnum` renderer (festival scenes are pure `fn(ms, buffer)` functions). Shared by firmware, sim, and the browser preview (via `anim_wasm`). |
| `anim_wasm/` | The `animations` crate built for `wasm32-unknown-unknown` behind a tiny C ABI — the web UI's preview engine. Embedded into the SPA as base64 by `web/embed_wasm.ts`. See "Animation previews". |
| `firmware/` | Pico 2 W (RP2351) hardware impl: bin target `ws`, PIO WS2812 driver, flash stores, `Board` impls (cyw43), embassy-net transports (incl. the retired `radio` tunnel). Feature matrix below. |
| `sim/` | macOS full-system simulator (`just sim` — dot-matrix window + real web UI on :8080) + old `animation_dev` scratchpad. |
| `radio_link/` | **RETIRED Nov 2026** (ESP32 slave radio): shared `no_std` framed-protocol crate. Kept as history. |
| `slave_radio/` | **RETIRED Nov 2026**: ESP32-C3 firmware (esp-hal, RISC-V). **Detached workspace** — excluded from the root workspace and has its own `Cargo.lock` + `.cargo/config.toml` (a trap for repo-root cargo commands, see below). |
| `embedded_tools/` | `LocalFs`/`ConfigFile` + the **log facade** (`use embedded_tools::{info,warn,error,debug,Debug2Format}`; defmt on embedded, println on host — feature `defmt` xor `std`). |
| `ws_client/` | Host-side client. |
| `web/` | SPA (Solid + TypeScript, Deno/vite, no Bootstrap — custom theme in `src/theme.scss`). `web/compile.sh` = wasm preview build + vite build + gzip → `web/bundle/index.html.gz` (gitignored, include_bytes'd into firmware + sim). `deno task dev` = :3000 dummy API; `deno task check` / `fmt`; `deno task wasm` (or `just wasm`) regenerates the gitignored, generated `src/lib/wasm_gen.ts` — needed before dev/check/build. |
| `docs/slave-radio.md` | **RETIRED**: complete ESP32 slave-radio forensics — wiring, architecture, flashing/OTA, monitoring quirks, protocol. History only. |
| `README.md` | Authoritative pinout table + architecture diagrams (Mermaid). |

## Toolchain traps (read first)

- **Always** `export PATH="$HOME/.cargo/bin:$PATH"` before cargo commands.
  The interactive shell otherwise picks up Homebrew `/opt/homebrew/bin/rustc`,
  which has no embedded targets.
- **Always** pass `--target thumbv8m.main-none-eabihf` for `firmware` builds —
  `[build] target` in `.cargo/config.toml` is commented out. Without it, cargo
  builds for the host and you get confusing `cortex-m` E0425 / "invalid
  register r0" errors.
- Run host tests from the **repo root** (e.g. `cargo test -p sim`). From
  inside the retired `slave_radio/` its `.cargo/config.toml` forces the RISC-V
  target and everything fails to link.
- `rustfmt` runs on save and reorders imports — re-read a file immediately
  before `edit_file` matches or fuzzy-match edits will miss.
- Rust size/objcopy tooling: Apple's `size` can't read ELF. Use
  `$(rustc --print sysroot)/lib/rustlib/<host>/bin/llvm-size` and
  `llvm-objcopy` (or `cargo objcopy` / `cargo-binutils`).
- **Web dev/check/build needs the generated `web/src/lib/wasm_gen.ts`**
  (gitignored): run `just wasm` once per animations change first — or just
  `./web/compile.sh`, which chains wasm → vite → gzip. It builds
  `-p anim-wasm --target wasm32-unknown-unknown`, so the rustup PATH rule
  applies there too (the Homebrew toolchain has no wasm target;
  `rust-toolchain.toml` now lists `wasm32-unknown-unknown`).

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
  raw pointer (mpsc `usize`) — same pattern as the retired radio tunnel's
  TUNNEL_APP; the router is single-owner afterwards, so it needs no `Sync`.
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

## Animation previews: the firmware's code in the browser (Nov 2026)

The web app has **no hand-written animation painters anymore**. Previews
run the real `animations` crate compiled for `wasm32-unknown-unknown`
(`anim_wasm/`, cdylib + rlib), base64-inlined into the single-file SPA at
build time (`web/embed_wasm.ts` → generated, gitignored
`web/src/lib/preview.ts`'s sibling `wasm_gen.ts`).
`web/src/lib/preview.ts` is only a loader now: shared rAF loop with a
per-tile fps budget → `anim_render(JSON.stringify(animation), ms)` → blit
the returned RGB888 (same 7200 B row-major layout as `GET /api/frame`).

- **One wire format**: `JSON.stringify` of the UI's `Animation` value is
  exactly serde's JSON of `AnimationEnum`, which the wasm parses with the
  same `Deserialize` derive as the playlist files. If the board can play
  it, the preview can play it. Text cards get the real 6×12/10×20
  embedded-graphics fonts, painted into the framebuffer — no canvas
  font overlay.
- **Why base64 in the bundle**: picoserve serves exactly one gzipped HTML
  file — there is no route to `fetch()` a `.wasm` — and the sim embeds the
  same bytes. Artifact: ~191 KB after `wasm-opt -Oz -all` (~230 KB before;
  `[profile.release-wasm]` = opt-level z, fat LTO, panic=abort, strip),
  ≈ +104 KB gzipped in the bundle. A separate `/anim.wasm` route would
  save ~19 KB on the wire but needs firmware + sim routes and a MIME type
  — not worth it at this size.
- **Hand-rolled C ABI, not wasm-bindgen** (`anim_alloc`/`anim_free`/
  `anim_render`/`anim_frame_ptr`/`anim_error_*`) — bindgen glue fights the
  inline-everything single-file build; keep it out. `anim_render` caches
  the parsed animation keyed on its JSON string, so the ~8 fps library
  grid doesn't re-parse every frame.
- **Previews show pre-gamma sRGB at full brightness** — the same reasoning
  as `/api/frame` and the sim default: the panel's γ stage exists to
  reproduce exactly these colours on linear strips, and the canvas already
  applies sRGB.
- **A panic is a trap**, same contract as the firmware (where it would be
  a watchdog reset). The loader re-instantiates once or twice, then
  disables previews to a `Blank`-navy frame with a toast — the SPA itself
  never dies on preview failure.
- **`cargo test -p anim-wasm`** (host, via the rlib) renders every enum
  variant at several timestamps — the parity net that makes "one
  implementation" safe. It immediately found and fixed a real firmware
  bug: `Fireworks` computed `ms_since_start % 200 - 100` in `u32`, which
  wrapped, and half the bursts flew off-panel. Keep its `ALL_JSON` list in
  sync when `AnimationEnum` grows.
- `just ci` runs `cargo test -p anim-wasm` and `just wasm` before the deno
  checks; `web/compile.sh` chains wasm → vite → gzip.

## Building

Feature matrix for `firmware`: pick one transport + one network.

- Transports: `defmt_rtt` (needs an external debug probe), `defmt_tcp`
- Networks: `wifi` (cyw43 — the primary venue path), `usb_ethernet`, `ppp`.
  `radio` (ESP32 slave) is **retired** — it still compiles, never deploy it.

```sh
export PATH="$HOME/.cargo/bin:$PATH"
cargo check -p firmware --features wifi,defmt_tcp --target thumbv8m.main-none-eabihf
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
cargo objcopy --release -p firmware --features wifi,defmt_tcp --bin ws \
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

Traps:

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

## Legacy: ESP32 slave radio (retired Nov 2026)

The `radio` network mode (Pico ↔ ESP32-C3/C6 over UART, radio terminates TCP)
is **retired and will probably never be used again** — cyw43 Wi-Fi passed
acceptance at 7–10× the relay's throughput. `radio_link/` + `slave_radio/`
stay in-tree as history; `firmware`'s `radio` feature still compiles but is
never deployed, and no dependency work happens for it (esp-hal stays pinned at
its old tag). **All operational forensics** — build/flashing/OTA recipes, A/B
partition traps, monitoring quirks, link protocol, heap bounds, portal routing,
acceptance harness — are consolidated in
[`docs/slave-radio.md`](docs/slave-radio.md).

## Hardware pins (see README.md pinout table for truth)

- GP2–GP9: WS2812 STRIP 0–7 (PIO1). GP23/24/25/29: cyw43 (wifi builds only).
  GP0/GP1 are deliberately left free for a debug console.
- GP12/GP13 (UART0) carried the retired radio link at 460800 baud
  ([`docs/slave-radio.md`](docs/slave-radio.md)) — free now, keep the pairing
  clear for future UART use.
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
- **Bundle size note (Nov 2026, wasm previews)**: `web/compile.sh` now
  produces a **149,952 B** gzipped single-file bundle — up from 45,790 B
  because the base64-inlined `anim_wasm` preview engine costs ~104 KB gz
  (see "Animation previews"). Still fine for the 1020 K slot and the
  measured download rate (~0.4 s per load at venue throughput), but
  **re-run acceptance at the new size** before quoting load times again.
  Re-run `web/compile.sh` before flashing UI changes; the sim embeds the
  same gz.
- **Wi-Fi acceptance PASSED (Nov 2026)**: `HOST=http://<ip>/ ./scripts/stress_wifi.sh`
  → 15/15 identical 73,541 B loads (pre-truncate-fix number; loads are now
  149,952 B with the wasm preview engine — re-run acceptance at the new size), one sha256 group, 0 curl failures, 0 guard
  trips. Singles ≈0.16 s (≈3.3–3.7 Mbit/s), 4-bursts ≈0.3–0.5 s (≈1.2–2 Mbit/s)
  — 7–10× the (now-retired) radio relay (≈46 KB/s ≈ 0.37 Mbit/s at 460800
  UART), which is what retired the slave radio.
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
console 667,480 B**. After the panel frame mirror (`/api/frame`, +7.2 KB
`.bss`), dropping `MadameRatcliffe` and the supervised mDNS responder:
**wifi 579,244 B, bench console 669,264 B** (retired `radio` build: 536,232 B
— the linker drops the responder there, it never spawns). After the wasm
preview engine replaced the TypeScript painters (149,952 B gz bundle,
~104 KB of it base64 wasm — see "Animation previews"): **wifi 683,564 B
(~361 K slot headroom), bench console 773,584 B**. The old wifi tightness is
gone; the next ceiling is the 1020 K slot, not the SPA. Console builds now fit
comfortably again. No size pressure anywhere else.

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
- **Panel frame mirror (Nov 2026)**: `GET /api/frame` serves the display
  worker's live buffer as raw row-major RGB888 (60×40×3 = **7200 B**,
  `led_core::types::PANEL_FRAME`, an `embassy_sync::mutex::Mutex` static).
  The worker publishes every 250 ms with non-blocking `try_lock` (a poller
  streaming a response just makes one publish skip). Snapshot is taken
  **before** `apply_power_limit` — the authored sRGB — because the panel's
  γ-stage exists to reproduce exactly those colors on linear strips, so a
  browser can paint it 1:1. The UI's `/now` + playlist heroes
  (`<LedMatrix live>`) poll it at 2 fps (`web/src/lib/liveframe.ts`) and
  fall back to the local wasm preview (`web/src/lib/preview.ts` — same
  renderers, so the fallback is frame-accurate, only the elapsed-time base
  differs) when unavailable (dummy dev mode, pre-mirror firmware, board
  offline). Library-card thumbnails are pure wasm renders of the playlist
  item. Streaming goes through `Content` from the mutex guard — never copy
  `PanelFrame` onto a task stack (7.2 KB).
- **mDNS responder (Nov 2026, hardened)**: `mdns_task` (tasks/common.rs) is a
  supervised **session loop** over `edge_mdns`, not a one-shot spawn: it
  waits for an IPv4 from `EthernetSignal`, binds fresh (fresh IGMP join —
  memberships die with the link, which made the old responder deaf after a
  rejoin), serves the *current* address, and rebinds on every connection
  event, on silent DHCP drift (the announce ticker compares
  `stack.config_v4()` to the served IP every 15 s), and on any UDP error
  (1 s backoff — no `.expect()` anywhere, the responder cannot die). It
  **re-announces every 15 s** (¼ of the 60 s TTL) so caches self-heal without
  a query round — verified from the bench: `dns-sd -Q led-board.local A`
  answers in ~1 ms even >TTL after the last query. **The notorious
  "~5 s to resolve led-board.local" is NOT the board**: this Mac's VPN utun
  stack flags mDNSResponder's `local` resolver `Not Reachable`
  (`scutil --dns`), so `dscacheutil`/getaddrinfo spend a fixed 5 s
  failover regardless of cache state (10 s for NXDOMAIN). Script against the
  IP (resolve once via `dns-sd`), or fix the VPN's split-DNS.
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

- **Embassy-net socket budgets**: `StackResources<N>` is the smoltcp **max
  socket count per stack** — exceeding it panics (`SocketSet full`), so size
  new socket-using services before adding them. Wifi build (`tasks/wifi.rs`):
  **16** = 5 web listeners + mdns + dhcp + defmt-TCP + a browser's parallel
  connections (10 was too tight: the 4-way burst test hit refused
  connections).
- Acceptance harness (wifi): `HOST=http://<ip>/ ./scripts/stress_wifi.sh` —
  3 single + 3×4-burst page loads, one sha256 group, 0 curl failures, 0 guard
  trips in the defmt log (radio-era `scripts/stress.sh` is history, see
  [`docs/slave-radio.md`](docs/slave-radio.md)).
