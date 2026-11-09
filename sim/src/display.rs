//! The simulator's `Ws2812ParallelDriver`: the eight strip buffers the
//! screen model scatters for the physical wiring get re-expanded back into
//! the 60x40 row-major frame and handed to the minifb window on the main
//! thread. Exercising the real scatter code (wiring permutation + row
//! zig-zag) is the point — the window is a bug-catcher for it.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use led_core::led_screen::{HEIGHT, N_STRIPS, PIXELS_PER_STRIP, ROWS_PER_STRIP, WIDTH, Ws2812ParallelDriver, strip_to_wire_map};
use smart_leds::RGB8;

/// Latest frame as packed 0xAARRGGBB, the shared hand-off to the window
/// thread. `dirty` lets the main thread skip identical redraws.
#[derive(Clone)]
pub struct SharedFrame {
    pixels: Arc<Mutex<Vec<u32>>>,
    dirty: Arc<AtomicU64>,
    generation: Arc<AtomicU64>,
}

impl SharedFrame {
    pub fn new() -> Self {
        Self {
            pixels: Arc::new(Mutex::new(vec![0xFF00_0000; WIDTH * HEIGHT])),
            dirty: Arc::new(AtomicU64::new(0)),
            generation: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Copy the newest frame into `out` (len WIDTH*HEIGHT). Returns true if
    /// a new frame arrived since the last call.
    pub fn take_into(&mut self, out: &mut [u32]) -> bool {
        let frame_gen = self.generation.load(Ordering::Acquire);
        let last = self.dirty.load(Ordering::Relaxed);

        if frame_gen != last {
            let pixels = self.pixels.lock().unwrap();
            out.copy_from_slice(&pixels);
            self.dirty.store(frame_gen, Ordering::Relaxed);
            return true;
        }

        false
    }
}

pub struct WindowDisplay {
    frame: SharedFrame,
    /// wire index → logical strip index (inverse of the wiring map)
    wire_to_logical: [usize; N_STRIPS],
}

impl WindowDisplay {
    pub fn new(frame: SharedFrame) -> Self {
        let wire_map = strip_to_wire_map();
        let mut wire_to_logical = [0usize; N_STRIPS];
        for (logical, &wire) in wire_map.iter().enumerate() {
            wire_to_logical[wire] = logical;
        }

        Self { frame, wire_to_logical }
    }
}

impl Ws2812ParallelDriver<PIXELS_PER_STRIP> for WindowDisplay {
    async fn write(&mut self, colors: &[[RGB8; PIXELS_PER_STRIP]; N_STRIPS]) {
        let mut pixels = self.frame.pixels.lock().unwrap();

        for wire in 0..N_STRIPS {
            let logical = self.wire_to_logical[wire];
            let base_row = logical * ROWS_PER_STRIP;

            for row_in_strip in 0..ROWS_PER_STRIP {
                let global_row = base_row + row_in_strip;
                let strip_offset = row_in_strip * WIDTH;

                for col in 0..WIDTH {
                    let src = if (row_in_strip + logical) % 2 == 0 {
                        // Even row within strip → forward (left to right)
                        strip_offset + col
                    } else {
                        // Odd row within strip → reversed (right to left)
                        strip_offset + (WIDTH - 1) - col
                    };

                    let c = colors[wire][src];

                    pixels[global_row * WIDTH + col] = 0xFF00_0000 | ((c.r as u32) << 16) | ((c.g as u32) << 8) | c.b as u32;
                }
            }
        }

        self.frame.generation.fetch_add(1, Ordering::Release);

        // The Pico's flush awaits the PIO DMA (~2 ms of bus time per frame),
        // which paces the display worker and lets sibling tasks run. The
        // window write is instant, so yield explicitly — without it the
        // animation loop would monopolise the embassy executor.
        embassy_time::Timer::after_micros(2_000).await;
    }
}
