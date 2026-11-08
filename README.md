# Raspberry Pi Pico 2 W

## Wiring:

                      |    |
               =======|    |=======
                | GP0         VBUS |   ──► radio VIN (5 V)
                | GP1         VSYS |
                | GND         GND  |   ◄── radio GND
      STRIP 0   | GP2           EN |
      STRIP 1   | GP3         3_3V |
      STRIP 2   | GP4         VREF |
      STRIP 3   | GP5         GP28 |
                | GND         GND  |
      STRIP 4   | GP6         GP27 |
      STRIP 5   | GP7         GP26 |
      STRIP 6   | GP8          RUN |
      STRIP 7   | GP9         GP22 |
                | GND         GND  |
                | GP10        GP21 |
                | GP11        GP20 |
        radio RX| GP12        GP19 |
        radio TX| GP13        GP18 |
                | GND         GND  |
                | GP14        GP17 |
                | GP15        GP16 |
               ====================

### ESP32 slave radio (optional, `radio` build)

The cyw43 WiFi on the Pico 2 W is flaky in some environments, so the firmware
has a `radio` build where an ESP32-C3 **or ESP32-C6** acts as the board's
radio. Four wires between the Pico and the radio MCU:

| Pico 2 W (RP2350) | Direction | ESP32-C3 / ESP32-C6 |
|-------------------|-----------|---------------------|
| GP12 (UART0 TX)   | →         | GPIO20 (UART1 RX)   |
| GP13 (UART0 RX)   | ←         | GPIO21 (UART1 TX)   |
| GND               | —         | GND                 |
| VBUS (5 V)        | →         | VIN / 5V            |

921600 baud, 8N1. GPIO20/21 are UART1's default pads on the C3 and valid
UART1 matrix pins on the C6, so the firmware (`slave_radio`, one codebase,
`esp32c3`/`esp32c6` features) wires both chips identically.

The C6 drains this UART with its UHCI DMA engine, and stream data is windowed
(TCP-style cumulative acks, see `radio_link`), so the relay is lossless:
buffer pressure pauses the producer instead of dropping bytes.

> **Bench note:** while each board is plugged into a computer, **omit the VIN
> wire** and power both from their own USB — back-feeding 5 V into an already
> USB-powered radio board can trip the port's protection (this killed the C3's
> USB enumeration whenever the link wiring was attached; revisit on a known-good
> board). VIN belongs to the final build only, where the radio draws its 5 V
> from the Pico's VBUS rail.

The C3 draws < 350 mA peak from that rail; if you inject 5 V into the strip's
rail, power the radio from the same supply instead of VBUS. Any ESP32-C3 with
≥ 4 MB flash works (GPIO8 drives the SuperMini status LED, active-low). On the
C6-Zero bench: join + DHCP verified end-to-end (Leighhack, WPA2); double-check
that GPIO20/21 are broken out on your header before wiring the link — the C6's
GPIO matrix can move UART1 to any free pins (`with_tx`/`with_rx` in
`slave_radio/src/bin/slave_radio.rs`) if they aren't.

## ESP32 slave radio architecture

```
WiFi LAN ══════► C3 STA :80 ─┐                      ┌──► picoserve web app
                  (fallback:  ├─ radio_link (UART) ──┤   (unchanged: /api/ws,
AP + portal :80) C3 AP :80 ──┘   framed + CRC16      │   /api/config, /api/wifi,
                                                      │   /api/ota, …)
Pico runs NO IP stack. The radio (C3 or C6) owns WiFi entirely:
 • joins the strongest saved network; on failure it falls back to an open
   AP + DHCP/captive-DNS portal (http://192.168.2.1/) with a WiFi-connect UI
 • mDNS: led-board.local on whichever interface is up
 • persists saved networks in flash (cfg partition), kept in sync with the
   Pico's config.json (Pico config wins at boot; Pico pushes on every save)
 • the built-in portal answers /api/wifi (scan/join) locally while the Pico's
   full app is still reachable through the fallback AP via the relay
```

Everything the Pico needs from the radio (scan, join, status, LED) travels as
JSON RPC over the same link (`radio_link` crate); browser connections are
relayed as multiplexed byte streams — see `radio_link/src/lib.rs` for the
framing spec.

### Building & flashing the radio firmware

```sh
cd slave_radio
cargo build --release            # C3 (default feature), or: just radio-build
just radio-flash                 # espflash flash --partition-table partitions.csv --monitor

# ESP32-C6 (rv32imac target + esp32c6 chip feature; same 4MB partition table):
just radio-c6-flash              # = cargo build --release --no-default-features \
                                 #   --features esp32c6 --target riscv32imac-unknown-none-elf \
                                 #   && espflash flash --partition-table partitions.csv …
```

### Running the board with the slave radio

```sh
just main        # choose network transport "radio" (radio,defmt_rtt build)
# or explicitly:
cargo objcopy --release -p firmware --features radio,defmt_rtt --bin ws \
    --target thumbv8m.main-none-eabihf -- -O binary tmp/ws.bin
```

Once the radio answers HELLO, the Pico pushes its saved WiFi networks (or adopts
the radio's when its own config is empty). Connect to `http://led-board.local/`
as before — the web app, WebSocket control, and OTA all work unchanged.
