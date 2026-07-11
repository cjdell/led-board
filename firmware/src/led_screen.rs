use crate::ws2812p::Ws2812ParallelDriver;
use alloc::vec::Vec;
use defmt::error;
use embedded_graphics::{
    geometry::{Dimensions, Point, Size},
    pixelcolor::{Rgb888, RgbColor as _},
    primitives::Rectangle,
};
use smart_leds::RGB8;

pub const WIDTH: usize = 60;
pub const HEIGHT: usize = 40;
pub const TOTAL_PIXELS: usize = 2400;

const NUM_STRIPS: usize = 8;
const PIXELS_PER_STRIP: usize = TOTAL_PIXELS / NUM_STRIPS;
const ROWS_PER_STRIP: usize = PIXELS_PER_STRIP / WIDTH;

pub struct LedScreen<DRIVER>
where
    DRIVER: Ws2812ParallelDriver<PIXELS_PER_STRIP>,
{
    buffer: [RGB8; TOTAL_PIXELS],
    strips: [[RGB8; PIXELS_PER_STRIP]; NUM_STRIPS],
    driver: DRIVER,
}

impl<DRIVER: Ws2812ParallelDriver<PIXELS_PER_STRIP>> LedScreen<DRIVER> {
    pub fn new(driver: DRIVER) -> Self {
        Self {
            buffer: [RGB8::default(); TOTAL_PIXELS],
            strips: [[RGB8::default(); PIXELS_PER_STRIP]; NUM_STRIPS], // Use RGB8::default() if available
            driver,
        }
    }

    pub fn copy_eg_buffer(&mut self, buffer: &[Rgb888; TOTAL_PIXELS]) {
        self.copy_buffer(buffer.iter().map(|p| RGB8::new(p.r(), p.g(), p.b())).collect());
    }

    pub fn copy_buffer(&mut self, buffer: Vec<RGB8>) {
        if buffer.len() != TOTAL_PIXELS {
            error!("write_buffer: Bad pixel count: {}", buffer.len());
            return;
        }

        self.buffer.copy_from_slice(&buffer);
    }

    pub async fn flush(&mut self) {
        let strip_to_wire = [0, 4, 1, 5, 2, 6, 3, 7]; // Make wiring simpler

        // `buffer` is a frame buffer of 60x40 = 2400 pixels, row-major order
        for strip_idx in 0..NUM_STRIPS {
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

impl<DRIVER: Ws2812ParallelDriver<PIXELS_PER_STRIP>> Dimensions for LedScreen<DRIVER> {
    fn bounding_box(&self) -> Rectangle {
        Rectangle::new(Point::zero(), Size::new(WIDTH as u32, HEIGHT as u32))
    }
}
