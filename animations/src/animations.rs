use crate::common::{Animation, AnimationParams, BufferTarget, BufferTargetRgb888, CloneableAnimation, HEIGHT, WIDTH};
use alloc::{
    boxed::Box,
    string::{String, ToString as _},
};
use core::{f32::consts::PI, primitive::f32};
use embedded_graphics::{
    Drawable as _,
    geometry::Point,
    mono_font::{MonoTextStyle, ascii::FONT_10X20},
    pixelcolor::{Rgb888, RgbColor},
    primitives::{Circle, Primitive as _, PrimitiveStyle},
    text::{Alignment, Baseline, Text, TextStyleBuilder},
};
use micromath::F32Ext;
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
pub enum AnimationEnum {
    Blank,
    TwoLineMessage(String, String),
    ScrollingMessage(String),
    Circles,
    Rainbows,
    TestPattern,
    RainbowWheel,
    SpinningCube,
    Columns,
    Intensity,
}

impl Animation for AnimationEnum {
    fn draw(&mut self, ms_since_start: u32, params: &AnimationParams, target: &mut BufferTargetRgb888) -> () {
        match self {
            AnimationEnum::Blank => {
                target.buffer.fill(Rgb888::new(0x00, 0, 0x10));
            }
            AnimationEnum::TwoLineMessage(line1, line2) => {
                let style = MonoTextStyle::new(&FONT_10X20, Rgb888::WHITE);

                Text::with_text_style(
                    line1,
                    Point::new(0, 0),
                    style,
                    TextStyleBuilder::new()
                        .alignment(Alignment::Left)
                        .baseline(Baseline::Top)
                        .build(),
                )
                .draw(target)
                .unwrap();

                Text::with_text_style(
                    line2,
                    Point::new(0, 20),
                    style,
                    TextStyleBuilder::new()
                        .alignment(Alignment::Left)
                        .baseline(Baseline::Top)
                        .build(),
                )
                .draw(target)
                .unwrap();
            }
            AnimationEnum::ScrollingMessage(msg) => {
                let style = MonoTextStyle::new(&FONT_10X20, Rgb888::WHITE);

                // Convert milliseconds to seconds for smoother animation
                let t = ms_since_start as f32 / 1000.0;

                // Horizontal scroll: moves left at a steady pace
                let scroll_offset = (t * 50.0) as i32 % (msg.len() as i32 * 10); // 50 px/sec
                let x = -scroll_offset;

                // Vertical wave: sine wave modulation based on character position
                let base_y = (HEIGHT as i32 - 20) / 2; // Center vertically
                let wave_amplitude = 8.0; // Max vertical displacement (pixels)
                let wave_frequency = 0.1; // How many waves fit across the text width

                // We'll animate each character with a slightly different phase
                // Since we're drawing the whole string as one, we simulate wave by offsetting Y per char
                // But Text::with_text_style draws the whole string at one position — so we need to draw char-by-char

                // 👇 SOLUTION: Draw each character individually with custom Y offset
                for (i, c) in msg.chars().enumerate() {
                    let char_width = 10; // Assuming FONT_10X20 is 10px wide
                    let char_x = x + i as i32 * char_width;

                    // Calculate vertical wave for this character
                    // Phase depends on character index + time
                    let phase = (i as f32 * wave_frequency + t * 2.0) * PI;
                    let y_offset = (phase.sin() * wave_amplitude) as i32;

                    let y = base_y + y_offset;

                    // Draw each character individually
                    Text::with_text_style(
                        &c.to_string(),
                        Point::new(char_x, y),
                        style,
                        TextStyleBuilder::new()
                            .alignment(Alignment::Left)
                            .baseline(Baseline::Top)
                            .build(),
                    )
                    .draw(target)
                    .unwrap();
                }
            }
            AnimationEnum::Circles => {
                Circle::new(Point::new((ms_since_start >> 6) as i32 % WIDTH as i32, 0), 30)
                    .into_styled(PrimitiveStyle::with_fill(Rgb888::WHITE))
                    .draw(target)
                    .unwrap();
            }
            AnimationEnum::Rainbows => {
                let time = ms_since_start as f32 / 1000.0; // Convert to seconds
                let hue_offset = (time * 0.5).fract(); // Slowly rotate hue base (0.5 = half a cycle per second)
                let frequency = 2.0; // Higher = more stripes
                let speed = 1.5; // How fast the waves move

                for y in 0..HEIGHT {
                    for x in 0..WIDTH {
                        // Create a waving, radial gradient based on position and time
                        let dx = x as f32 - WIDTH as f32 / 2.0;
                        let dy = y as f32 - HEIGHT as f32 / 2.0;
                        let distance = (dx * dx + dy * dy).sqrt();
                        let angle = (dx.atan2(dy) + time * speed) % (2.0 * PI);

                        // Combine radial and angular modulation for swirling rainbow effect
                        let hue_base = (angle * frequency + distance * 0.05) % 1.0;
                        let hue = (hue_base + hue_offset) % 1.0;

                        // Convert HSV (hue, 1.0, 1.0) to RGB
                        let rgb = hsv_to_rgb(hue, 1.0, 1.0);

                        target.buffer[y * WIDTH + x] = rgb;
                    }
                }
            }
            AnimationEnum::TestPattern => {
                let pixel = (ms_since_start / 16) % WIDTH as u32;

                for y in 0..HEIGHT {
                    for x in 0..WIDTH {
                        if x == pixel as usize {
                            target.buffer[y * WIDTH + x] = Rgb888::new(0xff, 0xff, 0xff);
                        }
                    }

                    target.buffer[y * WIDTH + 0] = Rgb888::new(0xff, 0xff, 0xff);
                    target.buffer[y * WIDTH + WIDTH - 1] = Rgb888::new(0xff, 0xff, 0xff);
                }
            }
            AnimationEnum::RainbowWheel => {
                let center_x = WIDTH as f32 / 2.0;
                let center_y = HEIGHT as f32 / 2.0;
                let radius = (WIDTH.min(HEIGHT) as f32) / 2.0 - 2.0; // Leave a small margin

                let angle_offset = (ms_since_start as f32) * 0.001; // Slow rotation: 1 rad/s

                for y in 0..HEIGHT {
                    for x in 0..WIDTH {
                        // Calculate distance from center
                        let dx = x as f32 - center_x;
                        let dy = y as f32 - center_y;
                        let distance = (dx * dx + dy * dy).sqrt();

                        // Only draw pixels inside the circle
                        // if distance <= radius {
                        let angle = angle_offset + (dy / dx).atan() % 1.0;

                        let hue = angle % 1.0;

                        let rgb = hsv_to_rgb(hue, 1.0, 1.0);

                        target.buffer[y * WIDTH + x] = rgb;
                        // } else {
                        //     // Optional: fade out to black outside the wheel
                        //     target.buffer[y * WIDTH + x] = Rgb888::new(0x00, 0x00, 0x00);
                        // }
                    }
                }
            }
            AnimationEnum::SpinningCube => {
                // Clear buffer to black
                for pixel in target.buffer.iter_mut() {
                    *pixel = Rgb888::new(0x00, 0x00, 0x00);
                }

                // Cube vertices (8 corners, normalized to [-1, 1] in all dimensions)
                let vertices = [
                    [-1.0, -1.0, -1.0], // 0: front-bottom-left
                    [1.0, -1.0, -1.0],  // 1: front-bottom-right
                    [1.0, 1.0, -1.0],   // 2: front-top-right
                    [-1.0, 1.0, -1.0],  // 3: front-top-left
                    [-1.0, -1.0, 1.0],  // 4: back-bottom-left
                    [1.0, -1.0, 1.0],   // 5: back-bottom-right
                    [1.0, 1.0, 1.0],    // 6: back-top-right
                    [-1.0, 1.0, 1.0],   // 7: back-top-left
                ];

                // Edges: each pair of connected vertices
                let edges = [
                    (0, 1),
                    (1, 2),
                    (2, 3),
                    (3, 0), // front face
                    (4, 5),
                    (5, 6),
                    (6, 7),
                    (7, 4), // back face
                    (0, 4),
                    (1, 5),
                    (2, 6),
                    (3, 7), // connecting edges
                ];

                // Rotation angles based on time (in radians)
                let t = ms_since_start as f32 * 0.0008; // ~0.8 rad/s total rotation
                let cos_t = t.cos();
                let sin_t = t.sin();

                let cos_x = (t * 0.7).cos();
                let sin_x = (t * 0.7).sin();
                let cos_y = (t * 0.5).cos();
                let sin_y = (t * 0.5).sin();
                let cos_z = t.cos();
                let sin_z = t.sin();

                // Projected vertices (2D screen space)
                let mut projected = [(0.0, 0.0); 8];
                let mut screen_coords = [(0, 0); 8];

                let center_x = WIDTH as f32 / 2.0;
                let center_y = HEIGHT as f32 / 2.0;
                let scale = (WIDTH.min(HEIGHT) as f32) * 0.7; // Scale to fit screen

                // Apply 3D rotation and perspective projection
                for (i, &v) in vertices.iter().enumerate() {
                    let mut x = v[0];
                    let mut y = v[1];
                    let mut z = v[2];

                    // Rotate around X axis
                    let y1 = y * cos_x - z * sin_x;
                    let z1 = y * sin_x + z * cos_x;
                    y = y1;
                    z = z1;

                    // Rotate around Y axis
                    let x1 = x * cos_y + z * sin_y;
                    let z2 = -x * sin_y + z * cos_y;
                    x = x1;
                    z = z2;

                    // Rotate around Z axis
                    let x2 = x * cos_z - y * sin_z;
                    let y2 = x * sin_z + y * cos_z;
                    x = x2;
                    y = y2;

                    // Perspective projection (simulate distance)
                    let perspective = 3.0 + z; // depth factor — avoid division by zero
                    if perspective <= 0.0 {
                        continue; // Behind camera
                    }

                    let proj_x = x * scale / perspective;
                    let proj_y = y * scale / perspective;

                    projected[i] = (proj_x, proj_y);
                    screen_coords[i] = ((center_x + proj_x) as i32, (center_y + proj_y) as i32);
                }

                // Draw edges
                for &(a, b) in &edges {
                    let (x0, y0) = screen_coords[a];
                    let (x1, y1) = screen_coords[b];

                    // Skip if either point is off-screen
                    if x0 < 0
                        || x0 >= WIDTH as i32
                        || y0 < 0
                        || y0 >= HEIGHT as i32
                        || x1 < 0
                        || x1 >= WIDTH as i32
                        || y1 < 0
                        || y1 >= HEIGHT as i32
                    {
                        continue;
                    }

                    // Bresenham's line algorithm for smooth wireframe
                    draw_line(x0, y0, x1, y1, Rgb888::new(0xff, 0xff, 0xff), target);
                }
            }
            AnimationEnum::Columns => {
                for y in 0..HEIGHT {
                    for x in 0..WIDTH {
                        if x % 8 == 0 {
                            target.buffer[y * WIDTH + x] = Rgb888::new(0x0f, 0x0f, 0x0f);
                        } else {
                            target.buffer[y * WIDTH + x] = Rgb888::new(0, 0, 0);
                        }
                    }
                }
            }
            AnimationEnum::Intensity => {
                // #[cfg(feature = "std")]
                // std::println!("{}", params.beat);
                for y in 0..2 {
                    for x in 0..WIDTH {
                        target.buffer[y * WIDTH + x] = Rgb888::new(params.beat, params.beat, params.beat);
                    }
                }
            }
        }
    }
}

impl CloneableAnimation for AnimationEnum {
    fn clone_box(&self) -> Box<dyn Animation> {
        Box::new(self.clone())
    }
}

// Helper: Convert HSV (0-1) to RGB888
fn hsv_to_rgb(h: f32, s: f32, v: f32) -> Rgb888 {
    let i = (h * 6.0) as u8;
    let f = h * 6.0 - i as f32;
    let p = (v * (1.0 - s)) * 255.0;
    let q = (v * (1.0 - f * s)) * 255.0;
    let t = (v * (1.0 - (1.0 - f) * s)) * 255.0;
    let v255 = (v * 255.0) as u8;

    let (r, g, b) = match i {
        0 => (v255, t as u8, p as u8),
        1 => (q as u8, v255, p as u8),
        2 => (p as u8, v255, t as u8),
        3 => (p as u8, q as u8, v255),
        4 => (t as u8, p as u8, v255),
        _ => (v255, p as u8, q as u8),
    };

    Rgb888::new(r, g, b)
}

// Simple Bresenham line drawing (for wireframe edges)
fn draw_line(x0: i32, y0: i32, x1: i32, y1: i32, color: Rgb888, target: &mut BufferTargetRgb888) {
    let dx = (x1 - x0).abs();
    let dy = (y1 - y0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx - dy;

    let mut x = x0;
    let mut y = y0;

    loop {
        // Clamp to screen bounds
        if x >= 0 && x < WIDTH as i32 && y >= 0 && y < HEIGHT as i32 {
            target.buffer[y as usize * WIDTH + x as usize] = color;
        }

        if x == x1 && y == y1 {
            break;
        }

        let e2 = 2 * err;
        if e2 > -dy {
            err -= dy;
            x += sx;
        }
        if e2 < dx {
            err += dx;
            y += sy;
        }
    }
}
