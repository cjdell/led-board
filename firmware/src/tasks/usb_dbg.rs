//! Bench-only debug console: logs to a USB-CDC serial port on the Pico's own
//! USB connector, plus a panic handler that reports the panic to the ring
//! before halting. Enabled with the `console` feature (radio builds only —
//! nothing else leaves USB free).
//!
//! Intentionally allocation-free: a fixed static ring of short lines is
//! drained to the host when a terminal attaches (opening the port does NOT
//! reset the RP2350, so pre-attach logs are replayed from the ring).

use core::cell::RefCell;
use core::fmt::{self, Write as _};

use embassy_executor::Spawner;
use embassy_rp::peripherals::USB;
use embassy_rp::usb::Driver;
use embassy_sync::blocking_mutex::{Mutex as BlockingMutex, raw::CriticalSectionRawMutex};
use embassy_time::{Duration, Instant, Timer};
use embassy_usb::control::{OutResponse, Recipient, Request, RequestType};
use embassy_usb::types::InterfaceNumber;
use embassy_usb::{Builder, Handler, class::cdc_acm};
use static_cell::StaticCell;

const RING_CAP: usize = 64;
const LINE_CAP: usize = 128;

pub struct Msg {
    len: u8,
    buf: [u8; LINE_CAP],
}

impl Msg {
    const EMPTY: Self = Self {
        len: 0,
        buf: [0; LINE_CAP],
    };
}

impl Clone for Msg {
    fn clone(&self) -> Self {
        Self {
            len: self.len,
            buf: self.buf,
        }
    }
}

impl fmt::Write for Msg {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for b in s.bytes() {
            if (self.len as usize) < LINE_CAP {
                self.buf[self.len as usize] = b;
                self.len += 1;
            } else {
                return Err(fmt::Error);
            }
        }
        Ok(())
    }
}

struct Ring {
    msgs: [Msg; RING_CAP],
    /// Monotonic count of produced messages.
    produced: u32,
}

impl Ring {
    const NEW: Self = Self {
        msgs: [Msg::EMPTY; RING_CAP],
        produced: 0,
    };

    fn push(&mut self, m: Msg) {
        let i = (self.produced % RING_CAP as u32) as usize;
        self.msgs[i] = m;
        self.produced = self.produced.wrapping_add(1);
    }

    /// Copy out the message with sequence `seen`, if still buffered.
    fn get(&self, seen: u32) -> Option<Msg> {
        let oldest = self.produced.wrapping_sub(RING_CAP as u32);
        if self.produced == seen || seen.wrapping_sub(oldest) >= RING_CAP as u32 {
            return None;
        }
        Some(self.msgs[(seen % RING_CAP as u32) as usize].clone())
    }
}

// Sync (blocking_mutex + critical section) so log() is callable from panic
// context and plain sync functions. Never hold across an await.
static RING: BlockingMutex<CriticalSectionRawMutex, RefCell<Ring>> = BlockingMutex::new(RefCell::new(Ring::NEW));

/// Pico SDK stdio-USB "reset" interface (class 0xFF, subclass 0x00, protocol
/// 0x01). picotool (>= 2.0) scans every device's config descriptor for this
/// interface regardless of VID/PID, then sends a class OUT request `0x01` to
/// interface (`picotool reboot -u -f --vid 5824 --pid 1171`; the explicit
/// vid/pid is required because picotool's default scan filters on VID 2E8A,
/// and that identity is itself unusable on this Mac — see console_task).
/// Handling it jumps to the BOOTSEL ROM, so bench reflashes never need the
/// button. Accepted masks (activity LED, interface disable) are ignored: we
/// have no activity LED.
#[cfg(feature = "console")]
struct ResetIface {
    itf: InterfaceNumber,
}

#[cfg(feature = "console")]
impl Handler for ResetIface {
    fn control_out(&mut self, req: Request, _data: &[u8]) -> Option<OutResponse> {
        // picotool sends request_type=Class, recipient=Interface; also accept
        // Vendor for other tooling that follows the older convention.
        if (req.request_type == RequestType::Class || req.request_type == RequestType::Vendor)
            && req.recipient == Recipient::Interface
            && req.index == self.itf.0 as u16
        {
            match req.request {
                0x01 => {
                    log(format_args!("usb_dbg: picotool reboot -> bootsel"));
                    embassy_rp::rom_data::reset_to_usb_boot(0, 0);
                }
                0x02 => {
                    log(format_args!("usb_dbg: picotool reboot -> flash"));
                    cortex_m::peripheral::SCB::sys_reset();
                }
                _ => {}
            }
        }
        None
    }
}

/// Log one line to the console ring. Compiles away unless `console` is on,
/// so call sites need no cfg gates.
pub fn log(args: fmt::Arguments) {
    #[cfg(feature = "console")]
    {
        let _ = Instant::now(); // keep timer referenced consistently across cfgs
        let mut m = Msg {
            len: 0,
            buf: [0; LINE_CAP],
        };
        let _ = write!(&mut m, "{}\n", args);
        RING.lock(|r| r.borrow_mut().push(m));
    }
    #[cfg(not(feature = "console"))]
    {
        let _ = args;
    }
}

/// Start the console task. Must be called exactly once, with the USB
/// peripheral driver (radio builds leave USB otherwise unused).
pub fn start(spawner: Spawner, driver: Driver<'static, USB>) {
    #[cfg(feature = "console")]
    spawner.spawn(console_task(driver).unwrap());
    #[cfg(not(feature = "console"))]
    {
        let _ = (spawner, driver);
    }
}

#[cfg(feature = "console")]
#[embassy_executor::task]
async fn console_task(driver: Driver<'static, USB>) {
    log(format_args!("usb_dbg: console up"));

    // Third-party VID: 2E8A app-mode devices get claimed by a DriverKit
    // personality on this Mac (probe-rs install) and never bind — macOS
    // leaves the whole device !registered !matched, invisible to libusb.
    // 16C0:0493 enumerates cleanly; picotool reaches the reset interface via
    // explicit `--vid 5824 --pid 1171` (default scan hard-filters 2E8A).
    let mut config = embassy_usb::Config::new(0x16C0, 0x0493);
    config.manufacturer = Some("led-board");
    config.product = Some("ws-dbg");
    config.serial_number = Some("bench");
    config.max_power = 100;
    config.max_packet_size_0 = 64;

    static CONFIG_DESC: StaticCell<[u8; 256]> = StaticCell::new();
    static BOS_DESC: StaticCell<[u8; 256]> = StaticCell::new();
    static CONTROL_BUF: StaticCell<[u8; 128]> = StaticCell::new();

    let mut builder = Builder::new(
        driver,
        config,
        &mut CONFIG_DESC.init([0; 256])[..],
        &mut BOS_DESC.init([0; 256])[..],
        &mut [],
        &mut CONTROL_BUF.init([0; 128])[..],
    );

    static CDC_STATE: StaticCell<cdc_acm::State<'static>> = StaticCell::new();
    let mut cdc = cdc_acm::CdcAcmClass::new(&mut builder, CDC_STATE.init(cdc_acm::State::new()), 64);

    // Button-free reboots: zero-endpoint vendor interface + control handler
    // (see ResetIface). Must be registered before build().
    let mut func = builder.function(0xff, 0x00, 0x01);
    let mut iface = func.interface();
    let reset_itf = iface.interface_number();
    let _alt = iface.alt_setting(0xff, 0x00, 0x01, None);
    drop(_alt);
    drop(iface);
    drop(func);
    static RESET_STATE: StaticCell<ResetIface> = StaticCell::new();
    builder.handler(RESET_STATE.init(ResetIface { itf: reset_itf }));

    let mut usb = builder.build();

    let mut seen: u32 = 0;
    let drain = async {
        loop {
            Timer::after(Duration::from_millis(50)).await;
            // DTR reflects the host terminal open/close (SET_CONTROL_LINE_STATE).
            // `wait_connection` keys on endpoint ENABLE, which survives a plain
            // port close — it cannot resync us after the host disappears.
            if !cdc.dtr() {
                continue;
            }
            loop {
                let msg = RING.lock(|r| r.borrow().get(seen));
                let Some(m) = msg else { break };
                if !cdc.dtr() {
                    break; // host vanished mid-drain
                }
                // Bounded write: if the IN buffer is stuck full (host left
                // without consuming), the write never completes — drop it via
                // the timeout (the endpoint write future is cancel-safe: the
                // RP2351 buffer is only touched once the slot frees). Leave
                // `seen` untouched so the message retries once a host returns.
                match embassy_futures::select::select(
                    cdc.write_packet(&m.buf[..m.len as usize]),
                    Timer::after(Duration::from_millis(500)),
                )
                .await
                {
                    embassy_futures::select::Either::First(Ok(())) => {
                        seen = seen.wrapping_add(1);
                    }
                    embassy_futures::select::Either::First(_) => break,
                    embassy_futures::select::Either::Second(()) => {
                        if !cdc.dtr() {
                            break;
                        }
                        // Host present but not draining (rare): retry this
                        // message — it was never loaded, so no duplicates.
                    }
                }
            }
        }
    };

    // usb.run() pumps the controller; drain feeds it lines.
    embassy_futures::join::join(usb.run(), drain).await;
}

/// Panic handler for `console` builds: report, then halt (no reset — the
/// console keeps the evidence; unplug/replug to retry).
#[cfg(feature = "console")]
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    log(format_args!("*** PANIC: {}", info));
    loop {
        cortex_m::asm::wfi();
    }
}
