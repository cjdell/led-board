# Raspberry Pi Pico 2 W

## Wiring:

                      |    |
               =======|    |=======
                | GP0         VBUS |   ──► C3 VIN (5 V)
                | GP1         VSYS |
                | GND         GND  |   ◄── C3 GND
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
        C3 RX   | GP12        GP19 |
        C3 TX   | GP13        GP18 |
                | GND         GND  |
                | GP14        GP17 |
                | GP15        GP16 |
               ====================

### ESP32-C3 slave radio (optional, `radio` build)

The cyw43 WiFi on the Pico 2 W is flaky in some environments, so the firmware
has a `radio` build where an ESP32-C3 acts as the board's radio. Four wires
between the Pico and the C3:

| Pico 2 W (RP2350) | Direction | ESP32-C3        |
|-------------------|-----------|-----------------|
| GP12 (UART0 TX)   | →         | GPIO20 (UART1 RX) |
| GP13 (UART0 RX)   | ←         | GPIO21 (UART1 TX) |
| GND               | —         | GND             |
| VBUS (5 V)        | →         | VIN / 5V        |

921600 baud, 8N1. The C3 draws < 350 mA peak from the Pico's VBUS rail; if you
inject 5 V into the strip's rail, power the C3 from the same supply instead of
VBUS. Any ESP32-C3 module with ≥ 4 MB flash works (GPIO8 drives its status
LED, active-low on SuperMini-class boards).

## ESP32-C3 slave radio architecture

```
WiFi LAN ══════► C3 STA :80 ─┐                      ┌──► picoserve web app
                  (fallback:  ├─ radio_link (UART) ──┤   (unchanged: /api/ws,
AP + portal :80) C3 AP :80 ──┘   framed + CRC16      │   /api/config, /api/wifi,
                                                      │   /api/ota, …)
Pico runs NO IP stack. The C3 owns WiFi entirely:
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

### Building & flashing the C3

```sh
cd slave_radio
cargo build --release            # or: just radio-build
just radio-flash                 # espflash flash --partition-table partitions.csv --monitor
```

### Running the board with the C3 radio

```sh
just main        # choose network transport "radio" (radio,defmt_rtt build)
# or explicitly:
cargo objcopy --release -p firmware --features radio,defmt_rtt --bin ws \
    --target thumbv8m.main-none-eabihf -- -O binary tmp/ws.bin
```

Once the C3 answers HELLO, the Pico pushes its saved WiFi networks (or adopts
the C3's when its own config is empty). Connect to `http://led-board.local/`
as before — the web app, WebSocket control, and OTA all work unchanged.
