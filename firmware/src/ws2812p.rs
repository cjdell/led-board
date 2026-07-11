use alloc::vec::Vec;
use defmt::info;
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
    /// One OSR word = one 8-bit column (bit N of each of the 8 strips).
    /// We consume 8 bits per bit-period (auto-pull threshold = 8).
    pub fn new(common: &mut Common<'a, PIO>) -> Self {
        // One side-set pin is used as a scratch/enable line — here we
        // instead drive all timing via SET/OUT on the 8 data pins
        // directly, so no side-set is needed.
        let mut a: Assembler<32> = Assembler::new();

        let mut wrap_target = a.label();
        let mut wrap_source = a.label();

        a.bind(&mut wrap_target);

        a.pull(true, true); // ifempty=false, block=true
        a.pull(true, true); // ifempty=false, block=true
        a.pull(true, true); // ifempty=false, block=true
        a.pull(true, true); // ifempty=false, block=true

        for _ in 0..4 {
            // ── T1: drive all 8 lines HIGH (start of bit pulse) ──────────
            // SET pins, 0xFF would need 8-bit set; SET only does 5 bits.
            // Work-around: use MOV pins, !NULL  (all-ones -> pins)
            // then delay for T1-1 additional cycles.
            a.mov_with_delay(
                pio::program::MovDestination::PINS,
                pio::program::MovOperation::Invert,
                pio::program::MovSource::NULL,
                T1 - 1,
            );

            // ── T2: output the data column; pins stay HIGH for 1-bits ────
            // OUT PINS, 8  — shifts 8 bits from OSR to the 8 output pins.
            // Delay for T2-1 additional cycles.
            a.out_with_delay(OutDestination::PINS, 8, T2 - 1);

            // ── T3: drive all 8 lines LOW (end of bit pulse) ─────────────
            a.mov_with_delay(
                pio::program::MovDestination::PINS,
                pio::program::MovOperation::None,
                pio::program::MovSource::NULL,
                T3 - 1,
            );
        }

        a.bind(&mut wrap_source);

        let prg = a.assemble_with_wrap(wrap_source, wrap_target);
        let prg = common.load_program(&prg);
        Self { prg }
    }
}

const N_STRIPS: usize = 8;
const BITS: usize = 24;

/// Parallel WS2812 driver for 8 simultaneous strips.
///
/// - `S`        – PIO state machine index
/// - `N_LEDS`   – number of LEDs **per strip**
/// - `N_STRIPS` – must be 8 (kept as const for future flexibility)
pub struct PioWs2812ParallelDriver<'d, P: Instance, const S: usize, const N_LEDS: usize> {
    dma: Channel<'d>,
    sm: StateMachine<'d, P, S>,
    words: Vec<u8>,
    dma_buf: Vec<u32>,
}

impl<'d, P: Instance, const S: usize, const N_LEDS: usize> PioWs2812ParallelDriver<'d, P, S, N_LEDS> {
    /// Configure the state machine for parallel 8-strip output.
    ///
    /// `first_pin` must be the lowest-numbered of 8 **consecutive** GPIO pins.
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

        // OUT PINS base = p0, count = 8.
        // cfg.set_out_pins(&[&p0, &p1, &p2, &p3, &p4, &p5, &p6, &p7]);

        // SET PINS same range (for the MOV pins trick).
        // cfg.set_set_pins(&[&p0, &p1, &p2, &p3, &p4]);    // DOESN'T ALLOW MORE THAN 5 PINS!

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

        // Pull 8 bits at a time (one column of 8 strip-bits).
        cfg.fifo_join = FifoJoin::TxOnly;
        cfg.shift_out = ShiftConfig {
            // auto_fill: true,
            auto_fill: false,
            threshold: 32, // ← 8 bits per OSR refill
            direction: ShiftDirection::Left,
        };

        sm.set_config(&cfg);

        sm.set_pin_dirs(Direction::Out, &[&p0, &p1, &p2, &p3, &p4, &p5, &p6, &p7]);
        sm.set_pins(Level::Low, &[&p0, &p1, &p2, &p3, &p4, &p5, &p6, &p7]);

        sm.set_enable(true);

        let n_words: usize = (BITS * N_LEDS + 3) / 4;

        Self {
            dma: embassy_rp::dma::Channel::new(dma, irq),
            sm,
            words: alloc::vec![0u8; BITS * N_LEDS],
            dma_buf: alloc::vec![0u32; n_words],
        }
    }

    // /// Write one "frame" to all 8 strips simultaneously.
    // ///
    // /// `colors[strip][led]` — row = strip index (0-7), column = LED index.
    // ///
    // /// Internally this transposes the data into 24×N_LEDS columns of
    // /// 8 bits and streams them to the PIO via DMA.
    // pub async fn write_async(&mut self, colors: &[[RGB8; N_LEDS]; N_STRIPS]) {
    //     // 24 bit-columns per LED × N_LEDS LEDs.
    //     // Each column is one byte: bit[k] = bit value for strip k.

    //     for led in 0..N_LEDS {
    //         // Pack each strip's color into GRB order.
    //         let grb: [u32; N_STRIPS] = core::array::from_fn(|strip| {
    //             let c = colors[strip][led];
    //             (u32::from(c.g) << 16) | (u32::from(c.r) << 8) | u32::from(c.b)
    //         });

    //         // Transpose: for each of the 24 bit positions (MSB first),
    //         // collect one bit from each strip into a byte.
    //         for bit in 0..BITS {
    //             let shift = 23 - bit; // MSB first
    //             let mut col: u8 = 0;
    //             for strip in 0..N_STRIPS {
    //                 if (grb[strip] >> shift) & 1 == 1 {
    //                     col |= 1 << strip;
    //                 }
    //             }
    //             self.words[led * BITS + bit] = col;
    //         }
    //     }

    //     // The PIO consumes 8-bit chunks; DMA expects u32 words, so we
    //     // reinterpret the byte slice as a u32 slice (packed, BE within
    //     // each word — Left-shift drain order takes care of the rest).
    //     // Easiest: cast to &[u32] by chunking 4 bytes → 1 word.
    //     for (i, chunk) in self.words.chunks(4).enumerate() {
    //         let mut w = 0u32;
    //         for (j, &b) in chunk.iter().enumerate() {
    //             w |= (b as u32) << (24 - j * 8);
    //         }
    //         self.dma_buf[i] = w;
    //     }

    //     info!("self.dma_buf: {}", self.dma_buf.len());

    //     self.sm.tx().dma_push(&mut self.dma, &self.dma_buf, false).await;

    //     Timer::after_micros(55).await;
    // }
}

pub trait Ws2812ParallelDriver<const N_LEDS: usize> {
    fn write(&mut self, colors: &[[RGB8; N_LEDS]; N_STRIPS]) -> impl Future<Output = ()>;
}

impl<const N_LEDS: usize, P: Instance> Ws2812ParallelDriver<N_LEDS> for PioWs2812ParallelDriver<'_, P, 0, N_LEDS> {
    async fn write(&mut self, colors: &[[RGB8; N_LEDS]; N_STRIPS]) {
        // --- Phase 1: Transpose colors into bit-columns ---
        // Unroll the bit loop entirely for the common N_STRIPS <= 8 case.
        // Precompute GRB for all strips of the current LED in registers,
        // then use bitwise tricks to transpose without branching.

        let words = &mut self.words;

        for led in 0..N_LEDS {
            // Load all strips' GRB values. With N_STRIPS small and fixed at
            // compile time, this array lives entirely in registers.
            let mut grb = [0u32; N_STRIPS];
            for strip in 0..N_STRIPS {
                let c = colors[strip][led];
                // Avoid three separate shifts by packing directly.
                grb[strip] = ((c.g as u32) << 16) | ((c.r as u32) << 8) | (c.b as u32);
            }

            // Transpose 24 bits × N_STRIPS into 24 byte columns.
            // Pull the base pointer out of the loop to help the compiler
            // avoid recomputing it each iteration.
            let base = led * BITS;
            for bit in 0..BITS {
                // Single shift per strip: mask the relevant bit plane.
                let shift = 23 - bit;
                let mut col: u8 = 0;
                // This inner loop is the hot path. With N_STRIPS = 8 and a
                // fixed shift the compiler (LLVM) can unroll and vectorize it
                // into a handful of UBFX / ORR instructions on Cortex-M33.
                for strip in 0..N_STRIPS {
                    // Cast to u8 after isolating the bit, avoiding a wide OR.
                    col |= (((grb[strip] >> shift) as u8) & 1) << strip;
                }
                // SAFETY-note: base + bit is always in-bounds by construction.
                words[base + bit] = col;
            }
        }

        // --- Phase 2: Pack bytes into big-endian u32 words for DMA ---
        // Replace the chunks() iterator + inner loop with direct index
        // arithmetic. chunks() cannot communicate its stride to the
        // optimiser as cleanly as a plain index expression, so the compiler
        // can miss the opportunity to use LDM / LDRD on M33.
        //
        // N_LEDS * BITS must be a multiple of 4 (guaranteed by construction:
        // BITS = 24 and N_LEDS are both powers of two / multiples of four).
        debug_assert_eq!(words.len() % 4, 0);

        let dma_buf = &mut self.dma_buf;
        let n = words.len() / 4;
        for i in 0..n {
            let j = i * 4;
            // Four byte-loads + three shifts + three ORs — the compiler will
            // schedule these as a single LDR + ROR sequence on M33 when the
            // source slice is 4-byte aligned (which it is, being a &[u8]
            // field in a #[repr(C)] struct).
            dma_buf[i] = ((words[j] as u32) << 24)
                | ((words[j + 1] as u32) << 16)
                | ((words[j + 2] as u32) << 8)
                | (words[j + 3] as u32);
        }

        // // --- Phase 3: Push words to PIO TX FIFO ---
        // // Use try_write in a spin loop rather than checking full() then
        // // pushing separately; the two-instruction sequence is atomic on M33
        // // (no IRQ can land between them without losing data if PIO is fast),
        // // but the single-call form is cleaner and some HALs optimise it better.
        // for &word in self.dma_buf.iter() {
        //     while !self.sm.tx().try_write(word) {}
        // }

        // for b in &self.dma_buf {
        //     while self.sm.tx().full() {}
        //     self.sm.tx().push(*b);
        // }

        self.sm.tx().dma_push(&mut self.dma, &self.dma_buf, false).await;
    }
}
