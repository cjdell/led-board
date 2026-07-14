use alloc::boxed::Box;
use embedded_graphics::{
    Pixel,
    draw_target::DrawTarget,
    geometry::{Dimensions, OriginDimensions, Point, Size},
    pixelcolor::{PixelColor, Rgb565, Rgb888, RgbColor as _},
    primitives::Rectangle,
};
use micromath::F32Ext;

pub const WIDTH: usize = 60;
pub const HEIGHT: usize = 40;
pub const TOTAL_PIXELS: usize = 2400;

pub struct BufferTarget<C: PixelColor> {
    pub buffer: [C; TOTAL_PIXELS],
}

impl<C> BufferTarget<C>
where
    C: PixelColor + Default,
{
    pub fn new() -> Self {
        Self {
            buffer: [C::default(); TOTAL_PIXELS],
        }
    }
}

pub type BufferTargetRgb888 = BufferTarget<Rgb888>;
pub type BufferTargetRgb565 = BufferTarget<Rgb565>;

#[derive(Debug, Clone)]
pub struct AnimationParams {
    pub beat: u8,
    pub low: u8,
    pub med: u8,
    pub high: u8,
}

impl Default for AnimationParams {
    fn default() -> Self {
        Self {
            beat: 255,
            low: Default::default(),
            med: Default::default(),
            high: Default::default(),
        }
    }
}

impl AnimationParams {
    pub fn new(beat: u8, low: u8, med: u8, high: u8) -> Self {
        Self { beat, low, med, high }
    }
}

pub trait CloneableAnimation {
    fn clone_box(&self) -> Box<dyn Animation>;
}

pub trait Animation: CloneableAnimation {
    fn draw(&mut self, ms_since_start: u32, params: &AnimationParams, target: &mut BufferTargetRgb888) -> ();
}

impl Clone for Box<dyn Animation> {
    fn clone(&self) -> Self {
        self.clone_box()
    }
}

macro_rules! impl_animation_clone {
    ($type:ty) => {
        impl crate::common::CloneableAnimation for $type {
            fn clone_box(&self) -> Box<dyn crate::common::Animation> {
                Box::new(self.clone())
            }
        }
    };
}

impl<C> OriginDimensions for BufferTarget<C>
where
    C: PixelColor + Default,
{
    fn size(&self) -> Size {
        Size::new(WIDTH as u32, HEIGHT as u32)
    }
}

impl<C: PixelColor + Default> DrawTarget for BufferTarget<C> {
    type Color = C;

    type Error = ();

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = embedded_graphics::prelude::Pixel<Self::Color>>,
    {
        for Pixel(pos, color) in pixels {
            if pos.x < 0 || pos.x >= self.bounding_box().size.width as i32 {
                continue;
            }
            if pos.y < 0 || pos.y >= self.bounding_box().size.height as i32 {
                continue;
            }

            self.buffer[(pos.y * WIDTH as i32 + pos.x) as usize] = color;
        }

        Ok(())
    }
}

/// Replacement for [`static_cell::make_static`](https://docs.rs/static_cell/latest/static_cell/macro.make_static.html) for use cases when the type is known.
#[macro_export]
macro_rules! make_static {
    ($t:ty, $val:expr) => ($crate::make_static!($t, $val,));
    ($t:ty, $val:expr, $(#[$m:meta])*) => {{
        $(#[$m])*
        static STATIC_CELL: static_cell::StaticCell<$t> = static_cell::StaticCell::new();
        STATIC_CELL.init($val)
    }};
}

pub fn apply_power_limit(buffer: &mut BufferTarget<Rgb888>, power_limit: f32) {
    let num_leds = buffer.buffer.len();
    let max_possible_power = num_leds as f32 * 765.0; // 255*3 per LED
    let max_allowed_power = power_limit * max_possible_power; // e.g., 50% power limit

    // Step 1: Calculate current total power
    let mut total_power = 0.0;
    for p in &buffer.buffer {
        total_power += p.r() as f32 + p.g() as f32 + p.b() as f32;
    }

    // Step 2: If we're under the limit, do nothing
    if total_power <= max_allowed_power || total_power == 0.0 {
        return;
    }

    // Step 3: Calculate scaling factor
    let scale_factor = max_allowed_power / total_power;

    // Step 4: Apply scaling to each LED
    for p in buffer.buffer.iter_mut() {
        let r = (p.r() as f32 * scale_factor).round() as u8;
        let g = (p.g() as f32 * scale_factor).round() as u8;
        let b = (p.b() as f32 * scale_factor).round() as u8;

        // Clamp to 0-255 (shouldn't exceed due to scaling, but just in case)
        *p = Rgb888::new(r.min(255), g.min(255), b.min(255));
    }
}
