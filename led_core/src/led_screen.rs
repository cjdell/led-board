use core::future::Future;
use embedded_graphics::{
    geometry::{Dimensions, Point, Size},
    pixelcolor::{Rgb888, RgbColor as _},
    primitives::Rectangle,
};
use smart_leds::RGB8;

pub const WIDTH: usize = 60;
pub const HEIGHT: usize = 40;
pub const TOTAL_PIXELS: usize = 2400;

pub const N_STRIPS: usize = 8;
pub const PIXELS_PER_STRIP: usize = TOTAL_PIXELS / N_STRIPS;
pub const ROWS_PER_STRIP: usize = PIXELS_PER_STRIP / WIDTH;

/// A parallel WS2812-style sink for the eight strips: one frame, all
/// strips at once. On the Pico this is the PIO + DMA driver
/// (`firmware::ws2812p`); in the simulator it is the dot-matrix window
/// (which sees the strips re-expanded back into rows).
pub trait Ws2812ParallelDriver<const N_LEDS: usize> {
    fn write(&mut self, colors: &[[RGB8; N_LEDS]; N_STRIPS]) -> impl Future<Output = ()>;
}

pub struct LedScreen<DRIVER>
where
    DRIVER: Ws2812ParallelDriver<PIXELS_PER_STRIP>,
{
    buffer: [RGB8; TOTAL_PIXELS],
    strips: [[RGB8; PIXELS_PER_STRIP]; N_STRIPS],
    driver: DRIVER,
}

impl<DRIVER: Ws2812ParallelDriver<PIXELS_PER_STRIP>> LedScreen<DRIVER> {
    pub fn new(driver: DRIVER) -> Self {
        Self {
            buffer: [RGB8::default(); TOTAL_PIXELS],
            strips: [[RGB8::default(); PIXELS_PER_STRIP]; N_STRIPS],
            driver,
        }
    }

    /// Copy a rendered frame (row-major, 60×40) into the panel buffer.
    /// Takes a reference — a per-frame `Vec` here once meant a 7.2 KB
    /// heap round trip every cycle of the display worker.
    pub fn copy_eg_buffer(&mut self, buffer: &[Rgb888; TOTAL_PIXELS]) {
        for (dst, src) in self.buffer.iter_mut().zip(buffer) {
            *dst = RGB8::new(src.r(), src.g(), src.b());
        }
    }

    pub async fn flush(&mut self) {
        let strip_to_wire = strip_to_wire_map();

        // `buffer` is a frame buffer of 60x40 = 2400 pixels, row-major order
        for strip_idx in 0..N_STRIPS {
            let base_row = strip_idx * ROWS_PER_STRIP; // First global row this strip handles

            for row_in_strip in 0..ROWS_PER_STRIP {
                let global_row = base_row + row_in_strip;
                let start_pixel = global_row * WIDTH; // Start index in buffer for this row
                let strip_offset = row_in_strip * WIDTH; // Where in the strip to write

                let wire = strip_to_wire[strip_idx];

                if (row_in_strip + strip_idx) % 2 == 0 {
                    // Even row within strip → forward (left to right)
                    for col in 0..WIDTH {
                        self.strips[wire][strip_offset + col] = self.buffer[start_pixel + col];
                    }
                } else {
                    // Odd row within strip → reversed (right to left)
                    for col in 0..WIDTH {
                        self.strips[wire][strip_offset + col] = self.buffer[start_pixel + (WIDTH - 1) - col];
                    }
                }
            }
        }

        self.driver.write(&self.strips).await;
    }
}

/// Logical strip index → physical wire index (makes the wiring diagram
/// readable). Shared by the screen's scatter and any host that must undo
/// it (the simulator window).
pub const fn strip_to_wire_map() -> [usize; N_STRIPS] {
    [0, 4, 1, 5, 2, 6, 3, 7]
}

impl<DRIVER: Ws2812ParallelDriver<PIXELS_PER_STRIP>> Dimensions for LedScreen<DRIVER> {
    fn bounding_box(&self) -> Rectangle {
        Rectangle::new(Point::zero(), Size::new(WIDTH as u32, HEIGHT as u32))
    }
}
