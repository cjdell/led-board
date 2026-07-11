use embedded_graphics::{
    Pixel,
    draw_target::DrawTarget,
    geometry::{Dimensions, Point, Size},
    pixelcolor::{
        Rgb888,
        raw::{RawData as _, RawU24},
    },
    primitives::Rectangle,
};

pub const WIDTH: usize = 60;
pub const HEIGHT: usize = 40;
pub const TOTAL_PIXELS: usize = 2400;

pub struct SimScreen {
    pub buffer: Vec<u32>,
}

impl SimScreen {
    pub fn new() -> Self {
        Self {
            buffer: vec![0; WIDTH * HEIGHT],
        }
    }
}

impl Dimensions for SimScreen {
    fn bounding_box(&self) -> embedded_graphics::primitives::Rectangle {
        Rectangle::new(Point::zero(), Size::new(WIDTH as u32, HEIGHT as u32))
    }
}

impl DrawTarget for SimScreen {
    type Color = Rgb888;

    type Error = ();

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = embedded_graphics::prelude::Pixel<Self::Color>>,
    {
        for Pixel(pos, color) in pixels {
            // println!("{} {}", pos.x, pos.y);

            if pos.x < 0 || pos.x >= self.bounding_box().size.width as i32 {
                continue;
            }
            if pos.y < 0 || pos.y >= self.bounding_box().size.height as i32 {
                continue;
            }

            let color: RawU24 = color.into();
            let raw: u32 = color.into_inner();

            self.buffer[(pos.y * WIDTH as i32 + pos.x) as usize] = raw | 0xFF000000;
        }

        Ok(())
    }
}
