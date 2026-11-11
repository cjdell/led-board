use alloc::boxed::Box;
use embedded_graphics::{
    Pixel,
    draw_target::DrawTarget,
    geometry::{Dimensions, OriginDimensions, Size},
    pixelcolor::{PixelColor, Rgb565, Rgb888, RgbColor as _},
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

/// sRGB → linear PWM. The SK6812-class strips are linear-light devices: writing
/// an authored sRGB value directly makes every 10–30% background look like a
/// 35–60% wash. Gamma-encode here so the panel matches the web preview and the
/// dark skies stay dark.
#[allow(dead_code)]
#[rustfmt::skip]
const GAMMA: [u8; 256] = [
      0,   0,   0,   0,   0,   0,   0,   0,   0,   0,   0,   0,   1,   1,   1,   1,
      1,   1,   1,   1,   2,   2,   2,   2,   2,   2,   3,   3,   3,   3,   4,   4,
      4,   4,   5,   5,   5,   5,   6,   6,   6,   7,   7,   7,   8,   8,   8,   9,
      9,   9,  10,  10,  11,  11,  11,  12,  12,  13,  13,  14,  14,  15,  15,  16,
     16,  17,  17,  18,  18,  19,  19,  20,  20,  21,  21,  22,  23,  23,  24,  24,
     25,  26,  26,  27,  28,  28,  29,  30,  30,  31,  32,  32,  33,  34,  35,  35,
     36,  37,  38,  38,  39,  40,  41,  42,  42,  43,  44,  45,  46,  47,  47,  48,
     49,  50,  51,  52,  53,  54,  55,  56,  56,  57,  58,  59,  60,  61,  62,  63,
     64,  65,  66,  67,  68,  69,  70,  71,  73,  74,  75,  76,  77,  78,  79,  80,
     81,  82,  84,  85,  86,  87,  88,  89,  91,  92,  93,  94,  95,  97,  98,  99,
    100, 102, 103, 104, 105, 107, 108, 109, 111, 112, 113, 115, 116, 117, 119, 120,
    121, 123, 124, 126, 127, 128, 130, 131, 133, 134, 136, 137, 139, 140, 142, 143,
    145, 146, 148, 149, 151, 152, 154, 155, 157, 158, 160, 162, 163, 165, 166, 168,
    170, 171, 173, 175, 176, 178, 180, 181, 183, 185, 186, 188, 190, 192, 193, 195,
    197, 199, 200, 202, 204, 206, 207, 209, 211, 213, 215, 217, 218, 220, 222, 224,
    226, 228, 230, 232, 233, 235, 237, 239, 241, 243, 245, 247, 249, 251, 253, 255,
];

#[cfg(not(feature = "std"))]
fn pwm_encode(c: Rgb888) -> Rgb888 {
    Rgb888::new(GAMMA[c.r() as usize], GAMMA[c.g() as usize], GAMMA[c.b() as usize])
}

#[cfg(feature = "std")]
fn pwm_encode(c: Rgb888) -> Rgb888 {
    c
}

pub fn apply_power_limit(buffer: &mut BufferTarget<Rgb888>, power_limit: f32) {
    let num_leds = buffer.buffer.len();
    let max_possible_power = num_leds as f32 * 765.0; // 255*3 per LED
    let max_allowed_power = power_limit * max_possible_power; // e.g., 50% power limit

    // Step 1: gamma-encode to linear-light PWM (hardware only — the sim's
    // monitor already applies sRGB) and total the actual output power.
    let mut total_power: u32 = 0;

    for p in buffer.buffer.iter_mut() {
        *p = pwm_encode(*p);
        total_power += p.r() as u32 + p.g() as u32 + p.b() as u32;
    }

    // Step 2: If we're under the limit, do nothing
    if (total_power as f32) <= max_allowed_power || total_power == 0 {
        return;
    }

    // Step 3: Calculate scaling factor, and a 256-entry value LUT: 256
    // float multiplies + rounds beat 7200 of them per frame, and the per-
    // pixel pass becomes three byte loads.
    let scale_factor = max_allowed_power / total_power as f32;
    let lut: [u8; 256] = core::array::from_fn(|i| (i as f32 * scale_factor).round().min(255.0) as u8);

    // Step 4: Apply scaling to each LED
    for p in buffer.buffer.iter_mut() {
        *p = Rgb888::new(lut[p.r() as usize], lut[p.g() as usize], lut[p.b() as usize]);
    }
}
