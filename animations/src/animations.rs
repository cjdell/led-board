use crate::TOTAL_PIXELS;
use crate::common::{Animation, AnimationParams, BufferTarget, BufferTargetRgb888, CloneableAnimation, HEIGHT, WIDTH};
use crate::draw::{draw_circle, draw_circle_filled, draw_line, draw_rect_filled, draw_triangle_filled};
use alloc::{
    boxed::Box,
    string::{String, ToString as _},
};
use core::{f32::consts::PI, primitive::f32};
use embedded_graphics::{
    Drawable as _,
    geometry::Point,
    mono_font::{
        MonoTextStyle, MonoTextStyleBuilder,
        ascii::{FONT_10X20, FONT_6X12},
    },
    pixelcolor::{Rgb888, RgbColor},
    primitives::{Circle, Primitive as _, PrimitiveStyle},
    text::{Alignment, Baseline, Text, TextStyleBuilder},
};
use micromath::F32Ext;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum AnimationEnum {
    Blank,
    TwoLineMessage(String, String),
    ScrollingMessage(String),
    Circles,
    Fireworks,
    SynthWave,
    Rainbows,
    TestPattern,
    RainbowWheel,
    SpinningCube,
    Ship,
    Columns,
    Intensity,
    BarChart,
    LineChart,
    EchoOfTheSoul,
    HarmonicSpiral,
    OctopusOfSound,
    MagneticField,
    SilentDancer,
    CenteredTitle(String, String),
    StatusCard(String, String),
    Aurora,
    Starfield,
    Snowfall,
    FallingLeaves,
    Fireflies,
    VinylSpin,
    CanalBoats,
}

impl Animation for AnimationEnum {
    fn draw(&mut self, ms_since_start: u32, params: &AnimationParams, target: &mut BufferTargetRgb888) -> () {
        match self {
            AnimationEnum::Blank => {
                target.buffer.fill(Rgb888::new(0x00, 0, 0x10));
            }
            AnimationEnum::CenteredTitle(line1, line2) => {
                Self::draw_card(line1, line2, Rgb888::new(255, 200, 80), target);
            }
            AnimationEnum::StatusCard(line1, line2) => {
                // Red alert card: the display worker's status-screen mode
                // (join failures, link lost, AP fallback) overlays these on
                // top of the playlist so the board reads its own state.
                Self::draw_card(line1, line2, Rgb888::new(255, 60, 40), target);
            }
            AnimationEnum::Aurora => {
                let t = ms_since_start as f32 / 1000.0;

                // Faint stars behind the curtains
                for i in 0..30u32 {
                    let h = fnv(i, 7);
                    let x = (h % WIDTH as u32) as usize;
                    let y = ((h >> 8) % 22u32) as usize;
                    let tw = 0.15 + 0.25 * ((sinp(t * (1.0 + ((h >> 4) % 8) as f32 * 0.2) + h as f32 * 0.01)) * 0.5 + 0.5);
                    let v = (tw * 255.0) as u8;
                    target.buffer[y * WIDTH + x] = Rgb888::new(v, v, (v as u16 + 30).min(255) as u8);
                }

                // Two layered curtains drawn as vertical gradient columns
                for layer in 0..2u32 {
                    let speed = 0.6 + layer as f32 * 0.35;
                    let hue_top = if layer == 0 { 0.34 } else { 0.42 };

                    for x in 0..WIDTH {
                        let xf = x as f32;
                        let wave = sinp(xf * 0.17 + t * speed) + 0.6 * sinp(xf * 0.06 - t * speed * 0.8);
                        let top = 6.0 + wave * 5.0 + layer as f32 * 4.0;
                        let thick = 15.0 + 5.0 * sinp(xf * 0.13 + t * (1.0 + speed));
                        let shimmer = 0.55 + 0.45 * sinp(xf * 0.55 + t * (2.0 + speed));

                        let end = (top + thick) as i32;
                        let mut y = top.floor() as i32;
                        while y < end {
                            if y >= 0 && y < HEIGHT as i32 && thick > 1.0 {
                                let fy = (y as f32 - top) / thick; // 0 at band top, 1 at bottom
                                let v = (1.0 - fy) * shimmer;
                                if v > 0.02 {
                                    // Green at the leading edge, drifting to violet below
                                    let hue = (hue_top + fy * 0.4).min(0.95);
                                    target.buffer[y as usize * WIDTH + x] = hsv_to_rgb(hue, 0.95, v);
                                }
                            }
                            y += 1;
                        }
                    }
                }
            }
            AnimationEnum::Starfield => {
                let t = ms_since_start as f32 / 1000.0;

                for i in 0..70u32 {
                    let h = fnv(i, 13);
                    let x = (h % WIDTH as u32) as usize;
                    let y = ((h >> 8) % HEIGHT as u32) as usize;
                    let speed = 0.6 + ((h >> 4) % 10) as f32 * 0.15;
                    let phase = (h >> 12) as f32 * 0.05;
                    let tw = 0.25 + 0.75 * ((sinp(t * speed + phase)) * 0.5 + 0.5);
                    let b = (tw * tw * 255.0) as u8;
                    let r = (b as f32 * 0.85) as u8;
                    let g = (b as f32 * 0.95) as u8;
                    let idx = y * WIDTH + x;
                    target.buffer[idx] = Rgb888::new(r, g, b);
                    if tw > 0.92 {
                        // Bright stars get a little sheen
                        if x + 1 < WIDTH {
                            target.buffer[idx + 1] = Rgb888::new(r / 2, g / 2, b / 2);
                        }
                        if y + 1 < HEIGHT {
                            target.buffer[idx + WIDTH + 1] = Rgb888::new(r / 2, g / 2, b / 2);
                        }
                    }
                }

                // Occasional shooting star
                let phase = t % 5.5;
                if phase < 1.0 {
                    let p = phase;
                    let hx = -4.0 + p * 68.0;
                    let hy = 3.0 + p * 13.0;
                    for k in 0..6 {
                        let x = (hx - k as f32 * 1.6) as i32;
                        let y = (hy - k as f32 * 0.3) as i32;
                        if x >= 0 && x < WIDTH as i32 && y >= 0 && y < HEIGHT as i32 {
                            let f = (200 - k * 32).max(0) as u8;
                            target.buffer[y as usize * WIDTH + x as usize] = Rgb888::new(f, f, 255);
                        }
                    }
                }
            }
            AnimationEnum::Snowfall => {
                let t = ms_since_start as f32 / 1000.0;

                for i in 0..60u32 {
                    let h = fnv(i, 29);
                    let depth = ((h >> 4) % 10) as f32 / 9.0; // 0 = far, 1 = near
                    let speed = 6.0 + depth * 14.0;
                    let sway = 3.0 + depth * 7.0;
                    let wob = 0.4 + ((h >> 8) % 12) as f32 * 0.12;

                    let yf = (((h % HEIGHT as u32) as f32) + t * speed) % (HEIGHT as f32 + 2.0);
                    let xf = ((h >> 16) as f32 / 65536.0 * WIDTH as f32)
                        + t * (2.0 + depth * 4.0)
                        + sinp(t * wob + i as f32) * sway;
                    let x = xf.rem_euclid(WIDTH as f32) as usize;
                    let y = yf as usize;

                    if y < HEIGHT {
                        let v = (150.0 + depth * 105.0) as u8;
                        target.buffer[y * WIDTH + x] =
                            Rgb888::new((v as u16 * 92 / 100) as u8, (v as u16 * 97 / 100) as u8, 255);
                        if depth > 0.75 {
                            // Near flakes catch the light
                            if x + 1 < WIDTH {
                                target.buffer[y * WIDTH + x + 1] = Rgb888::new(v / 2, v / 2, 200);
                            }
                            if y + 1 < HEIGHT {
                                target.buffer[(y + 1) * WIDTH + x] = Rgb888::new(v / 2, v / 2, 200);
                            }
                        }
                    }
                }
            }
            AnimationEnum::FallingLeaves => {
                let t = ms_since_start as f32 / 1000.0;
                const PALETTE: [(u8, u8, u8); 5] = [
                    (224, 96, 24),  // autumn orange
                    (190, 44, 32),  // red
                    (236, 178, 42), // gold
                    (150, 88, 30),  // brown
                    (246, 214, 96), // yellow
                ];

                for i in 0..18u32 {
                    let h = fnv(i, 41);
                    let (r, g, b) = PALETTE[(h % 5) as usize];
                    let depth = ((h >> 3) % 10) as f32 / 9.0;
                    let speed = 5.0 + depth * 9.0;
                    let sway = 5.0 + ((h >> 7) % 6) as f32;
                    let wob = 0.7 + ((h >> 11) % 8) as f32 * 0.1;
                    let spin = 1.2 + ((h >> 15) % 10) as f32 * 0.25;

                    let yf = (((h >> 2) % HEIGHT as u32) as f32 + t * speed) % (HEIGHT as f32 + 4.0);
                    let xf = ((h >> 16) as f32 / 65536.0 * (WIDTH + 20) as f32)
                        + sinp(t * wob + i as f32) * sway
                        + t * 1.5
                        - 10.0;
                    let x = xf.rem_euclid((WIDTH + 20) as f32) as i32 - 10;
                    let y = yf as i32 - 2;

                    // Tumbling leaf: the blob narrows and widens as it turns
                    let turn = sinp(t * spin + i as f32);
                    let size = 1 + ((turn.abs() * 1.8) as i32).min(2);
                    if x > -3 && x < WIDTH as i32 + 3 && y > -3 && y < HEIGHT as i32 {
                        let dim = 0.5 + 0.5 * turn.abs();
                        draw_circle_filled(
                            x,
                            y,
                            size,
                            Rgb888::new(
                                (r as f32 * dim) as u8,
                                (g as f32 * dim) as u8,
                                (b as f32 * dim) as u8,
                            ),
                            target,
                        );
                    }
                }
            }
            AnimationEnum::Fireflies => {
                let t = ms_since_start as f32 / 1000.0;

                // Woodland night: a deep green wash fading to black overhead
                for y in 0..HEIGHT {
                    let f = y as f32 / HEIGHT as f32;
                    for x in 0..WIDTH {
                        target.buffer[y * WIDTH + x] =
                            Rgb888::new((4.0 * f) as u8, (14.0 * f) as u8, (8.0 * f) as u8);
                    }
                }

                // A low winter moon
                draw_circle_filled(50, 5, 3, Rgb888::new(150, 150, 130), target);

                // Pine-silhouette tree line
                for tx in (0..WIDTH as i32).step_by(9) {
                    let h = 6 + ((tx * 7) % 5);
                    draw_triangle_filled(tx, 40, tx + 6, 40, tx + 3, 40 - h, Rgb888::new(3, 9, 5), target);
                }

                // Drifting, blinking fireflies
                for i in 0..14u32 {
                    let h = fnv(i, 57);
                    let fx = 8.0 + (h % 44) as f32;
                    let fy = 6.0 + ((h >> 6) % 22) as f32;
                    let sx = 0.3 + ((h >> 3) % 10) as f32 * 0.05;
                    let sy = 0.2 + ((h >> 9) % 10) as f32 * 0.05;
                    let pulse = 0.6 + ((h >> 13) % 12) as f32 * 0.15;

                    let x = (fx + sinp(t * sx + i as f32) * 7.0 + sinp(t * 0.11 * (1.0 + i as f32 * 0.01)) * 4.0) as i32;
                    let y = (fy + sinp(t * sy + i as f32 * 0.7) * 5.0) as i32;
                    let glow = sinp(t * pulse + i as f32 * 1.7) * 0.5 + 0.5;
                    let glow = glow * glow * glow * glow; // mostly dark, flaring bright

                    if glow > 0.04 && x >= 0 && x < WIDTH as i32 && y >= 0 && y < HEIGHT as i32 {
                        let v = (glow * 255.0) as u8;
                        draw_circle_filled(
                            x,
                            y,
                            2,
                            Rgb888::new((v as u16 * 45 / 100) as u8, v, (v as u16 * 35 / 100) as u8),
                            target,
                        );
                        target.buffer[y as usize * WIDTH + x as usize] =
                            Rgb888::new(v, v, (v as u16 * 60 / 100) as u8);
                    }
                }
            }
            AnimationEnum::VinylSpin => {
                let t = ms_since_start as f32 / 1000.0;
                let cx = 30.0;
                let cy = 20.0;
                let r_outer = 18.0;
                let rot = t * 3.0; // spin rate, rad/s

                for y in 0..HEIGHT {
                    for x in 0..WIDTH {
                        let dx = x as f32 - cx;
                        let dy = y as f32 - cy;
                        let dist = (dx * dx + dy * dy).sqrt();
                        if dist > r_outer {
                            continue;
                        }

                        let angle = dy.atan2(dx);
                        let mut v = 22.0 + 10.0 * sinp(dist * 2.5); // groove shimmer
                        let gloss = (cosp(angle - rot) * 0.5 + 0.5).powi(4) * 38.0; // rotating sheen
                        v += gloss;
                        if dist > 16.5 {
                            v *= (r_outer - dist) * 2.0; // soften the rim
                        }

                        let mut color = if dist < 6.0 {
                            if dist < 0.9 {
                                Rgb888::new(0, 0, 0) // spindle hole
                            } else {
                                let mut lv = 150.0 + 60.0 * ((cosp(angle - rot) * 0.5 + 0.5).max(0.0));
                                if dist > 5.2 {
                                    lv *= 0.6;
                                }
                                Rgb888::new(lv as u8, 26, 34)
                            }
                        } else {
                            Rgb888::new((v * 0.75) as u8, (v * 0.75) as u8, (v * 0.85) as u8)
                        };

                        // Highlight dot on the label so the spin reads
                        if dist > 2.5 && dist < 4.5 && (angle - rot).rem_euclid(2.0 * PI) < 0.45 {
                            color = Rgb888::new(230, 230, 200);
                        }

                        target.buffer[y * WIDTH + x] = color;
                    }
                }
            }
            AnimationEnum::CanalBoats => {
                let t = ms_since_start as f32 / 1000.0;
                let water_top = 20usize;

                // Dusk sky
                for y in 0..water_top {
                    let f = y as f32 / water_top as f32;
                    for x in 0..WIDTH {
                        target.buffer[y * WIDTH + x] = Rgb888::new(
                            (10.0 + 26.0 * f) as u8,
                            (10.0 + 22.0 * f) as u8,
                            (30.0 + 28.0 * f) as u8,
                        );
                    }
                }

                // Early-evening stars
                for (sx, sy, sv) in [(8usize, 3usize, 140u8), (17, 6, 95), (44, 2, 110), (52, 8, 80)] {
                    target.buffer[sy * WIDTH + sx] = Rgb888::new(sv, sv, 255);
                }

                // Low sun by the horizon
                draw_circle_filled(49, 18, 2, Rgb888::new(160, 110, 50), target);

                // Old mill on the far bank: body, chimney and a slowly turning wheel
                draw_rect_filled(41, 9, 9, 11, Rgb888::new(8, 8, 12), target);
                draw_rect_filled(45, 5, 2, 4, Rgb888::new(8, 8, 12), target);
                draw_circle_filled(41, 13, 4, Rgb888::new(6, 6, 10), target);
                draw_circle(41, 13, 4, Rgb888::new(34, 30, 44), target);
                draw_circle(41, 13, 2, Rgb888::new(34, 30, 44), target);
                let wrot = t % (2.0 * PI);
                for k in 0..4 {
                    let a = wrot + k as f32 * (PI / 2.0);
                    draw_line(
                        41,
                        13,
                        (41.0 + 4.0 * a.cos()) as i32,
                        (13.0 + 4.0 * a.sin()) as i32,
                        Rgb888::new(34, 30, 44),
                        target,
                    );
                }

                // Water with drifting ripples
                for y in water_top..HEIGHT {
                    let f = (y - water_top) as f32 / (HEIGHT - water_top) as f32;
                    for x in 0..WIDTH {
                        let ripple = 0.5 + 0.5 * sinp(x as f32 * 0.35 + t * 1.4 + y as f32 * (0.8 + 0.4 * f));
                        target.buffer[y * WIDTH + x] = Rgb888::new(
                            (8.0 + 8.0 * ripple) as u8,
                            (22.0 + 10.0 * f + 8.0 * ripple) as u8,
                            (46.0 + 18.0 * f + 16.0 * ripple) as u8,
                        );
                    }
                }

                // Sun glinting on the water below the low sun
                for y in (water_top + 1)..HEIGHT {
                    let f = (y - water_top) as f32 / (HEIGHT - water_top) as f32;
                    for dx in -3i32..=3 {
                        let x = 49 + dx;
                        if x < 0 || x >= WIDTH as i32 {
                            continue;
                        }
                        let lx = (x as f32 + sinp(y as f32 * 1.6 + t * 3.0) * 2.0) as i32;
                        if lx < 0 || lx >= WIDTH as i32 {
                            continue;
                        }
                        let v = (1.0 - f) * (0.45 + 0.55 * sinp(x as f32 * 2.1 + t * 5.0));
                        if v <= 0.1 {
                            continue;
                        }
                        let idx = y * WIDTH + lx as usize;
                        let base = target.buffer[idx];
                        target.buffer[idx] = Rgb888::new(
                            (base.r() as f32 + 210.0 * v).min(255.0) as u8,
                            (base.g() as f32 + 150.0 * v).min(255.0) as u8,
                            (base.b() as f32 + 80.0 * v).min(255.0) as u8,
                        );
                    }
                }

                // A narrowboat crossing the cut, with its reflection
                const CYCLE: f32 = 18.0;
                let p = t % CYCLE;
                let bx = -16.0 + (p / CYCLE) * (WIDTH as f32 + 32.0);
                let bi = bx as i32;

                draw_rect_filled(bi, 17, 14, 2, Rgb888::new(58, 24, 20), target); // hull
                draw_rect_filled(bi, 18, 14, 1, Rgb888::new(200, 60, 40), target); // stripe
                draw_rect_filled(bi + 10, 14, 1, 3, Rgb888::new(30, 30, 34), target); // chimney
                draw_rect_filled(bi + 1, 14, 4, 3, Rgb888::new(60, 40, 90), target); // cabin
                if bi + 2 >= 0 && bi + 2 < WIDTH as i32 {
                    target.buffer[15 * WIDTH + (bi + 2) as usize] = Rgb888::new(255, 200, 90); // window
                }

                // Wake trailing behind the boat
                for (k, wx) in [(2i32, 120i32), (5, 70)] {
                    let x = bi - k;
                    if x >= 0 && x < WIDTH as i32 {
                        target.buffer[19 * WIDTH + x as usize] =
                            Rgb888::new(30, (wx as f32 * 0.9) as u8, wx as u8);
                    }
                }

                draw_rect_filled(bi + 1, 20, 12, 3, Rgb888::new(12, 10, 26), target); // reflection
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
                draw_circle_filled(30, 20, (20 * params.beat as i32) / 255, Rgb888::WHITE, target);

                for i in 0..10 {
                    let x_offset = (ms_since_start >> (4 + i)) + (i * 13) + ((params.beat as u32) >> 4);
                    let y_offset = (ms_since_start >> (3 + i)) + (i * 7);
                    let x = x_offset as i32 % WIDTH as i32;
                    let y = y_offset as i32 % HEIGHT as i32;
                    let radius = 3 + (i % 4);
                    let r = ((i * 170) % 256) as u8;
                    let g = ((i * 310) % 256) as u8;
                    let b = ((i * 430) % 256) as u8;

                    draw_circle_filled(x, y, radius as i32, Rgb888::new(r, g, b), target);
                }
            }
            AnimationEnum::Fireworks => {
                // Decay old particles
                let mut new_particles = alloc::vec::Vec::new();

                // Spawn new particles on beat
                if params.beat > 200 {
                    let burst_count = (params.beat / 80) as usize;
                    for _ in 0..burst_count {
                        let x = (WIDTH as i32 / 2) + ((ms_since_start as i32 >> 4) % 20) - 10;
                        let y = (HEIGHT as i32 / 2) + ((ms_since_start as i32 >> 3) % 15) - 7;
                        let vx = (ms_since_start % 200 - 100) as f32 / 50.0;
                        let vy = (ms_since_start % 150 - 75) as f32 / 50.0;
                        let r = ((ms_since_start + 100 * 17) % 256) as u8;
                        let g = ((ms_since_start + 200 * 31) % 256) as u8;
                        let b = ((ms_since_start + 300 * 43) % 256) as u8;
                        let color = Rgb888::new(r, g, b);
                        let life = 20 + (ms_since_start % 30) as u32;

                        new_particles.push((x, y, vx, vy, color, life));
                    }
                }

                // Update and draw existing particles
                for &(x, y, vx, vy, color, life) in &new_particles {
                    let px = (x as f32 + vx * (life as f32 * 0.1)) as i32;
                    let py = (y as f32 + vy * (life as f32 * 0.1)) as i32;
                    let radius = (life as i32) / 5;
                    if radius > 0 && px >= 0 && px < WIDTH as i32 && py >= 0 && py < HEIGHT as i32 {
                        draw_circle_filled(px, py, radius, color, target);
                    }
                }

                // Draw a subtle trailing glow
                let glow_time = ms_since_start % 200;
                let glow_radius = (glow_time as i32) / 4;
                if glow_radius > 0 {
                    let cx = WIDTH as i32 / 2;
                    let cy = HEIGHT as i32 / 2;
                    let glow_color = Rgb888::new(255, 255, 255);
                    draw_circle_filled(cx, cy, glow_radius, glow_color, target);
                }
            }
            AnimationEnum::SynthWave => {
                const WIDTH: usize = 60;
                const HEIGHT: usize = 40;

                let beat_phase = (params.beat as f32 / 255.0) * 3.14159 * 2.0; // 0–2π
                let time = ms_since_start as i32 / 20; // 50fps

                // Draw 8 horizontal "wave" lines
                for line in 0..8 {
                    let y = (line * 5) as i32; // spaced vertically
                    let freq = 0.4 + (line as f32) * 0.05;
                    let amp = 2.0 + (line as f32) * 0.3;
                    let phase = (time as f32 * freq + beat_phase) % (2.0 * 3.14159);

                    // Draw wave as a series of dots
                    for x in 0..WIDTH as i32 {
                        let x_norm = x as f32 * 0.1;
                        let wave = (f32::sin(x_norm * freq + phase) * amp + amp) as i32;
                        let py = y + wave;
                        if py >= 0 && py < HEIGHT as i32 {
                            let hue = ((x + line + time) % 256) as i32;
                            let r = ((hue + 40) % 256) as i32;
                            let g = ((hue + 180) % 256) as i32;
                            let b = ((hue + 80) % 256) as i32;
                            let brightness = (255 * (params.beat as i32)) / 255;
                            let color = Rgb888::new(
                                (r * brightness / 255) as u8,
                                (g * brightness / 255) as u8,
                                (b * brightness / 255) as u8,
                            );
                            target.buffer[py as usize * WIDTH + x as usize] = color;
                        }
                    }
                }

                // Draw vertical "bass" bar on beat
                if params.beat > 180 {
                    let bar_height = (params.beat as i32 / 4) + 10;
                    let bar_x = (ms_since_start as i32 / 10) % WIDTH as i32;
                    let bar_y = HEIGHT as i32 - bar_height;
                    let r = 255;
                    let g = 50;
                    let b = 200;
                    for y in bar_y..HEIGHT as i32 {
                        if y >= 0 {
                            target.buffer[y as usize * WIDTH + bar_x as usize] = Rgb888::new(r, g, b);
                        }
                    }
                }
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
                    draw_line(
                        x0,
                        y0,
                        x1,
                        y1,
                        Rgb888::new(params.beat, params.beat, params.beat),
                        target,
                    );
                }
            }
            AnimationEnum::Ship => {
                // Original wedge-fighter wireframe renderer
                // Same rotation / perspective-projection / Bresenham-line approach as the
                // cube example, applied to a ~26-vertex ship silhouette:
                // wedge fuselage, swept wings, twin tail fins, rear engine block.
                //
                // This is an original design (not a reproduction of any specific
                // copyrighted spacecraft) — general "wedge fighter" shape only.

                // Clear buffer to black
                for pixel in target.buffer.iter_mut() {
                    *pixel = Rgb888::new(0x00, 0x00, 0x00);
                }

                // --- Vertices (26 points, normalized roughly to [-1, 1]) ---
                //
                // Layout (looking down +Z, nose pointing toward -Z / "front"):
                //   0-3   : nose tip + fuselage front cross-section
                //   4-7   : mid-fuselage cross-section (widest point, wing root height)
                //   8-11  : rear fuselage cross-section (before engine block)
                //   12-13 : left wing tip (top/bottom thin edge)
                //   14-15 : right wing tip (top/bottom thin edge)
                //   16-17 : left tail fin tip (top, and root-back)
                //   18-19 : right tail fin tip (top, and root-back)
                //   20-23 : engine block rear face (4 corners)
                //   24-25 : twin engine nozzle centers (drawn as small crosses via extra edges)
                let vertices = [
                    // 0-3: nose (single point, front cross doesn't need 4 distinct pts, but
                    // we keep a tiny diamond so the nose has thickness)
                    [0.0, 0.0, -1.6],   // 0: nose tip
                    [0.15, 0.0, -1.3],  // 1: nose right
                    [-0.15, 0.0, -1.3], // 2: nose left
                    [0.0, 0.15, -1.3],  // 3: nose top (slight)
                    // 4-7: mid-fuselage cross section (wing root level)
                    [0.35, 0.1, -0.2],  // 4: mid right
                    [-0.35, 0.1, -0.2], // 5: mid left
                    [0.0, 0.3, -0.2],   // 6: mid top
                    [0.0, -0.15, -0.2], // 7: mid bottom (keel)
                    // 8-11: rear fuselage cross section
                    [0.3, 0.15, 0.8],  // 8: rear right
                    [-0.3, 0.15, 0.8], // 9: rear left
                    [0.0, 0.35, 0.8],  // 10: rear top
                    [0.0, -0.1, 0.8],  // 11: rear bottom
                    // 12-15: wing tips (flat, swept back)
                    [1.4, 0.05, 0.5],   // 12: right wingtip front-ish
                    [1.3, 0.05, 0.95],  // 13: right wingtip rear-ish
                    [-1.4, 0.05, 0.5],  // 14: left wingtip front-ish
                    [-1.3, 0.05, 0.95], // 15: left wingtip rear-ish
                    // 16-19: twin tail fins (angled outward + up from rear fuselage)
                    [0.55, 0.9, 0.95],  // 16: right fin tip
                    [0.4, 0.2, 1.05],   // 17: right fin rear-root
                    [-0.55, 0.9, 0.95], // 18: left fin tip
                    [-0.4, 0.2, 1.05],  // 19: left fin rear-root
                    // 20-23: engine block rear face (behind rear fuselage cross-section)
                    [0.35, 0.3, 1.2],   // 20: engine top-right
                    [-0.35, 0.3, 1.2],  // 21: engine top-left
                    [0.35, -0.1, 1.2],  // 22: engine bottom-right
                    [-0.35, -0.1, 1.2], // 23: engine bottom-left
                    // 24-25: nozzle centers (small offset pair used for a "+" cross marker)
                    [0.35, 0.1, 1.3],  // 24: right nozzle center
                    [-0.35, 0.1, 1.3], // 25: left nozzle center
                ];

                // --- Edges ---
                let edges = [
                    // nose to nose-cross ring
                    (0, 1),
                    (0, 2),
                    (0, 3),
                    (1, 3),
                    (2, 3),
                    (1, 7),
                    (2, 7), // nose underside blends to keel
                    // nose ring to mid ring (fuselage front taper)
                    (1, 4),
                    (2, 5),
                    (3, 6),
                    // mid cross ring
                    (4, 6),
                    (4, 7),
                    (5, 6),
                    (5, 7),
                    // mid ring to rear ring (fuselage rear taper)
                    (4, 8),
                    (5, 9),
                    (6, 10),
                    (7, 11),
                    // rear cross ring
                    (8, 10),
                    (8, 11),
                    (9, 10),
                    (9, 11),
                    // wings: root (mid ring sides) to tips, swept back to rear ring
                    (4, 12),
                    (12, 13),
                    (13, 8),
                    (5, 14),
                    (14, 15),
                    (15, 9),
                    // wing tip leading/trailing thin edge (gives the flat blade some form)
                    (4, 13),
                    (5, 15),
                    // tail fins: from rear ring up to tip, and tip back down to fin root
                    (10, 16),
                    (16, 17),
                    (17, 8),
                    (10, 18),
                    (18, 19),
                    (19, 9),
                    // rear ring to engine block front
                    (8, 20),
                    (9, 21),
                    (10, 20),
                    (10, 21),
                    (11, 22),
                    (11, 23),
                    (8, 22),
                    (9, 23),
                    // engine block rear face
                    (20, 21),
                    (20, 22),
                    (21, 23),
                    (22, 23),
                    // nozzle cross markers
                    (24, 20),
                    (24, 22),
                    (25, 21),
                    (25, 23),
                ];

                // --- Rotation angles based on time (radians) ---
                let t = ms_since_start as f32 * 0.0008; // ~0.8 rad/s total rotation

                let cos_x = (t * 0.7).cos();
                let sin_x = (t * 0.7).sin();
                let cos_y = (t * 0.2).cos();
                let sin_y = (t * 0.2).sin();
                let cos_z = t.cos();
                let sin_z = t.sin();

                // --- Projected vertices (2D screen space) ---
                let mut screen_coords = [(0, 0); 26];
                let mut visible = [true; 26];

                let center_x = WIDTH as f32 / 2.0;
                let center_y = HEIGHT as f32 / 2.0;
                let scale = (WIDTH.min(HEIGHT) as f32) * 1.5; // ship spans wider than the cube did

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
                    let perspective = 4.0 + z; // depth factor — avoid division by zero
                    if perspective <= 0.0 {
                        visible[i] = false;
                        continue;
                    }

                    let proj_x = x * scale / perspective;
                    let proj_y = y * scale / perspective;

                    screen_coords[i] = ((center_x + proj_x) as i32, (center_y + proj_y) as i32);
                }

                // --- Draw edges ---
                for &(a, b) in &edges {
                    if !visible[a] || !visible[b] {
                        continue;
                    }

                    let (x0, y0) = screen_coords[a];
                    let (x1, y1) = screen_coords[b];

                    // // Skip if either point is off-screen
                    // if x0 < 0
                    //     || x0 >= WIDTH as i32
                    //     || y0 < 0
                    //     || y0 >= HEIGHT as i32
                    //     || x1 < 0
                    //     || x1 >= WIDTH as i32
                    //     || y1 < 0
                    //     || y1 >= HEIGHT as i32
                    // {
                    //     continue;
                    // }

                    // Bresenham's line algorithm for smooth wireframe
                    draw_line(
                        x0,
                        y0,
                        x1,
                        y1,
                        Rgb888::new(params.beat, params.beat, params.beat),
                        target,
                    );
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
                for y in 0..1 {
                    for x in 0..WIDTH {
                        target.buffer[y * WIDTH + x] = Rgb888::new(params.beat, params.beat, params.beat);
                    }
                }
            }
            AnimationEnum::BarChart => {
                const WIDTH: usize = 60;
                const HEIGHT: usize = 40;

                let beat = params.beat as i32 * HEIGHT as i32 / 256;
                let low = params.low as i32 * HEIGHT as i32 / 256;
                let med = params.med as i32 * HEIGHT as i32 / 256;
                let high = params.high as i32 * HEIGHT as i32 / 256;

                draw_rect_filled(0, HEIGHT as i32 - beat, WIDTH as i32 / 4, beat, Rgb888::WHITE, target);
                draw_rect_filled(15, HEIGHT as i32 - low, WIDTH as i32 / 4, low, Rgb888::RED, target);
                draw_rect_filled(30, HEIGHT as i32 - med, WIDTH as i32 / 4, med, Rgb888::GREEN, target);
                draw_rect_filled(45, HEIGHT as i32 - high, WIDTH as i32 / 4, high, Rgb888::BLUE, target);
            }
            AnimationEnum::LineChart => {
                let beat = params.beat as i32 * WIDTH as i32 / 256 / 4;
                let low = params.low as i32 * WIDTH as i32 / 256 / 4;
                let med = params.med as i32 * WIDTH as i32 / 256 / 4;
                let high = params.high as i32 * WIDTH as i32 / 256 / 4;

                draw_rect_filled(0, 0, beat, 1, Rgb888::WHITE, target);
                draw_rect_filled(15, 0, low, 1, Rgb888::RED, target);
                draw_rect_filled(30, 0, med, 1, Rgb888::GREEN, target);
                draw_rect_filled(45, 0, high, 1, Rgb888::BLUE, target);
            }
            AnimationEnum::EchoOfTheSoul => {
                const WIDTH: usize = 60;
                const HEIGHT: usize = 40;

                let beat = params.beat as f32 / 255.0; // 0.0 → 1.0
                let low = params.low as f32 / 255.0;
                let med = params.med as f32 / 255.0;
                let high = params.high as f32 / 255.0;

                let time = ms_since_start as f32 / 1000.0; // seconds

                // Core "soul" — a glowing, breathing orb at center
                let core_radius = (12.0 + 8.0 * beat) as i32;
                let core_alpha = 0.3 + 0.7 * beat;
                let hue = (time * 20.0 + low * 180.0) % 360.0;
                let r = ((hue + 0.0) % 360.0 / 360.0 * 255.0) as u8;
                let g = ((hue + 120.0) % 360.0 / 360.0 * 255.0) as u8;
                let b = ((hue + 240.0) % 360.0 / 360.0 * 255.0) as u8;
                let color = Rgb888::new(r, g, b);
                draw_circle_filled(WIDTH as i32 / 2, HEIGHT as i32 / 2, core_radius, color, target);

                // Inner glow — subtle halo
                let halo_radius = core_radius + 2;
                let glow_color = Rgb888::new(255, 255, 255);
                draw_circle_filled(WIDTH as i32 / 2, HEIGHT as i32 / 2, halo_radius, glow_color, target);

                // "Thought waves" — ripples that expand with bass
                for i in 0..3 {
                    let phase = (time * 0.8 + i as f32 * 1.2) % 6.28;
                    let radius = (15.0 + low * 20.0 + i as f32 * 8.0) as i32;
                    let intensity = (0.2 + low * 0.8) * (1.0 + f32::sin(phase + time * 2.0)) / 2.0;
                    let alpha = intensity * core_alpha;

                    if alpha > 0.05 {
                        let r = (r as f32 * alpha) as u8;
                        let g = (g as f32 * alpha) as u8;
                        let b = (b as f32 * alpha) as u8;
                        draw_circle_filled(
                            WIDTH as i32 / 2,
                            HEIGHT as i32 / 2,
                            radius,
                            Rgb888::new(r, g, b),
                            target,
                        );
                    }
                }

                // "Emotional sparks" — particles that fly out on high frequencies
                for i in 0..(high * 15.0) as usize {
                    let angle = time * 3.0 + i as f32 * 0.3;
                    let dist = 5.0 + high * 15.0;
                    let x = (WIDTH as i32 / 2 + (dist * f32::cos(angle)) as i32) % WIDTH as i32;
                    let y = (HEIGHT as i32 / 2 + (dist * f32::sin(angle)) as i32) % HEIGHT as i32;
                    let life = (ms_since_start % 100) as u32;
                    let size = 1 + (life / 25) as i32;
                    let r = (255.0 * high) as u8;
                    let g = (120.0 * high) as u8;
                    let b = (255.0 * med) as u8;
                    draw_circle_filled(x, y, size, Rgb888::new(r, g, b), target);
                }
            }
            AnimationEnum::HarmonicSpiral => {
                const WIDTH: usize = 60;
                const HEIGHT: usize = 40;

                let beat = params.beat as f32 / 255.0;
                let low = params.low as f32 / 255.0;
                let med = params.med as f32 / 255.0;
                let high = params.high as f32 / 255.0;

                let time = ms_since_start as f32 / 500.0; // slow rotation

                // Draw 3 spiraling filaments — each a different frequency
                for band in 0..3 {
                    let freq = [low, med, high][band];
                    let color = match band {
                        0 => Rgb888::new(255, 40, 120), // Red-Pink (bass)
                        1 => Rgb888::new(100, 255, 80), // Green (mid)
                        2 => Rgb888::new(60, 180, 255), // Blue (treble)
                        _ => Rgb888::WHITE,
                    };

                    let spiral_radius = 12.0 + freq * 20.0;
                    let turns = 1.5 + freq * 3.0;
                    let density = 12 + (freq * 20.0) as usize;

                    for i in 0..density {
                        let angle = time * 2.0 + (i as f32 / density as f32) * 6.28 * turns;
                        let r = spiral_radius * (1.0 + f32::sin(i as f32 * 0.3 + time * 1.5) * 0.3);
                        let x = (WIDTH as i32 / 2 + (r * f32::cos(angle)) as i32) % WIDTH as i32;
                        let y = (HEIGHT as i32 / 2 + (r * f32::sin(angle)) as i32) % HEIGHT as i32;

                        let brightness = (freq * (1.0 + f32::sin(time * 4.0 + i as f32 * 0.1))) * 255.0;
                        let r = (color.r() as f32 * brightness / 255.0) as u8;
                        let g = (color.g() as f32 * brightness / 255.0) as u8;
                        let b = (color.b() as f32 * brightness / 255.0) as u8;

                        target.buffer[(y as usize * WIDTH + x as usize).min(TOTAL_PIXELS - 1)] = Rgb888::new(r, g, b);
                    }
                }

                // Center “core” — pulses with beat
                let core_size = (4.0 + beat * 8.0) as i32;
                let pulse = (f32::sin(time * 8.0) * 0.5 + 0.5) * beat;
                let hue = (time * 40.0) % 360.0;
                let r = ((hue + 0.0) % 360.0 / 360.0 * 255.0) as u8;
                let g = ((hue + 120.0) % 360.0 / 360.0 * 255.0) as u8;
                let b = ((hue + 240.0) % 360.0 / 360.0 * 255.0) as u8;

                draw_circle_filled(
                    WIDTH as i32 / 2,
                    HEIGHT as i32 / 2,
                    core_size,
                    Rgb888::new(r, g, b),
                    target,
                );
            }
            AnimationEnum::OctopusOfSound => {
                const WIDTH: usize = 60;
                const HEIGHT: usize = 40;

                let beat = params.beat as f32 / 255.0;
                let low = params.low as f32 / 255.0;
                let med = params.med as f32 / 255.0;
                let high = params.high as f32 / 255.0;

                let time = ms_since_start as f32 / 200.0;

                // Draw 6 tentacles — each a sine wave of light
                for t in 0..6 {
                    let base_x = (t * 10 + (time * 15.0) as i32) % WIDTH as i32;
                    let length = (low * 30.0) as i32 + 5;
                    let curl = f32::sin(time * 3.0 + t as f32 * 0.8) * 0.5;

                    for i in 0..length {
                        let y = HEIGHT as i32 - i - 1;
                        if y < 0 {
                            break;
                        }

                        let x_offset = (curl * (i as f32 / length as f32) * 10.0) as i32;
                        let x = base_x + x_offset;

                        if x >= 0 && x < WIDTH as i32 {
                            let hue = (t as f32 * 60.0 + time * 100.0) % 360.0;
                            let r = ((hue + 0.0) % 360.0 / 360.0 * 255.0) as u8;
                            let g = ((hue + 120.0) % 360.0 / 360.0 * 255.0) as u8;
                            let b = ((hue + 240.0) % 360.0 / 360.0 * 255.0) as u8;

                            let intensity = (1.0 - (i as f32 / length as f32)) * (0.5 + med * 0.5);
                            let r = (r as f32 * intensity) as u8;
                            let g = (g as f32 * intensity) as u8;
                            let b = (b as f32 * intensity) as u8;

                            target.buffer[y as usize * WIDTH + x as usize] = Rgb888::new(r, g, b);
                        }
                    }

                    // Tentacle tip — glowing orb
                    let tip_y = HEIGHT as i32 - length;
                    if tip_y >= 0 {
                        let tip_x = base_x + (curl * 10.0) as i32;
                        let size = (2.0 + high * 4.0) as i32;
                        let color = Rgb888::new(255, 255, 255);
                        draw_circle_filled(tip_x, tip_y, size, color, target);
                    }
                }

                // "Bubbles" rising from the base
                for i in 0..(low * 10.0) as i32 {
                    let x = (ms_since_start as i32 >> (i + 2)) % WIDTH as i32;
                    let y = (HEIGHT as i32 / 2 + (i as i32 * 2)) % HEIGHT as i32;
                    let size = 1 + (i % 3);
                    let r = (255.0 * low) as u8;
                    let g = (200.0 * low) as u8;
                    let b = (255.0 * high) as u8;
                    draw_circle_filled(x as i32, y, size, Rgb888::new(r, g, b), target);
                }
            }
            AnimationEnum::MagneticField => {
                const WIDTH: usize = 60;
                const HEIGHT: usize = 40;

                let low = params.low as f32 / 255.0;
                let med = params.med as f32 / 255.0;
                let high = params.high as f32 / 255.0;

                let time = ms_since_start as f32 / 300.0;

                // Draw 3 "magnetic poles"
                let pole1_x = (WIDTH as i32 / 4) + (time * 20.0) as i32 % (WIDTH as i32 / 2);
                let pole2_x = (WIDTH as i32 * 3 / 4) - (time * 20.0) as i32 % (WIDTH as i32 / 2);
                let pole_y = HEIGHT as i32 / 2;

                // Draw field lines between poles
                for y in 0..HEIGHT {
                    for x in 0..WIDTH {
                        let dx1 = (x as f32 - pole1_x as f32);
                        let dy1 = (y as f32 - pole_y as f32);
                        let dx2 = (x as f32 - pole2_x as f32);
                        let dy2 = (y as f32 - pole_y as f32);

                        let dist1 = (dx1 * dx1 + dy1 * dy1).sqrt();
                        let dist2 = (dx2 * dx2 + dy2 * dy2).sqrt();

                        if dist1 > 0.1 && dist2 > 0.1 {
                            let field_x = dx1 / dist1 * low + dx2 / dist2 * med;
                            let field_y = dy1 / dist1 * low + dy2 / dist2 * med;
                            let field_mag = (field_x * field_x + field_y * field_y).sqrt();

                            if field_mag > 0.05 {
                                let angle = f32::atan2(field_y, field_x) + time * 0.8;
                                let intensity = (field_mag * 2.0) * high;

                                let hue = (angle.to_degrees() + time * 100.0) % 360.0;
                                let r = ((hue + 0.0) % 360.0 / 360.0 * 255.0) as u8;
                                let g = ((hue + 120.0) % 360.0 / 360.0 * 255.0) as u8;
                                let b = ((hue + 240.0) % 360.0 / 360.0 * 255.0) as u8;

                                let color = Rgb888::new(
                                    (r as f32 * intensity) as u8,
                                    (g as f32 * intensity) as u8,
                                    (b as f32 * intensity) as u8,
                                );
                                target.buffer[y * WIDTH + x] = color;
                            }
                        }
                    }
                }

                // Particles attracted to poles
                for i in 0..(high * 15.0) as usize {
                    let x = (ms_since_start as i32 >> (i + 4)) % WIDTH as i32;
                    let y = (ms_since_start as i32 >> (i + 2)) % HEIGHT as i32;
                    let vx = (med * 0.5) as f32 * (f32::sin(time * 3.0 + i as f32 * 0.1) * 2.0);
                    let vy = (low * 0.8) as f32;

                    let px = (x as f32 + vx) as i32;
                    let py = (y as f32 + vy) as i32;

                    if px >= 0 && px < WIDTH as i32 && py >= 0 && py < HEIGHT as i32 {
                        let r = (255.0 * high) as u8;
                        let g = (180.0 * med) as u8;
                        let b = (255.0 * low) as u8;
                        draw_circle_filled(px, py, 1, Rgb888::new(r, g, b), target);
                    }
                }
            }
            AnimationEnum::SilentDancer => {
                const WIDTH: usize = 60;
                const HEIGHT: usize = 40;

                let beat = params.beat as f32 / 255.0;
                let low = params.low as f32 / 255.0;
                let med = params.med as f32 / 255.0;
                let high = params.high as f32 / 255.0;

                let time = ms_since_start as f32 / 100.0;

                // The dancer: one pixel that transforms
                let x = (WIDTH as i32 / 2 + (f32::sin(time * 1.5) * 10.0 + med * 20.0) as i32) % WIDTH as i32;
                let y = (HEIGHT as i32 / 2 + (f32::cos(time * 1.8) * 8.0 + low * 15.0) as i32) % HEIGHT as i32;

                // Shape: changes with beat
                let shape = (time * 3.0) as u32 % 4;
                let size = match shape {
                    0 => 1, // dot
                    1 => 2, // small circle
                    2 => 3, // medium
                    3 => 4, // large
                    _ => 1,
                };

                // Color: emotion mapping
                let hue = (time * 60.0 + low * 200.0) % 360.0;
                let r = ((hue + 0.0) % 360.0 / 360.0 * 255.0) as u8;
                let g = ((hue + 120.0) % 360.0 / 360.0 * 255.0) as u8;
                let b = ((hue + 240.0) % 360.0 / 360.0 * 255.0) as u8;

                // Pulse with beat
                let pulse = (f32::sin(time * 6.0) * 0.5 + 0.5) * beat;
                let r = (r as f32 * pulse) as u8;
                let g = (g as f32 * pulse) as u8;
                let b = (b as f32 * pulse) as u8;

                // Draw the dancer
                if size == 1 {
                    target.buffer[y as usize * WIDTH + x as usize] = Rgb888::new(r, g, b);
                } else {
                    draw_circle_filled(x, y, size, Rgb888::new(r, g, b), target);
                }

                // Leave a memory trail
                for i in 1..8 {
                    let age = i as f32;
                    let trail_x = (x as f32 + f32::sin(time * 1.2 - age * 0.3) * age * 2.0) as i32;
                    let trail_y = (y as f32 + f32::cos(time * 1.5 - age * 0.4) * age * 1.5) as i32;

                    if trail_x >= 0 && trail_x < WIDTH as i32 && trail_y >= 0 && trail_y < HEIGHT as i32 {
                        let alpha = 1.0 - (age / 8.0);
                        let r = (r as f32 * alpha) as u8;
                        let g = (g as f32 * alpha) as u8;
                        let b = (b as f32 * alpha) as u8;
                        target.buffer[trail_y as usize * WIDTH + trail_x as usize] = Rgb888::new(r, g, b);
                    }
                }
            }
        }
    }
}

impl AnimationEnum {
    /// Variant name — the stable "kind" identifier the UI keys its metadata
    /// (categories, preview renderer) off. Exhaustive on purpose: adding a
    /// variant must break here until it is named.
    pub fn kind_name(&self) -> &'static str {
        match self {
            AnimationEnum::Blank => "Blank",
            AnimationEnum::TwoLineMessage(..) => "TwoLineMessage",
            AnimationEnum::ScrollingMessage(..) => "ScrollingMessage",
            AnimationEnum::Circles => "Circles",
            AnimationEnum::Fireworks => "Fireworks",
            AnimationEnum::SynthWave => "SynthWave",
            AnimationEnum::Rainbows => "Rainbows",
            AnimationEnum::TestPattern => "TestPattern",
            AnimationEnum::RainbowWheel => "RainbowWheel",
            AnimationEnum::SpinningCube => "SpinningCube",
            AnimationEnum::Ship => "Ship",
            AnimationEnum::Columns => "Columns",
            AnimationEnum::Intensity => "Intensity",
            AnimationEnum::BarChart => "BarChart",
            AnimationEnum::LineChart => "LineChart",
            AnimationEnum::EchoOfTheSoul => "EchoOfTheSoul",
            AnimationEnum::HarmonicSpiral => "HarmonicSpiral",
            AnimationEnum::OctopusOfSound => "OctopusOfSound",
            AnimationEnum::MagneticField => "MagneticField",
            AnimationEnum::SilentDancer => "SilentDancer",
            AnimationEnum::CenteredTitle(..) => "CenteredTitle",
            AnimationEnum::StatusCard(..) => "StatusCard",
            AnimationEnum::Aurora => "Aurora",
            AnimationEnum::Starfield => "Starfield",
            AnimationEnum::Snowfall => "Snowfall",
            AnimationEnum::FallingLeaves => "FallingLeaves",
            AnimationEnum::Fireflies => "Fireflies",
            AnimationEnum::VinylSpin => "VinylSpin",
            AnimationEnum::CanalBoats => "CanalBoats",
        }
    }

    /// Human-readable title for "now playing" displays: text-carrying
    /// variants show their text, everything else its kind name.
    pub fn display_title(&self) -> String {
        match self {
            AnimationEnum::ScrollingMessage(msg) => msg.clone(),
            AnimationEnum::TwoLineMessage(a, b) => alloc::format!("{a} / {b}"),
            AnimationEnum::CenteredTitle(a, b) => alloc::format!("{a} {b}"),
            AnimationEnum::StatusCard(a, b) => alloc::format!("{a} {b}"),
            other => String::from(other.kind_name()),
        }
    }

    // Shared renderer for CenteredTitle/StatusCard: gold 6x12 font, up to
    // ~10 chars per line, centered — unlike 10x20 the section/status names
    // all fit on the 60px panel.
    fn draw_card(line1: &str, line2: &str, color: Rgb888, target: &mut BufferTargetRgb888) {
        let style = MonoTextStyleBuilder::new()
            .font(&FONT_6X12)
            .text_color(color)
            .build();

        for (line, y) in [(line1, 7i32), (line2, 21)] {
            let w = line.chars().count() as i32 * 6;
            let x = ((WIDTH as i32 - w) / 2).max(0);
            Text::with_text_style(
                line,
                Point::new(x, y),
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

// Helper: deterministic seed → pseudo-random u32 (FNV-style), for star/snow/leaf
// positions. Fixed seeds keep every animation frame sequence reproducible.
fn fnv(i: u32, salt: u32) -> u32 {
    i.wrapping_mul(1_677_761_993) ^ salt.wrapping_mul(2_246_822_519)
}

// Helpers: micromath's sin/cos are only accurate within -π..π, so fold the
// argument into that range first (time-based phases grow past π quickly).
fn sinp(a: f32) -> f32 {
    wrap_pi(a % (2.0 * PI)).sin()
}

fn cosp(a: f32) -> f32 {
    wrap_pi(a % (2.0 * PI)).cos()
}

fn wrap_pi(a: f32) -> f32 {
    let mut a = a;
    if a > PI {
        a -= 2.0 * PI;
    } else if a < -PI {
        a += 2.0 * PI;
    }
    a
}
