use alloc::vec::Vec;
use embassy_rp::{
    Peri,
    clocks::clk_sys_freq,
    dma::{Channel, ChannelInstance},
    gpio::Level,
    pio::{
        self, Common, Direction, FifoJoin, Instance, LoadedProgram, PinConfig, PioPin, ShiftConfig, ShiftDirection,
        StateMachine,
        program::{Assembler, OutDestination},
    },
};
use fixed::types::U24F8;
use smart_leds::RGB8;

const T1: u8 = 2;
const T2: u8 = 5;
const T3: u8 = 3;
const CYCLES_PER_BIT: u32 = (T1 + T2 + T3) as u32;

/// Holds the ws2812 parallel program loaded into PIO instruction memory.
pub struct PioWs2812ParallelProgram<'a, PIO: Instance> {
    prg: LoadedProgram<'a, PIO>,
}

impl<'a, PIO: Instance> PioWs2812ParallelProgram<'a, PIO> {
    /// Load the parallel ws2812 program.
    ///
    /// Protocol per bit:
    ///   T1 cycles: all 8 pins HIGH         (start pulse)
    ///   T2 cycles: pins HIGH if bit=1,
    ///              pins LOW  if bit=0       (data window)
    ///   T3 cycles: all 8 pins LOW           (stop/reset)
    ///
    /// One OSR word feeds four OUTs (auto_fill, threshold 32): the loop is
    /// exactly T1 + T2 + T3 = 10 SM cycles per bit period, so the SM clock
    /// divisor below lands on the exact 800 kHz WS2812 rate. (The previous
    /// program paid for 4 explicit PULLs per wrap — 11 cycles per bit,
    /// 9 % of the wire budget lost.)
    ///
    /// Timing check at 8 MHz SM cycles: T0H = 250 ns, T1H = 875 ns,
    /// T0L = 1000 ns, T1L = 375 ns — all inside WS2812B tolerance.
    pub fn new(common: &mut Common<'a, PIO>) -> Self {
        let mut a: Assembler<32> = Assembler::new();

        let mut wrap_target = a.label();
        let mut wrap_source = a.label();

        a.bind(&mut wrap_target);

        // ── T1: drive all 8 lines HIGH (start of bit pulse) ──────────────
        // SET pins, 0xFF only does 5 bits — MOV pins, !NULL gets all eight.
        a.mov_with_delay(
            pio::program::MovDestination::PINS,
            pio::program::MovOperation::Invert,
            pio::program::MovSource::NULL,
            T1 - 1,
        );

        // ── T2: output the data column; pins stay HIGH for 1-bits ────────
        a.out_with_delay(OutDestination::PINS, 8, T2 - 1);

        // ── T3: drive all 8 lines LOW (end of bit pulse) ─────────────────
        a.mov_with_delay(
            pio::program::MovDestination::PINS,
            pio::program::MovOperation::None,
            pio::program::MovSource::NULL,
            T3 - 1,
        );

        a.bind(&mut wrap_source);

        let prg = a.assemble_with_wrap(wrap_source, wrap_target);
        let prg = common.load_program(&prg);
        Self { prg }
    }
}

// The strip count and the driver contract are defined by the shared screen
// model (led-core); this file is the PIO implementation of that contract.
use led_core::led_screen::{N_STRIPS, Ws2812ParallelDriver};

const BITS: usize = 24;

/// WS2812 latch: the reset-low gap between frames that makes a strip show
/// the data just shifted in (tRESET ≥ 50 µs). Pipelining removed the CPU's
/// incidental inter-frame pause, so the driver owns this delay explicitly.
const LATCH_US: u64 = 60;

/// Busy-poll interval while waiting for the in-flight frame to finish.
/// Only the overshoot past true completion costs frame time (~1 %).
const DMA_POLL_US: u64 = 100;

/// Parallel WS2812 driver for 8 simultaneous strips, fully pipelined:
/// while frame N is on the wire, `write` for frame N+1 transposes into
/// the other DMA buffer and returns once N's transfer is done — the
/// display worker's render time hides inside the 9 ms wire time.
///
/// Frame cycle = max(wire + latch, CPU). Wire = N_LEDS × 24 bit periods
/// × 1.25 µs = 9.00 ms at N_LEDS = 300, so the panel ceiling is ~110 fps;
/// if render+transpose ever exceeds the wire time the next buffer is
/// still being filled as its transfer starts (a torn frame, not a hang).
///
/// - `S`        – PIO state machine index
/// - `N_LEDS`   – number of LEDs **per strip**
pub struct PioWs2812ParallelDriver<'d, P: Instance, const S: usize, const N_LEDS: usize> {
    dma: Channel<'d>,
    sm: StateMachine<'d, P, S>,
    /// Double-buffered DMA payloads (packed bit-column words).
    buf: [Vec<u32>; 2],
    /// Index of the buffer *not* currently on the wire — the transpose
    /// target and the next transfer to start.
    next: usize,
    /// False until the first `dma_push`: no previous transfer to wait on.
    started: bool,
}

impl<'d, P: Instance, const S: usize, const N_LEDS: usize> PioWs2812ParallelDriver<'d, P, S, N_LEDS> {
    /// Configure the state machine for parallel 8-strip output.
    ///
    /// `pin_0` must be the lowest-numbered of 8 **consecutive** GPIO pins.
    pub fn new<D: ChannelInstance>(
        pio: &mut Common<'d, P>,
        mut sm: StateMachine<'d, P, S>,
        dma: Peri<'d, D>,
        irq: impl embassy_rp::interrupt::typelevel::Binding<D::Interrupt, embassy_rp::dma::InterruptHandler<D>> + 'd,
        pin_0: Peri<'d, impl PioPin>,
        pin_1: Peri<'d, impl PioPin>,
        pin_2: Peri<'d, impl PioPin>,
        pin_3: Peri<'d, impl PioPin>,
        pin_4: Peri<'d, impl PioPin>,
        pin_5: Peri<'d, impl PioPin>,
        pin_6: Peri<'d, impl PioPin>,
        pin_7: Peri<'d, impl PioPin>,
        program: &PioWs2812ParallelProgram<'d, P>,
    ) -> Self {
        let mut cfg = pio::Config::default();

        // Build all 8 PIO pins.
        let p0 = pio.make_pio_pin(pin_0);
        let p1 = pio.make_pio_pin(pin_1);
        let p2 = pio.make_pio_pin(pin_2);
        let p3 = pio.make_pio_pin(pin_3);
        let p4 = pio.make_pio_pin(pin_4);
        let p5 = pio.make_pio_pin(pin_5);
        let p6 = pio.make_pio_pin(pin_6);
        let p7 = pio.make_pio_pin(pin_7);

        // OUT and SET pin ranges both cover the 8 data pins.
        let pins = PinConfig {
            out_base: p0.pin(),
            out_count: 8,
            set_base: p0.pin(),
            set_count: 8,
            ..PinConfig::default()
        };
        unsafe { cfg.set_pins(pins) };

        cfg.use_program(&program.prg, &[]);

        // Clock: 800 kHz WS2812 rate × 10 PIO cycles per bit.
        let clock_freq = U24F8::from_num(clk_sys_freq() / 1000);
        let ws2812_freq = U24F8::from_num(800);
        let bit_freq = ws2812_freq * CYCLES_PER_BIT;
        cfg.clock_divider = clock_freq / bit_freq;

        // One OSR word (4 columns of 8 strip-bits) per 4 OUTs, refilled
        // without instruction cost — see the program comment above.
        cfg.fifo_join = FifoJoin::TxOnly;
        cfg.shift_out = ShiftConfig {
            auto_fill: true,
            threshold: 32,
            direction: ShiftDirection::Left,
        };

        sm.set_config(&cfg);

        sm.set_pin_dirs(Direction::Out, &[&p0, &p1, &p2, &p3, &p4, &p5, &p6, &p7]);
        sm.set_pins(Level::Low, &[&p0, &p1, &p2, &p3, &p4, &p5, &p6, &p7]);

        sm.set_enable(true);

        // BITS * N_LEDS is a multiple of 4 (24-bit pixels, u32 words).
        let n_words: usize = BITS * N_LEDS / 4;
        let buf_0 = alloc::vec![0u32; n_words];
        let buf_1 = alloc::vec![0u32; n_words];

        Self {
            dma: embassy_rp::dma::Channel::new(dma, irq),
            sm,
            buf: [buf_0, buf_1],
            next: 0,
            started: false,
        }
    }
}

impl<const N_LEDS: usize, P: Instance> Ws2812ParallelDriver<N_LEDS> for PioWs2812ParallelDriver<'_, P, 0, N_LEDS> {
    async fn write(&mut self, colors: &[[RGB8; N_LEDS]; N_STRIPS]) {
        // Bench breadcrumbs: "begin #1" without "done #1" = the first PIO/DMA
        // push hung; frames stopping mid-count = core1 died mid-stream.
        static WRITES: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
        let frame = WRITES.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        if frame == 0 {
            crate::tasks::usb_dbg::log(format_args!("disp: write #1 begin"));
        }

        // --- Transpose into the idle DMA buffer ---
        // Runs while the previous frame is still streaming, so this costs
        // wire time only when it exceeds the remaining transfer (torn
        // frame, see the type docs). One pass: four bit-columns are packed
        // into one big-endian u32 as they are produced — the old `words`
        // byte staging plus a separate pack pass is gone. Bit k of byte b
        // of word (led, w) is strip k's bit (23 - (4w + b)) of that LED,
        // MSB first, matching the left-shifting OSR.
        let words = &mut self.buf[self.next];

        for led in 0..N_LEDS {
            // Load all strips' GRB values. With N_STRIPS fixed at 8 the
            // compiler keeps this array in registers.
            let mut grb = [0u32; N_STRIPS];
            for strip in 0..N_STRIPS {
                let c = colors[strip][led];
                grb[strip] = ((c.g as u32) << 16) | ((c.r as u32) << 8) | (c.b as u32);
            }

            let base = led * (BITS / 4);
            for w in 0..(BITS / 4) {
                let mut out: u32 = 0;
                for b in 0..4 {
                    let shift = 23 - (w * 4 + b);
                    let mut col: u8 = 0;
                    for strip in 0..N_STRIPS {
                        col |= (((grb[strip] >> shift) as u8) & 1) << strip;
                    }
                    out |= (col as u32) << (24 - b * 8);
                }
                words[base + w] = out;
            }
        }

        // --- Hand over the wire to this frame ---
        if self.started {
            // `Transfer`'s waker plumbing is crate-private, so completion
            // is a busy poll. The transfer is normally long done by the
            // time the transpose finishes; the loop then costs one read.
            while self.dma.regs().ctrl_trig().read().busy() {
                embassy_time::Timer::after_micros(DMA_POLL_US).await;
            }
            // Pins have been low since the last bit; add explicit latch
            // margin on top of the DMA-poll overshoot.
            embassy_time::Timer::after_micros(LATCH_US).await;
        }

        // `dma_push` arms the channel (EN=true) inside `configure()` — the
        // transfer is live when the call returns, and the returned
        // `Transfer` future only polls the busy bit. Forget it deliberately:
        // dropping it runs the channel-abort path and kills the frame we
        // just started. Completion is detected by the busy poll above.
        core::mem::forget(self.sm.tx().dma_push(&mut self.dma, &self.buf[self.next], false));

        self.started = true;
        self.next ^= 1;

        if frame < 3 || (frame + 1) % 1000 == 0 {
            crate::tasks::usb_dbg::log(format_args!("disp: write #{} started", frame + 1));
        }
    }
}
