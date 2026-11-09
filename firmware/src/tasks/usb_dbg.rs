//! Bench debug console: a USB-CDC text port on the Pico's own USB connector,
//! plus a panic handler that reports the panic to the ring before halting.
//! Enabled with the `console` feature (radio builds always; wifi builds opt
//! in — `wifi,console` — since the cyw43 link is the thing being debugged and
//! USB is otherwise unused).
//!
//! On builds with `console` but no TCP/RTT defmt logger, this module also
//! provides the global defmt logger, streamed raw onto a **second** CDC port
//! so `defmt-print serial` sees a clean binary frame stream while the first
//! port keeps human-readable text lines.
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
#[cfg(all(feature = "console", not(feature = "defmt_tcp"), not(feature = "defmt_rtt")))]
use embassy_sync::rwlock::RwLock;
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

    /// Copy out the message with sequence `*seen`, if still buffered.
    ///
    /// If the reader lags behind the ring window (the host attached late and
    /// the producer wrapped), `*seen` is snapped forward to the oldest buffered
    /// message: those lines are lost, but the stream keeps flowing. Without
    /// this, a single overflow wedged the console for the rest of the boot —
    /// every later line was invisible (seen: silent console after "pl:").
    fn get(&self, seen: &mut u32) -> Option<Msg> {
        let oldest = self.produced.wrapping_sub(RING_CAP as u32);
        if seen.wrapping_sub(oldest) >= RING_CAP as u32 {
            if *seen == self.produced {
                return None;
            }
            *seen = oldest;
        }
        let msg = self.msgs[(*seen % RING_CAP as u32) as usize].clone();
        Some(msg)
    }
}

// Sync (blocking_mutex + critical section) so log() is callable from panic
// context and plain sync functions. Never hold across an await.
static RING: BlockingMutex<CriticalSectionRawMutex, RefCell<Ring>> = BlockingMutex::new(RefCell::new(Ring::NEW));

// ---------------------------------------------------------------------------
// defmt global logger over USB-CDC (`console` builds without defmt_tcp /
// defmt_rtt, e.g. `wifi,console`). Encoded frames go to a byte ring drained
// onto a dedicated second CDC port; defmt-print re-syncs on frame headers,
// so overflow may skip lines but never corrupts the stream permanently.
// Mirrors the structure of the defmt_tcp crate.
// ---------------------------------------------------------------------------

#[cfg(all(feature = "console", not(feature = "defmt_tcp"), not(feature = "defmt_rtt")))]
const FRAME_RING_CAP: usize = 4096;

#[cfg(all(feature = "console", not(feature = "defmt_tcp"), not(feature = "defmt_rtt")))]
struct FrameRing {
    buf: [u8; FRAME_RING_CAP],
    head: usize,
    tail: usize,
}

#[cfg(all(feature = "console", not(feature = "defmt_tcp"), not(feature = "defmt_rtt")))]
impl FrameRing {
    const NEW: Self = Self {
        buf: [0; FRAME_RING_CAP],
        head: 0,
        tail: 0,
    };

    fn push(&mut self, bytes: &[u8]) {
        for &b in bytes {
            let next = (self.head + 1) % FRAME_RING_CAP;
            if next == self.tail {
                // Ring full: drop oldest byte. Losing frame middles is fine —
                // defmt-print skips what it can't parse.
                self.tail = (self.tail + 1) % FRAME_RING_CAP;
            }
            self.buf[self.head] = b;
            self.head = next;
        }
    }

    fn pop(&mut self, out: &mut [u8]) -> usize {
        let mut n = 0;
        while n < out.len() && self.tail != self.head {
            out[n] = self.buf[self.tail];
            self.tail = (self.tail + 1) % FRAME_RING_CAP;
            n += 1;
        }
        n
    }
}

#[cfg(all(feature = "console", not(feature = "defmt_tcp"), not(feature = "defmt_rtt")))]
static FRAMES: BlockingMutex<CriticalSectionRawMutex, RefCell<FrameRing>> =
    BlockingMutex::new(RefCell::new(FrameRing::NEW));

#[cfg(all(feature = "console", not(feature = "defmt_tcp"), not(feature = "defmt_rtt")))]
#[defmt::global_logger]
struct UsbLogger;

#[cfg(all(feature = "console", not(feature = "defmt_tcp"), not(feature = "defmt_rtt")))]
unsafe impl defmt::Logger for UsbLogger {
    fn acquire() {
        let mut encoder = match DEFMT_ENCODER.try_write() {
            Ok(encoder) => encoder,
            Err(_) => return,
        };
        encoder.start_frame(|bytes| FRAMES.lock(|f| f.borrow_mut().push(bytes)));
    }

    unsafe fn flush() {}

    unsafe fn release() {
        let mut encoder = match DEFMT_ENCODER.try_write() {
            Ok(encoder) => encoder,
            Err(_) => return,
        };
        encoder.end_frame(|bytes| FRAMES.lock(|f| f.borrow_mut().push(bytes)));
    }

    unsafe fn write(bytes: &[u8]) {
        let mut encoder = match DEFMT_ENCODER.try_write() {
            Ok(encoder) => encoder,
            Err(_) => return,
        };
        encoder.write(bytes, |bytes| FRAMES.lock(|f| f.borrow_mut().push(bytes)));
    }
}

#[cfg(all(feature = "console", not(feature = "defmt_tcp"), not(feature = "defmt_rtt")))]
static DEFMT_ENCODER: RwLock<CriticalSectionRawMutex, defmt::Encoder> = RwLock::new(defmt::Encoder::new());

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

/// Current boot stage. The console task reports it in its heartbeat, so a boot
/// that stalls (or a task that dies) still shows exactly how far the machine
/// got — the console task lives on the main executor; if it stops beating, the
/// whole core is gone, not just one subsystem.
#[cfg(feature = "console")]
static STAGE: core::sync::atomic::AtomicU8 = core::sync::atomic::AtomicU8::new(0);

/// Mark a boot milestone: records the stage number for the heartbeat and logs
/// the named line.
pub fn stage(n: u8, what: &str) {
    #[cfg(feature = "console")]
    {
        STAGE.store(n, core::sync::atomic::Ordering::Relaxed);
        log(format_args!("stage {n}: {what}"));
    }
    #[cfg(not(feature = "console"))]
    {
        let _ = (n, what);
    }
}

#[cfg(feature = "console")]
fn stage_now() -> u8 {
    STAGE.load(core::sync::atomic::Ordering::Relaxed)
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

    // Second CDC port: raw defmt frame stream (see UsbLogger). Only present
    // on builds where this module is the defmt logger, so existing bench
    // tooling sees the same single port on `radio,defmt_tcp,console`.
    #[cfg(all(feature = "console", not(feature = "defmt_tcp"), not(feature = "defmt_rtt")))]
    static DEFMT_CDC_STATE: StaticCell<cdc_acm::State<'static>> = StaticCell::new();
    #[cfg(all(feature = "console", not(feature = "defmt_tcp"), not(feature = "defmt_rtt")))]
    let mut defmt_cdc = cdc_acm::CdcAcmClass::new(&mut builder, DEFMT_CDC_STATE.init(cdc_acm::State::new()), 64);

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

    // Single CDC handle on purpose: `split()` + a concurrent read_packet()
    // deadlocked the class state (the reader holds shared state across its
    // await, which starves usb.run() and hangs the whole executor).
    // Button-free reflash uses the vendor reset interface (ResetIface) or the
    // BOOTSEL button path instead of a serial command.

    let mut seen: u32 = 0;
    let boot_t = Instant::now();
    let mut last_beat = Instant::now();
    let drain = async {
        loop {
            Timer::after(Duration::from_millis(50)).await;
            // No DTR gate on purpose: macOS does not reliably raise DTR on
            // open (the CDC driver here never sends SET_CONTROL_LINE_STATE),
            // which made every boot read as a totally silent console. Writes
            // are bounded below, so a detached host costs one 500 ms timeout
            // per message and nothing else.
            //
            // Liveness beat: uptime + how far boot got. A late attacher sees
            // these immediately, so "board is up but stuck at stage N" and
            // "board reset-looping" are distinguishable from the first read.
            if last_beat.elapsed() >= Duration::from_secs(2) {
                last_beat = Instant::now();
                log(format_args!(
                    "hb: up {}s stage {}",
                    boot_t.elapsed().as_secs(),
                    stage_now()
                ));
            }
            loop {
                let msg = RING.lock(|r| r.borrow_mut().get(&mut seen));
                let Some(m) = msg else { break };
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
                        // No host listening (or a stalled IN slot): retry this
                        // message — it was never loaded, so no duplicates.
                    }
                }
            }
        }
    };

    #[cfg(all(feature = "console", not(feature = "defmt_tcp"), not(feature = "defmt_rtt")))]
    let drain = {
        let defmt_drain = async {
            let mut chunk = [0u8; 64];
            let mut pending = 0usize;
            loop {
                Timer::after(Duration::from_millis(20)).await;
                if !defmt_cdc.dtr() {
                    pending = 0; // host gone: drop the un-sent chunk
                    continue;
                }
                if pending == 0 {
                    pending = FRAMES.lock(|f| f.borrow_mut().pop(&mut chunk));
                    if pending == 0 {
                        continue;
                    }
                }
                match embassy_futures::select::select(
                    defmt_cdc.write_packet(&chunk[..pending]),
                    Timer::after(Duration::from_millis(500)),
                )
                .await
                {
                    embassy_futures::select::Either::First(_) => pending = 0,
                    embassy_futures::select::Either::Second(()) => {
                        if !defmt_cdc.dtr() {
                            pending = 0;
                        }
                        // else: host present but endpoint busy — retry same chunk
                    }
                }
            }
        };
        embassy_futures::join::join(drain, defmt_drain)
    };

    // usb.run() pumps the controller; drain feeds it lines (and, on logger
    // builds, the defmt frame stream on the second CDC port); commands reads
    // host requests from the same port.
    embassy_futures::join::join(usb.run(), drain).await;
}

/// Panic handler for `console` builds: report to the ring, then reset. The
/// board re-enters the boot sequence, and a host attached to the console
/// sees the panic line followed by a full breadcrumb replay of the new boot
/// — field-deployable panic forensics without needing a live executor to
/// drain the ring (the executor is dying when this runs).
#[cfg(feature = "console")]
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    log(format_args!("*** PANIC: {}", info));
    cortex_m::peripheral::SCB::sys_reset()
}
