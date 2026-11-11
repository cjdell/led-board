//! Festival programme scenes — the lantern-parade animation set.
//!
//! Every scene is a pure function of elapsed time: the runner may rewind,
//! fade, or draw the same scene into two buffers on one frame, and all of
//! them still line up. Keeping them stateless is what lets the playlist
//! cross-fade and the web previews mirror the panel exactly.
//!
//! The art direction is deliberately bold: a saturated sky, one clear
//! silhouette, and a handful of animated details that carry the motion.
//! At 60x40 a legible silhouette beats detail every time.

use crate::animations::hsv_to_rgb;
use crate::common::{BufferTargetRgb888, HEIGHT, WIDTH};
use crate::draw::{draw_circle, draw_circle_filled, draw_line, draw_rect_filled, draw_triangle_filled};
use core::f32::consts::PI;
use embedded_graphics::pixelcolor::{Rgb888, RgbColor as _};
#[allow(unused_imports)]
use micromath::F32Ext;

const W: i32 = WIDTH as i32;
const H: i32 = HEIGHT as i32;

pub(crate) fn rgb(r: u8, g: u8, b: u8) -> Rgb888 {
    Rgb888::new(r, g, b)
}

#[inline]
pub(crate) fn put(x: i32, y: i32, c: Rgb888, out: &mut BufferTargetRgb888) {
    if x >= 0 && x < W && y >= 0 && y < H {
        out.buffer[y as usize * WIDTH + x as usize] = c;
    }
}

#[inline]
fn hline(x0: i32, x1: i32, y: i32, c: Rgb888, out: &mut BufferTargetRgb888) {
    if y < 0 || y >= H {
        return;
    }

    let lo = x0.max(0);
    let hi = x1.min(W - 1);

    if lo <= hi {
        let row = y as usize * WIDTH;

        for x in lo..=hi {
            out.buffer[row + x as usize] = c;
        }
    }
}

#[inline]
pub(crate) fn add(x: i32, y: i32, c: Rgb888, a: f32, out: &mut BufferTargetRgb888) {
    if x < 0 || x >= W || y < 0 || y >= H {
        return;
    }

    let p = &mut out.buffer[y as usize * WIDTH + x as usize];
    let (r, g, b) = (p.r() as f32, p.g() as f32, p.b() as f32);

    *p = Rgb888::new(
        (r + c.r() as f32 * a).min(255.0) as u8,
        (g + c.g() as f32 * a).min(255.0) as u8,
        (b + c.b() as f32 * a).min(255.0) as u8,
    );
}

#[inline]
pub(crate) fn over(x: i32, y: i32, c: Rgb888, a: f32, out: &mut BufferTargetRgb888) {
    if x < 0 || x >= W || y < 0 || y >= H {
        return;
    }

    let p = &mut out.buffer[y as usize * WIDTH + x as usize];
    let ia = 1.0 - a;

    *p = Rgb888::new(
        (p.r() as f32 * ia + c.r() as f32 * a) as u8,
        (p.g() as f32 * ia + c.g() as f32 * a) as u8,
        (p.b() as f32 * ia + c.b() as f32 * a) as u8,
    );
}

pub(crate) fn ellipse(cx: f32, cy: f32, rx: f32, ry: f32, c: Rgb888, out: &mut BufferTargetRgb888) {
    if rx <= 0.0 || ry <= 0.0 {
        return;
    }

    let y0 = (cy - ry).floor() as i32;
    let y1 = (cy + ry).ceil() as i32;

    for y in y0..=y1 {
        let dy = (y as f32 - cy) / ry;
        let inner = 1.0 - dy * dy;

        if inner <= 0.0 {
            continue;
        }

        let half = inner.sqrt() * rx;

        hline((cx - half).round() as i32, (cx + half).round() as i32, y, c, out);
    }
}

fn glow(cx: f32, cy: f32, radius: f32, c: Rgb888, strength: f32, out: &mut BufferTargetRgb888) {
    let x0 = (cx - radius).floor() as i32;
    let x1 = (cx + radius).ceil() as i32;
    let y0 = (cy - radius).floor() as i32;
    let y1 = (cy + radius).ceil() as i32;

    for y in y0..=y1 {
        for x in x0..=x1 {
            let dx = x as f32 - cx;
            let dy = y as f32 - cy;
            let d = (dx * dx + dy * dy).sqrt() / radius;

            if d < 1.0 {
                let f = 1.0 - d;
                add(x, y, c, f * f * strength, out);
            }
        }
    }
}

fn sky(top: Rgb888, bottom: Rgb888, out: &mut BufferTargetRgb888) {
    for y in 0..HEIGHT {
        let f = y as f32 / (HEIGHT - 1) as f32;
        let base = Rgb888::new(
            (top.r() as f32 + (bottom.r() as f32 - top.r() as f32) * f) as u8,
            (top.g() as f32 + (bottom.g() as f32 - top.g() as f32) * f) as u8,
            (top.b() as f32 + (bottom.b() as f32 - top.b() as f32) * f) as u8,
        );
        // One-count checker dither breaks up the visible gradient bands.
        let d0 = if (y & 1) == 0 { 0 } else { 1 };

        for x in 0..WIDTH {
            let d = d0 ^ ((x & 1) as u8);

            out.buffer[y * WIDTH + x] = Rgb888::new(
                base.r().saturating_add(d),
                base.g().saturating_add(d),
                base.b().saturating_add(d),
            );
        }
    }
}

fn stars(t: f32, seed: u32, count: u32, y_max: u32, out: &mut BufferTargetRgb888) {
    for i in 0..count {
        let h = fnv(i, seed);
        let x = (h % WIDTH as u32) as i32;
        let y = ((h >> 8) % y_max) as i32;
        let speed = 0.5 + ((h >> 4) % 12) as f32 * 0.17;
        let phase = (h >> 12) as f32 * 0.03;
        let tw = 0.25 + 0.75 * (sinp(t * speed + phase) * 0.5 + 0.5);
        let b = (tw * tw * 210.0) as u8;
        let c = match i % 11 {
            0 => Rgb888::new(b, (b as f32 * 0.72) as u8, (b as f32 * 0.62) as u8),
            1 => Rgb888::new((b as f32 * 0.7) as u8, (b as f32 * 0.85) as u8, b),
            _ => Rgb888::new(b, b, (b as f32 * 1.05).min(255.0) as u8),
        };

        put(x, y, c, out);

        if tw > 0.9 && x + 1 < W {
            add(x + 1, y, c, 0.25, out);
        }
    }
}

fn polygon(points: &[(i32, i32)], c: Rgb888, out: &mut BufferTargetRgb888) {
    if points.len() < 3 {
        return;
    }

    let mut min_y = i32::MAX;
    let mut max_y = i32::MIN;

    for p in points {
        min_y = min_y.min(p.1);
        max_y = max_y.max(p.1);
    }

    for y in min_y..=max_y {
        let mut xs = [0i32; 16];
        let mut n = 0usize;

        for i in 0..points.len() {
            let (x0, y0) = points[i];
            let (x1, y1) = points[(i + 1) % points.len()];

            if (y0 <= y && y1 > y) || (y1 <= y && y0 > y) {
                if n < xs.len() {
                    xs[n] = x0 + (y - y0) * (x1 - x0) / (y1 - y0);
                    n += 1;
                }
            }
        }

        xs[..n].sort_unstable();

        let mut i = 0;

        while i + 1 < n {
            hline(xs[i], xs[i + 1], y, c, out);
            i += 2;
        }
    }
}

fn moon(cx: i32, cy: i32, r: i32, out: &mut BufferTargetRgb888) {
    glow(cx as f32, cy as f32, r as f32 * 2.6, rgb(70, 90, 150), 0.5, out);
    draw_circle_filled(cx, cy, r, rgb(238, 234, 208), out);
    draw_circle_filled(cx - (r / 3), cy - (r / 4), (r / 4).max(1), rgb(214, 208, 184), out);
    draw_circle_filled(cx + (r / 3), cy + (r / 3), (r / 5).max(1), rgb(218, 212, 188), out);
    draw_circle_filled(cx + (r / 4), cy - (r / 3), (r / 6).max(1), rgb(222, 216, 192), out);
}

fn snow_layer(t: f32, seed: u32, count: u32, speed: f32, wind: f32, out: &mut BufferTargetRgb888) {
    for i in 0..count {
        let h = fnv(i, seed);
        let depth = ((h >> 4) % 10) as f32 / 9.0;
        let s = speed * (0.45 + depth * 1.2);
        let yf = (((h % H as u32) as f32) + t * s) % (H as f32 + 3.0) - 2.0;
        let xf = ((h >> 16) as f32 / 65536.0) * (W as f32 + 16.0)
            + t * wind * (0.4 + depth)
            + sinp(t * (0.35 + depth * 0.5) + i as f32) * (1.5 + depth * 3.0)
            - 8.0;
        let x = xf.rem_euclid(W as f32 + 16.0) as i32 - 8;
        let y = yf as i32;

        if y < 0 || y >= H {
            continue;
        }

        let v = (130.0 + depth * 125.0) as u8;
        let c = Rgb888::new((v as f32 * 0.86) as u8, (v as f32 * 0.93) as u8, 255);

        if depth > 0.72 {
            put(x, y, c, out);
            add(x + 1, y, c, 0.55, out);
            add(x, y + 1, c, 0.55, out);
            add(x - 1, y, c, 0.35, out);
        } else {
            over(x, y, c, 0.5 + depth * 0.5, out);
        }
    }
}

fn pine(x: i32, base_y: i32, height: i32, c: Rgb888, out: &mut BufferTargetRgb888) {
    draw_triangle_filled(
        x - height / 3,
        base_y,
        x + height / 3,
        base_y,
        x,
        base_y - height,
        c,
        out,
    );
    draw_triangle_filled(
        x - height / 4,
        base_y - height / 2,
        x + height / 4,
        base_y - height / 2,
        x,
        base_y - height - height / 4,
        c,
        out,
    );
    draw_rect_filled(x - 1, base_y, 2, 2, c, out);
}

fn leaf(x: i32, y: i32, size: i32, angle: f32, c: Rgb888, out: &mut BufferTargetRgb888) {
    let dx = angle.cos();
    let dy = angle.sin();

    ellipse(x as f32, y as f32, size as f32, (size as f32 * 0.5).max(1.0), c, out);

    // A pointed tip, vein and stem are what make the solid blob a leaf.
    put(
        x + (dx * (size + 1) as f32).round() as i32,
        y + (dy * (size + 1) as f32 * 0.55).round() as i32,
        c,
        out,
    );

    if size > 0 {
        let v = Rgb888::new(c.r() / 2 + 12, c.g() / 2 + 8, c.b() / 3);
        let vx = dx * size as f32 * 0.7;
        let vy = dy * size as f32 * 0.35;

        draw_line(x - vx as i32, y - vy as i32, x + vx as i32, y + vy as i32, v, out);
        draw_line(
            x - vx as i32,
            y - vy as i32,
            x - (vx * 1.8) as i32,
            y - (vy * 2.0) as i32,
            Rgb888::new(72, 44, 24),
            out,
        );
    }
}

fn sparkle(x: i32, y: i32, phase: f32, c: Rgb888, out: &mut BufferTargetRgb888) {
    let tw = sinp(phase) * 0.5 + 0.5;

    if tw < 0.25 {
        return;
    }

    let v = (tw * tw) as f32;

    add(x, y, c, v, out);

    if tw > 0.72 {
        add(x + 1, y, c, v * 0.4, out);
        add(x - 1, y, c, v * 0.4, out);
        add(x, y + 1, c, v * 0.4, out);
        add(x, y - 1, c, v * 0.4, out);
    }
}

// ── Nature's Magic ─────────────────────────────────────────────────────────

/// A deep, dusty starfield with a Milky Way band, coloured stars, a
/// constellation and the occasional shooting star.
pub fn starfield(ms: u32, out: &mut BufferTargetRgb888) {
    let t = ms as f32 / 1000.0;

    sky(rgb(1, 2, 10), rgb(7, 9, 30), out);

    // Milky Way dust: a diagonal river of faint stars.
    for i in 0..190 {
        let h = fnv(i, 149);
        let x = (h % 60) as f32;
        let base = 31.0 - x * 0.42;
        let y = base + ((h >> 8) % 14) as f32 - 7.0 + sinp(x * 0.07 + h as f32 * 0.001) * 2.0;
        let b = 18 + ((h >> 16) % 42) as u8;

        add(
            x as i32,
            y as i32,
            rgb((b as f32 * 0.8) as u8, (b as f32 * 0.85) as u8, b),
            0.5,
            out,
        );
    }

    stars(t, 13, 95, 39, out);

    // A small constellation (the Plough-ish shape) with faint joins.
    let pts = [(7i32, 9i32), (11, 12), (15, 11), (19, 15), (25, 14), (29, 18), (34, 17)];
    for pair in pts.windows(2) {
        draw_line(pair[0].0, pair[0].1, pair[1].0, pair[1].1, rgb(38, 44, 68), out);
    }
    for p in pts {
        add(p.0, p.1, rgb(190, 210, 255), 0.9, out);
    }

    // A ringed planet in the upper-left.
    draw_circle_filled(12, 6, 3, rgb(220, 170, 110), out);
    draw_circle_filled(11, 5, 1, rgb(250, 210, 150), out);
    draw_line(6, 8, 18, 4, rgb(190, 190, 220), out);
    draw_line(6, 9, 18, 5, rgb(110, 120, 160), out);

    // Shooting star every 6.5 s.
    let phase = t % 6.5;

    if phase < 1.1 {
        let p = phase / 1.1;
        for k in 0..8 {
            let x = (-4.0 + p * 76.0 - k as f32 * 1.6) as i32;
            let y = (2.0 + p * 16.0 - k as f32 * 0.35) as i32;
            let a = (1.0 - k as f32 / 8.0) * 0.9;

            add(x, y, rgb(220, 235, 255), a, out);

            if k < 3 {
                add(x, y + 1, rgb(120, 160, 255), a * 0.3, out);
            }
        }
    }
}

/// A village snowfall: layered flakes, a cottage window and a moon behind
/// the pines.
pub fn snowfall(ms: u32, out: &mut BufferTargetRgb888) {
    let t = ms as f32 / 1000.0;

    sky(rgb(8, 14, 38), rgb(28, 48, 78), out);
    moon(47, 7, 5, out);

    // Far pines.
    for x in (0..WIDTH as i32).step_by(9) {
        pine(x, 32, 9 + ((x * 5) % 6), rgb(20, 38, 54), out);
    }

    // Cottage with a warm window, snowy eaves and a smoking chimney.
    draw_rect_filled(6, 26, 11, 9, rgb(35, 26, 34), out);
    draw_triangle_filled(4, 26, 19, 26, 11, 20, rgb(226, 232, 236), out);
    draw_triangle_filled(5, 27, 18, 27, 11, 21, rgb(170, 184, 196), out);
    draw_rect_filled(3, 26, 18, 1, rgb(238, 244, 250), out);
    draw_rect_filled(14, 20, 2, 5, rgb(52, 38, 40), out);
    draw_rect_filled(13, 19, 4, 1, rgb(238, 244, 250), out);
    draw_rect_filled(8, 29, 3, 3, rgb(255, 190, 88), out);
    draw_rect_filled(13, 29, 2, 3, rgb(255, 166, 70), out);
    draw_rect_filled(10, 33, 3, 2, rgb(20, 16, 22), out);
    for k in 0..4 {
        over(
            15 + k as i32 / 2,
            18 - k as i32 * 2,
            rgb(180, 190, 200),
            0.12 - k as f32 * 0.02,
            out,
        );
    }
    glow(9.5, 30.5, 5.0, rgb(255, 170, 70), 0.3, out);

    // A snowman in front of the cottage.
    draw_circle_filled(28, 34, 3, rgb(226, 236, 244), out);
    draw_circle_filled(28, 30, 2, rgb(232, 242, 250), out);
    draw_circle_filled(28, 27, 1, rgb(238, 246, 252), out);
    draw_rect_filled(26, 25, 5, 1, rgb(34, 28, 34), out);
    draw_rect_filled(27, 23, 3, 2, rgb(34, 28, 34), out);
    put(29, 27, rgb(255, 150, 40), out);
    put(27, 27, rgb(20, 20, 26), out);
    put(29, 26, rgb(20, 20, 26), out);
    draw_line(26, 30, 22, 28, rgb(80, 54, 36), out);
    draw_line(30, 30, 34, 28, rgb(80, 54, 36), out);
    draw_rect_filled(26, 31, 1, 2, rgb(30, 24, 30), out);
    draw_rect_filled(29, 31, 1, 2, rgb(30, 24, 30), out);
    add(28, 34, rgb(200, 220, 240), 0.12, out);

    // Fence posts, mostly buried.
    for x in (20..38).step_by(4) {
        draw_rect_filled(x, 33, 1, 4, rgb(70, 52, 48), out);
        put(x, 33, rgb(230, 238, 246), out);
    }
    draw_rect_filled(20, 34, 18, 1, rgb(70, 52, 48), out);

    // Snowfield.
    for x in 0..WIDTH as i32 {
        let h = 2 + (sinp(x as f32 * 0.15) * 1.5) as i32;

        for y in (35 - h)..H {
            put(x, y, rgb(198, 212, 226), out);
        }
        put(x, 35 - h, rgb(240, 246, 252), out);
    }

    // Three snow layers so depth reads as parallax.
    snow_layer(t, 29, 26, 10.0, 2.0, out);
    snow_layer(t, 47, 22, 6.0, -1.0, out);
    snow_layer(t, 67, 16, 3.0, 0.5, out);

    // Sparkle on the drift.
    for i in 0..14 {
        let h = fnv(i, 151);
        sparkle(
            (h % 60) as i32,
            35 + ((h >> 8) % 4) as i32,
            t * 1.8 + i as f32,
            rgb(210, 235, 255),
            out,
        );
    }
}

/// Autumn leaves tumbling past a low sun: warm, layered and always in motion.
pub fn falling_leaves(ms: u32, out: &mut BufferTargetRgb888) {
    let t = ms as f32 / 1000.0;

    // Dusk gradient, amber at the horizon.
    for y in 0..HEIGHT {
        let f = y as f32 / (HEIGHT - 1) as f32;
        let r = 42.0 + 200.0 * f * f;
        let g = 16.0 + 110.0 * f * f;
        let b = 54.0 - 30.0 * f;

        for x in 0..WIDTH {
            let d = ((x + y) & 1) as u8;

            out.buffer[y * WIDTH + x] = Rgb888::new((r + d as f32) as u8, (g + d as f32) as u8, (b + d as f32) as u8);
        }
    }

    glow(47.0, 30.0, 18.0, rgb(255, 196, 100), 0.55, out);
    draw_circle_filled(47, 30, 5, rgb(255, 226, 150), out);

    // Distant treeline and a foreground trunk with branches.
    for x in (0..WIDTH as i32).step_by(7) {
        let h = 4 + ((x * 11) % 7);
        draw_triangle_filled(x, 37, x + 5, 37, x + 2, 37 - h, rgb(52, 26, 34), out);
    }
    draw_rect_filled(0, 18, 3, 22, rgb(38, 20, 24), out);
    draw_line(2, 20, 16, 8, rgb(38, 20, 24), out);
    draw_line(2, 26, 15, 17, rgb(38, 20, 24), out);
    draw_line(2, 12, 13, 3, rgb(38, 20, 24), out);
    leaf(14, 6, 2, 0.4, rgb(180, 54, 40), out);
    leaf(11, 16, 2, 1.9, rgb(212, 106, 30), out);
    leaf(6, 11, 1, 0.1, rgb(180, 54, 40), out);
    leaf(17, 12, 1, 2.4, rgb(236, 178, 42), out);

    // Tumbling leaves in three depth layers.
    const PALETTE: [(u8, u8, u8); 5] = [
        (238, 112, 30),
        (204, 48, 34),
        (246, 188, 52),
        (160, 94, 34),
        (255, 224, 120),
    ];

    for layer in 0..3 {
        let count = 6 + layer * 4;

        for i in 0..count {
            let h = fnv(i + layer * 23, 41);
            let (r, g, b) = PALETTE[(h % 5) as usize];
            let speed = 4.0 + layer as f32 * 3.0 + ((h >> 3) % 8) as f32 * 0.6;
            let yf = (((h >> 2) % 50) as f32 + t * speed) % (H as f32 + 8.0) - 4.0;
            let xf = ((h >> 16) as f32 / 65536.0) * (W as f32 + 24.0)
                + sinp(t * (0.6 + layer as f32 * 0.15) + i as f32 * 0.8) * (3.0 + layer as f32 * 2.0)
                + t * (1.0 + layer as f32 * 0.4)
                - 12.0;
            let x = xf.rem_euclid(W as f32 + 24.0) as i32 - 12;
            let y = yf as i32;

            if y < -4 || y > H + 4 {
                continue;
            }

            let turn = sinp(t * (1.4 + layer as f32 * 0.5) + i as f32 * 1.7);
            let size = 1 + (layer as i32 / 2) + if layer == 2 && turn.abs() > 0.78 { 1 } else { 0 };
            let dim = 0.55 + 0.45 * turn.abs();
            let c = Rgb888::new((r as f32 * dim) as u8, (g as f32 * dim) as u8, (b as f32 * dim) as u8);

            leaf(x, y, size as i32, t * (2.0 + layer as f32) + i as f32, c, out);

            if layer == 2 && turn.abs() > 0.5 {
                glow(x as f32, y as f32, 3.0, Rgb888::new(r, g, b), 0.18, out);
            }
        }
    }

    // Fallen leaf drift along the bottom.
    for i in 0..16 {
        let h = fnv(i, 43);
        let (r, g, b) = PALETTE[(h % 5) as usize];
        leaf(
            2 + (h % 56) as i32,
            37 + ((h >> 8) % 3) as i32,
            1 + ((h >> 16) % 2) as i32,
            h as f32 * 0.1,
            Rgb888::new(r / 2 + 20, g / 2 + 10, b / 2),
            out,
        );
    }

    // Birds heading home.
    for i in 0..3 {
        let x = ((t * 6.0 + i as f32 * 17.0) % 76.0) as i32 - 8;
        let y = 9 + i * 3;
        draw_line(x, y, x + 2, y - 1, rgb(40, 20, 24), out);
        draw_line(x + 2, y - 1, x + 4, y, rgb(40, 20, 24), out);
    }
}

/// A woodland pool at night: moon, reeds, and fireflies drifting over the
/// water.
pub fn fireflies(ms: u32, out: &mut BufferTargetRgb888) {
    let t = ms as f32 / 1000.0;

    sky(rgb(2, 8, 8), rgb(10, 30, 22), out);
    stars(t, 57, 35, 26, out);
    moon(47, 6, 4, out);

    // Layered pines.
    for x in (1..WIDTH as i32).step_by(8) {
        pine(x, 30, 12 + ((x * 5) % 6), rgb(6, 18, 14), out);
    }
    for x in (5..WIDTH as i32).step_by(11) {
        pine(x, 34, 9 + ((x * 3) % 5), rgb(3, 12, 10), out);
    }

    // Pond with the moon's path.
    for y in 32..H {
        let f = (y - 32) as f32 / (H - 32) as f32;

        for x in 0..WIDTH {
            let ripple = 0.5 + 0.5 * sinp(x as f32 * 0.42 + t * 1.3 + y as f32 * 1.5);

            out.buffer[y as usize * WIDTH + x] = Rgb888::new(
                (6.0 + 8.0 * f + 4.0 * ripple) as u8,
                (20.0 + 16.0 * f + 6.0 * ripple) as u8,
                (26.0 + 18.0 * f + 8.0 * ripple) as u8,
            );
        }
    }
    for y in 33..H {
        for dx in -3i32..=3 {
            let x = 47 + dx;
            let v = 0.25 * (1.0 - (y - 33) as f32 / 8.0) * (0.5 + 0.5 * sinp(x as f32 * 1.7 + t * 3.0));
            add(x, y, rgb(230, 230, 200), v.max(0.0), out);
        }
    }

    // Reeds and grass in front of the water.
    for x in (0..WIDTH as i32).step_by(4) {
        let h = 3 + (x * 7) % 5;
        draw_line(x, 40, x + 1, 40 - h, rgb(4, 18, 12), out);
        put(x + 1, 40 - h, rgb(30, 70, 34), out);
    }

    // Fireflies: glowing cores with short trails.
    for i in 0..24 {
        let h = fnv(i, 57);
        let base_x = 5.0 + (h % 50) as f32;
        let base_y = 7.0 + ((h >> 6) % 25) as f32;
        let sx = 0.22 + ((h >> 3) % 10) as f32 * 0.04;
        let sy = 0.18 + ((h >> 9) % 10) as f32 * 0.04;
        let pulse = 0.5 + ((h >> 13) % 12) as f32 * 0.15;

        for trail in (0..6).rev() {
            let age = trail as f32 * 0.11;
            let x = (base_x + sinp((t - age) * sx + i as f32) * 8.0 + sinp((t - age) * 0.13 + i as f32) * 4.0) as i32;
            let y = (base_y + sinp((t - age) * sy + i as f32 * 0.7) * 5.0) as i32;
            let glowv = sinp((t - age) * pulse + i as f32 * 1.7) * 0.5 + 0.5;
            let glowv = glowv.powi(4) * (1.0 - trail as f32 / 7.0);

            if glowv > 0.02 {
                add(x, y, rgb(160, 255, 110), glowv * 0.8, out);
            }
        }

        let x = (base_x + sinp(t * sx + i as f32) * 8.0 + sinp(t * 0.13 + i as f32) * 4.0) as i32;
        let y = (base_y + sinp(t * sy + i as f32 * 0.7) * 5.0) as i32;
        let glowv = sinp(t * pulse + i as f32 * 1.7) * 0.5 + 0.5;

        if glowv > 0.04 {
            glow(x as f32, y as f32, 3.4, rgb(180, 255, 110), glowv * 0.9, out);
            put(x, y, rgb(245, 255, 190), out);
            put(x + 1, y, rgb(190, 255, 120), out);
        }
    }
}

/// A robin on a snow-laden branch: breathing chest, the occasional head
/// turn, and a wing flick, against a cold winter morning.
pub fn robin(ms: u32, out: &mut BufferTargetRgb888) {
    let t = ms as f32 / 1000.0;

    sky(rgb(28, 48, 78), rgb(132, 166, 190), out);

    // Low winter sun is only a warm smear behind the branch.
    glow(46.0, 8.0, 13.0, rgb(255, 214, 150), 0.35, out);
    draw_circle_filled(46, 8, 4, rgb(255, 236, 196), out);

    // Distant hedge line.
    for x in 0..WIDTH as i32 {
        let h = 3 + ((x * 13 + 7) % 5);
        hline(x, x, 31 - h, rgb(34, 54, 66), out);
        hline(x, x, 32 - h, rgb(28, 46, 58), out);
        for y in 32 - h..34 {
            put(x, y, rgb(22, 36, 48), out);
        }
    }

    snow_layer(t, 5, 28, 7.0, 2.0, out);

    // Branch with a thick snow cap.
    for k in 0..5 {
        draw_line(-2, 32 + k, 58, 38 + k / 2, rgb(38, 26, 22), out);
    }
    for k in 0..2 {
        draw_line(2, 31 + k, 58, 37 + k, rgb(228, 236, 244), out);
    }
    draw_line(40, 37, 58, 31, rgb(38, 26, 22), out);
    draw_line(40, 36, 58, 30, rgb(228, 236, 244), out);

    let bob = sinp(t * 1.7) * 0.8;
    let turn = sinp(t * 0.45);
    let hx = (turn * 1.3) as i32;
    let flick = (sinp(t * 0.8 + 1.0) * 0.5 + 0.5).powi(10);

    let bx = 27.0;
    let by = 23.5 + bob;

    // Tail and feet.
    draw_triangle_filled(19, 27, 23, 24, 20, 35, rgb(44, 30, 24), out);
    draw_line(25, 31, 25, 33, rgb(40, 30, 26), out);
    draw_line(30, 32, 30, 34, rgb(40, 30, 26), out);

    // Body.
    ellipse(bx, by, 8.0, 9.0, rgb(58, 40, 32), out);
    ellipse(bx + 1.0, by + 2.0, 6.4, 6.4, rgb(226, 82, 38), out);
    ellipse(bx + 1.5, by + 3.0, 4.2, 4.4, rgb(250, 128, 52), out);
    ellipse(bx + 1.5, by + 4.5, 2.5, 2.5, rgb(255, 172, 90), out);

    // Wing (lifted slightly on the flick).
    let wing_y = by - 1.0 - flick * 5.0;
    ellipse(bx - 4.0, wing_y, 3.6, 6.2, rgb(70, 48, 36), out);
    draw_line(
        (bx - 6.0) as i32,
        (wing_y - 3.0) as i32,
        (bx - 2.0) as i32,
        (wing_y + 4.0) as i32,
        rgb(110, 78, 54),
        out,
    );

    // Head, eye and beak.
    ellipse(bx + 3.0 + hx as f32, by - 7.0, 5.2, 5.2, rgb(52, 36, 30), out);
    ellipse(bx + 4.0 + hx as f32, by - 7.4, 2.0, 2.0, rgb(255, 255, 255), out);
    put(34 + hx, 15, rgb(8, 8, 10), out);

    let beak_x = 36 + hx;
    draw_triangle_filled(beak_x, 15, beak_x + 3, 16, beak_x, 17, rgb(255, 176, 46), out);
    put(beak_x, 16, rgb(210, 120, 30), out);

    // Holly berries in the bottom corner.
    for (x, y) in [(4, 36), (7, 34), (9, 37)] {
        draw_circle_filled(x, y, 1, rgb(190, 34, 30), out);
    }

    for i in 0..12 {
        let h = fnv(i, 91);
        sparkle(
            (h % W as u32) as i32,
            28 + ((h >> 8) % 11) as i32,
            t * 2.0 + i as f32,
            rgb(210, 230, 255),
            out,
        );
    }
}

/// A great horned owl in the moonlight: huge amber eyes, a slow head turn
/// and a blink you can catch if you watch.
pub fn owl(ms: u32, out: &mut BufferTargetRgb888) {
    let t = ms as f32 / 1000.0;

    sky(rgb(4, 10, 24), rgb(16, 42, 52), out);
    stars(t, 33, 55, 26, out);
    moon(48, 8, 6, out);

    // Far trees, then the trunk the owl owns.
    for x in (2..WIDTH as i32).step_by(11) {
        pine(x, 34, 12 + ((x * 7) % 5), rgb(8, 20, 24), out);
    }

    draw_rect_filled(2, 0, 6, 40, rgb(18, 20, 22), out);
    draw_rect_filled(3, 0, 1, 40, rgb(34, 32, 30), out);
    draw_line(8, 8, 22, 3, rgb(20, 22, 24), out);
    draw_line(8, 12, 19, 8, rgb(20, 22, 24), out);

    // Branch under the owl.
    for k in 0..4 {
        draw_line(18, 31 + k, 58, 33 + k, rgb(26, 22, 20), out);
    }
    for _ in 0..1 {
        draw_line(18, 30, 58, 32, rgb(196, 208, 210), out);
    }

    let turn = sinp(t * 0.38);
    let hx = (turn * 2.2) as i32;
    let bob = sinp(t * 1.1) * 0.5;

    // Moon halo behind the bird: without it the dark-brown body vanishes
    // into the night sky.
    glow(35.0, 24.0, 18.0, rgb(90, 130, 160), 0.55, out);

    // Body.
    ellipse(35.0, 25.0 + bob, 8.0, 9.5, rgb(78, 52, 32), out);
    ellipse(35.0, 26.0 + bob, 6.5, 7.8, rgb(118, 80, 46), out);
    ellipse(35.0, 28.0 + bob, 4.0, 4.5, rgb(150, 104, 58), out);

    // Folded wing and speckles.
    ellipse(29.5, 26.0 + bob, 3.4, 6.6, rgb(52, 38, 28), out);
    draw_line(26, 21 + bob as i32, 26, 32 + bob as i32, rgb(120, 160, 180), out);
    draw_line(43, 21 + bob as i32, 43, 32 + bob as i32, rgb(150, 120, 74), out);
    for i in 0..24 {
        let h = fnv(i, 17);
        let x = 30 + (h % 11) as i32;
        let y = 18 + ((h >> 8) % 13) as i32;
        put(x, y, rgb(170, 132, 78), out);
        if i % 4 == 0 {
            put(x + 1, y, rgb(210, 170, 104), out);
        }
    }

    // Head with ear tufts.
    draw_triangle_filled(28, 14, 30, 8, 33, 12, rgb(70, 48, 30), out);
    draw_triangle_filled(38, 14, 36, 8, 33, 12, rgb(70, 48, 30), out);
    ellipse(33.0 + hx as f32, 15.0 + bob, 6.3, 5.8, rgb(84, 58, 36), out);
    put(29, 9, rgb(160, 190, 210), out);
    put(37, 9, rgb(160, 190, 210), out);

    // Facial discs.
    ellipse(30.5 + hx as f32, 15.4 + bob, 3.2, 3.8, rgb(116, 96, 68), out);
    ellipse(37.0 + hx as f32, 15.4 + bob, 3.2, 3.8, rgb(116, 96, 68), out);

    // Blink windows: eyes go dark for ~140 ms each cycle.
    let blink = (t % 4.3) < 0.14;

    if blink {
        draw_line(29 + hx, 15, 32 + hx, 15, rgb(30, 22, 16), out);
        draw_line(36 + hx, 15, 39 + hx, 15, rgb(30, 22, 16), out);
    } else {
        glow(30.5 + hx as f32, 15.0, 3.0, rgb(255, 180, 40), 0.35, out);
        glow(37.0 + hx as f32, 15.0, 3.0, rgb(255, 180, 40), 0.35, out);
        draw_circle_filled(30 + hx, 15, 1, rgb(255, 206, 60), out);
        draw_circle_filled(37 + hx, 15, 1, rgb(255, 206, 60), out);
        put(30 + hx, 15, rgb(10, 8, 6), out);
        put(37 + hx, 15, rgb(10, 8, 6), out);
        put(31 + hx, 14, rgb(255, 255, 215), out);
        put(38 + hx, 14, rgb(255, 255, 215), out);
    }

    // Beak and chest feathers.
    draw_triangle_filled(33 + hx, 17, 35 + hx, 17, 34 + hx, 20, rgb(230, 180, 80), out);
    draw_line(30, 24, 33, 25, rgb(160, 120, 70), out);
    draw_line(33, 25, 36, 24, rgb(160, 120, 70), out);
    draw_line(31, 28, 35, 29, rgb(160, 120, 70), out);
    draw_line(31, 31, 35, 32, rgb(140, 104, 62), out);

    // Fireflies keep the lower corner alive.
    for i in 0..5 {
        let h = fnv(i, 51);
        let fx = (h % 25) as i32;
        let fy = 28 + ((h >> 8) % 8) as i32;
        sparkle(fx, fy, t * 1.3 + i as f32 * 1.7, rgb(150, 255, 110), out);
    }
}

/// A stag on a moonlit ridge — antlers, mist and a head that lifts to the
/// sky. The only bright thing in a blue winter night.
pub fn deer(ms: u32, out: &mut BufferTargetRgb888) {
    let t = ms as f32 / 1000.0;

    sky(rgb(6, 14, 30), rgb(26, 52, 70), out);
    stars(t, 61, 48, 24, out);
    glow(9.0, 9.0, 20.0, rgb(90, 120, 160), 0.55, out);
    moon(9, 9, 7, out);

    for x in (2..WIDTH as i32).step_by(9) {
        pine(x, 30, 10 + ((x * 5) % 7), rgb(9, 24, 30), out);
    }

    // Two ridges; the stag stands on the nearer one.
    for x in 0..WIDTH as i32 {
        let h = 3 + (sinp(x as f32 * 0.19 + 1.4) * 2.0) as i32;
        for y in (33 - h)..H {
            put(x, y, rgb(12, 0x1d, 0x22), out);
        }
    }
    for x in 0..WIDTH as i32 {
        let h = 2 + (sinp(x as f32 * 0.13 + 4.0) * 1.5) as i32;
        for y in (36 - h)..H {
            put(x, y, rgb(6, 12, 14), out);
        }
    }

    // Mist bands drifting across the ridge.
    for k in 0..3 {
        let yy = 29 + k * 2;
        let shift = (sinp(t * 0.35 + k as f32) * 12.0) as i32;
        for x in 0..WIDTH as i32 {
            let a = 0.10 + 0.08 * sinp(x as f32 * 0.1 + t + k as f32);
            over((x + shift).rem_euclid(W), yy, rgb(120, 150, 170), a, out);
        }
    }

    // Grazing / lifting head cycle.
    let lift = (sinp(t * 0.42) * 0.5 + 0.5).powi(3);
    let head_y = 15.0 - lift * 3.5;
    let head_x = 38.0 + sinp(t * 0.28) * 0.8;

    // Legs: four thin winter legs under a compact body.
    for (x0, x1) in [(19, 18), (24, 25), (29, 30), (33, 34)] {
        draw_line(x0, 27, x1, 36, rgb(12, 10, 10), out);
        draw_line(x0 + 1, 27, x1 + 1, 36, rgb(22, 18, 14), out);
        put(x1, 36, rgb(38, 32, 26), out);
    }

    // Body, neck, head and muzzle silhouette. The neck follows the grazing/
    // lifting head; the old fixed neck left the head floating at the top of
    // the lift cycle.
    ellipse(25.0, 24.5, 9.0, 4.2, rgb(14, 12, 12), out);
    ellipse(21.0, 24.0, 3.6, 3.8, rgb(14, 12, 12), out);

    let neck_x = head_x - 1.0;
    let neck_y = head_y + 1.5;

    for k in 0..3 {
        draw_line(
            29 + k,
            24,
            (neck_x - 1.0 + k as f32) as i32,
            neck_y as i32,
            rgb(14, 12, 12),
            out,
        );
    }
    ellipse(neck_x - 0.5, neck_y + 2.0, 2.4, 3.0, rgb(14, 12, 12), out);

    ellipse(head_x, head_y, 3.0, 2.3, rgb(16, 13, 12), out);
    draw_triangle_filled(
        (head_x + 2.0) as i32,
        (head_y as i32) - 1,
        (head_x + 5.0) as i32,
        head_y as i32,
        (head_x + 2.0) as i32,
        (head_y as i32) + 1,
        rgb(16, 13, 12),
        out,
    );

    // Ears and a proud set of antlers. Ears ride with the head.
    let ear_y = head_y as i32;

    draw_triangle_filled(
        (head_x - 2.0) as i32,
        ear_y,
        (head_x - 1.0) as i32,
        ear_y - 5,
        (head_x + 1.0) as i32,
        ear_y,
        rgb(14, 12, 12),
        out,
    );
    draw_triangle_filled(
        (head_x + 2.0) as i32,
        ear_y,
        (head_x + 3.5) as i32,
        ear_y - 4,
        (head_x + 5.0) as i32,
        ear_y + 1,
        rgb(14, 12, 12),
        out,
    );
    let ax = head_x - 0.5;
    let ay = head_y - 2.5;
    let sway = sinp(t * 0.3) * 0.3;
    for (dx, dy) in [
        (0, -2),
        (1, -4),
        (2, -6),
        (3, -8),
        (-1, -4),
        (-2, -6),
        (0, -8),
        (1, -7),
        (4, -6),
    ] {
        let ex = ax + dx as f32 + sway * dy as f32;
        let ey = ay + dy as f32;
        draw_line(ax as i32, ay as i32, ex as i32, ey as i32, rgb(130, 96, 52), out);
    }
    for (dx, dy) in [(1, -4), (-2, -6), (2, -6)] {
        draw_line(
            ax as i32 + dx,
            ay as i32 + dy,
            ax as i32 + dx + 2,
            ay as i32 + dy - 2,
            rgb(130, 96, 52),
            out,
        );
    }

    // Tail with a pale tuft.
    draw_line(16, 23, 14, 20, rgb(14, 12, 12), out);
    put(14, 19, rgb(210, 190, 160), out);

    // Rim light along the back and antlers — the stag is a black silhouette,
    // so a moonlit edge is what makes it readable on the panel.
    draw_line(18, 23, 31, 21, rgb(190, 220, 245), out);
    for x in [24, 27, 30] {
        put(x, 20, rgb(220, 240, 255), out);
    }
    put((head_x as i32) - 1, (head_y as i32) - 2, rgb(210, 235, 255), out);
    put((head_x as i32) + 1, (head_y as i32) + 1, rgb(160, 190, 220), out);
    put(ax as i32, ay as i32, rgb(255, 220, 150), out);
    put((ax + 2.0) as i32, (ay - 6.0) as i32, rgb(255, 210, 140), out);
    put((ax + 3.0) as i32, (ay - 8.0) as i32, rgb(230, 190, 120), out);
    put((head_x as i32) + 2, head_y as i32, rgb(255, 210, 130), out);

    // Eye glint.
    put((head_x as i32) + 2, (head_y as i32) - 1, rgb(255, 190, 80), out);

    // Snow at the hooves and fireflies in the grass.
    for i in 0..10 {
        let h = fnv(i, 77);
        sparkle(
            (h % 20) as i32,
            32 + ((h >> 8) % 6) as i32,
            t * 1.6 + i as f32,
            rgb(170, 220, 255),
            out,
        );
    }
    for i in 0..5 {
        let h = fnv(i, 83);
        sparkle(
            20 + (h % 30) as i32,
            26 + ((h >> 8) % 6) as i32,
            t * 2.0 + i as f32 * 1.3,
            rgb(180, 255, 120),
            out,
        );
    }

    snow_layer(t, 23, 24, 4.0, 1.5, out);
}

// ── Awe and Wonder ─────────────────────────────────────────────────────────

/// A close-up snow leopard in the moonlight: huge green eyes, rosettes and
/// frosty breath. A portrait reads far better than a side-on silhouette at
/// 60x40.
pub fn snow_leopard(ms: u32, out: &mut BufferTargetRgb888) {
    let t = ms as f32 / 1000.0;

    sky(rgb(4, 12, 30), rgb(38, 64, 92), out);
    stars(t, 41, 50, 20, out);
    glow(12.0, 8.0, 24.0, rgb(150, 180, 220), 0.4, out);
    moon(12, 8, 6, out);

    // Distant peaks with snow caps.
    for x in 0..WIDTH as i32 {
        let h = 7 + (sinp(x as f32 * 0.11 + 2.0) * 4.0 + sinp(x as f32 * 0.31) * 2.5) as i32;

        for y in (28 - h)..34 {
            put(x, y, rgb(30, 46, 66), out);
        }
        for y in (28 - h)..(30 - h / 2) {
            put(x, y, rgb(92, 116, 140), out);
        }
    }

    // Snow ledge.
    for x in 0..WIDTH as i32 {
        let top = 32 + (sinp(x as f32 * 0.2) * 0.8) as i32;

        for y in top..H {
            put(x, y, rgb(52, 66, 82), out);
        }

        put(x, top, rgb(170, 190, 205), out);
    }

    snow_layer(t, 17, 26, 5.0, -2.0, out);

    let tilt = sinp(t * 0.6) * 1.2;
    let bob = sinp(t * 1.5) * 0.5;
    let cx = 30.0 + tilt;
    let cy = 16.5 + bob;

    let fur = rgb(136, 146, 154);
    let fur_light = rgb(196, 202, 200);
    let fur_dark = rgb(76, 86, 98);

    // Chest, shoulders and front paws.
    ellipse(cx, 36.0, 15.0, 9.0, fur_dark, out);
    ellipse(cx, 35.0, 12.0, 7.0, fur, out);
    ellipse(cx, 36.0, 7.0, 5.0, fur_light, out);
    ellipse(cx - 8.0, 38.0, 4.0, 2.4, fur, out);
    ellipse(cx + 8.0, 38.0, 4.0, 2.4, fur, out);

    for k in 0..3 {
        put((cx - 10.0 + k as f32 * 2.0) as i32, 39, fur_light, out);
        put((cx + 6.0 + k as f32 * 2.0) as i32, 39, fur_light, out);
    }

    // Tail curling in from the left.
    for i in 0..18 {
        let f = i as f32 / 17.0;
        let x = cx - 14.0 - f * 7.0 + sinp(t * 0.8 + f * 3.0) * 1.5;
        let y = 38.0 - f * 8.0;

        put(x as i32, y as i32, fur_dark, out);

        if i % 3 == 0 {
            put(x as i32 + 1, y as i32, fur, out);
        }
    }
    put((cx - 21.0) as i32, 30, rgb(44, 52, 66), out);

    // Ears, outer then inner. Snow leopards have short, rounded ears; keep
    // the pink subdued so the cat does not read as a house cat.
    draw_triangle_filled(
        (cx - 10.0) as i32,
        (cy - 5.0) as i32,
        (cx - 12.0) as i32,
        (cy - 12.0) as i32,
        (cx - 3.0) as i32,
        (cy - 8.0) as i32,
        fur,
        out,
    );
    draw_triangle_filled(
        (cx + 10.0) as i32,
        (cy - 5.0) as i32,
        (cx + 12.0) as i32,
        (cy - 12.0) as i32,
        (cx + 3.0) as i32,
        (cy - 8.0) as i32,
        fur,
        out,
    );
    draw_triangle_filled(
        (cx - 9.0) as i32,
        (cy - 7.0) as i32,
        (cx - 10.5) as i32,
        (cy - 10.0) as i32,
        (cx - 5.0) as i32,
        (cy - 8.0) as i32,
        rgb(66, 72, 82),
        out,
    );
    draw_triangle_filled(
        (cx + 9.0) as i32,
        (cy - 7.0) as i32,
        (cx + 10.5) as i32,
        (cy - 10.0) as i32,
        (cx + 5.0) as i32,
        (cy - 8.0) as i32,
        rgb(66, 72, 82),
        out,
    );

    // Head, cheek ruff and a dark outline so the face separates from the
    // moonlit peaks.
    ellipse(cx, cy, 12.0, 10.5, fur_dark, out);
    ellipse(cx, cy, 11.0, 9.5, fur, out);
    ellipse(cx, cy + 2.0, 10.0, 7.0, fur, out);
    ellipse(cx, cy - 2.0, 8.0, 6.0, fur, out);
    draw_triangle_filled(
        (cx - 11.0) as i32,
        (cy - 1.0) as i32,
        (cx - 8.0) as i32,
        (cy + 4.0) as i32,
        (cx - 7.0) as i32,
        (cy - 1.0) as i32,
        fur_light,
        out,
    );
    draw_triangle_filled(
        (cx + 11.0) as i32,
        (cy - 1.0) as i32,
        (cx + 8.0) as i32,
        (cy + 4.0) as i32,
        (cx + 7.0) as i32,
        (cy - 1.0) as i32,
        fur_light,
        out,
    );

    // Rosettes on the forehead and cheeks: 2x2 broken rings, kept clear of
    // the eyes and muzzle.
    let spots = [
        (cx - 7.0, cy - 6.0),
        (cx - 3.0, cy - 7.0),
        (cx + 1.0, cy - 7.0),
        (cx + 5.0, cy - 6.0),
        (cx - 8.0, cy - 2.0),
        (cx + 8.0, cy - 2.0),
        (cx - 9.0, cy - 1.0),
        (cx + 9.0, cy - 1.0),
        (cx - 9.0, cy + 3.0),
        (cx + 9.0, cy + 3.0),
        (cx - 7.0, cy + 6.0),
        (cx + 7.0, cy + 6.0),
        (cx - 10.0, cy + 5.0),
        (cx + 10.0, cy + 5.0),
        (cx - 5.0, cy - 4.0),
        (cx + 6.0, cy - 4.0),
    ];

    for (sx, sy) in spots {
        let x = sx as i32;
        let y = sy as i32;

        put(x, y, rgb(52, 58, 68), out);
        put(x + 1, y, rgb(52, 58, 68), out);
        put(x, y + 1, rgb(68, 74, 84), out);
        put(x + 1, y + 1, rgb(68, 74, 84), out);
    }

    // A few chest rosettes.
    for (sx, sy) in [
        (cx - 10.0, 34.0),
        (cx - 6.0, 33.0),
        (cx + 7.0, 34.0),
        (cx + 11.0, 36.0),
        (cx - 11.0, 37.0),
    ] {
        put(sx as i32, sy as i32, rgb(52, 58, 68), out);
        put(sx as i32 + 1, sy as i32, rgb(52, 58, 68), out);
        put(sx as i32, sy as i32 + 1, rgb(68, 74, 84), out);
    }

    // Eyes: green, glowing, with a vertical pupil. They blink.
    let blink = (t % 4.6) < 0.13;
    let eye_y = (cy - 2.0) as i32;

    // Heavy brows and eyeliner give the face a wild-cat expression rather
    // than a house-cat one.
    draw_line(
        (cx - 9.0) as i32,
        (cy - 5.0) as i32,
        (cx - 4.0) as i32,
        (cy - 4.0) as i32,
        rgb(48, 54, 64),
        out,
    );
    draw_line(
        (cx + 4.0) as i32,
        (cy - 4.0) as i32,
        (cx + 9.0) as i32,
        (cy - 5.0) as i32,
        rgb(48, 54, 64),
        out,
    );
    draw_line(
        (cx - 9.0) as i32,
        (cy - 4.0) as i32,
        (cx - 3.0) as i32,
        (cy - 3.0) as i32,
        rgb(48, 54, 64),
        out,
    );
    draw_line(
        (cx + 3.0) as i32,
        (cy - 3.0) as i32,
        (cx + 9.0) as i32,
        (cy - 4.0) as i32,
        rgb(48, 54, 64),
        out,
    );

    for ex in [(cx - 5.0) as i32, (cx + 5.0) as i32] {
        if blink {
            draw_line(ex - 2, eye_y, ex + 2, eye_y, rgb(38, 30, 34), out);
        } else {
            glow(ex as f32, eye_y as f32, 3.6, rgb(80, 220, 130), 0.35, out);
            ellipse(ex as f32, eye_y as f32, 3.0, 2.0, rgb(24, 30, 28), out);
            ellipse(ex as f32, eye_y as f32, 1.8, 1.0, rgb(100, 226, 142), out);
            put(ex, eye_y - 1, rgb(12, 22, 18), out);
            put(ex, eye_y, rgb(12, 22, 18), out);
            put(ex - 1, eye_y - 1, rgb(230, 255, 230), out);
        }
    }
    // Tear lines from the inner corners.
    draw_line(
        (cx - 3.0) as i32,
        (cy - 1.0) as i32,
        (cx - 2.0) as i32,
        (cy + 3.0) as i32,
        rgb(70, 78, 88),
        out,
    );
    draw_line(
        (cx + 3.0) as i32,
        (cy - 1.0) as i32,
        (cx + 2.0) as i32,
        (cy + 3.0) as i32,
        rgb(70, 78, 88),
        out,
    );

    // Muzzle, nose, mouth and whisker spots.
    ellipse(cx, cy + 5.0, 5.0, 3.0, rgb(186, 192, 192), out);
    ellipse(cx, cy + 6.5, 4.0, 1.4, rgb(130, 138, 142), out);
    draw_triangle_filled(
        (cx - 1.0) as i32,
        (cy + 3.0) as i32,
        (cx + 1.0) as i32,
        (cy + 3.0) as i32,
        cx as i32,
        (cy + 5.0) as i32,
        rgb(104, 84, 90),
        out,
    );
    draw_line(
        cx as i32,
        (cy + 5.0) as i32,
        cx as i32,
        (cy + 7.0) as i32,
        rgb(96, 72, 74),
        out,
    );
    draw_line(
        cx as i32,
        (cy + 7.0) as i32,
        (cx - 2.0) as i32,
        (cy + 8.0) as i32,
        rgb(96, 72, 74),
        out,
    );
    draw_line(
        cx as i32,
        (cy + 7.0) as i32,
        (cx + 2.0) as i32,
        (cy + 8.0) as i32,
        rgb(96, 72, 74),
        out,
    );

    for k in 0..3 {
        let y = (cy + 5.0 + k as f32) as i32;
        put((cx - 5.0) as i32 - k, y, rgb(70, 62, 64), out);
        put((cx + 5.0) as i32 + k, y, rgb(70, 62, 64), out);
        draw_line(
            (cx - 5.0) as i32,
            y,
            (cx - 12.0) as i32,
            y - 1 + k as i32,
            rgb(210, 218, 226),
            out,
        );
        draw_line(
            (cx + 5.0) as i32,
            y,
            (cx + 12.0) as i32,
            y - 1 + k as i32,
            rgb(210, 218, 226),
            out,
        );
    }

    // Frosty breath drifting to the right.
    let breath = t % 3.2;
    for k in 0..5 {
        let a = (1.0 - breath / 1.6).max(0.0) * 0.3;

        if breath < 1.6 && a > 0.0 {
            over(
                (cx + 6.0 + k as f32 * 1.8) as i32,
                (cy + 7.0 + k as f32 * 0.4) as i32,
                rgb(210, 228, 242),
                a,
                out,
            );
        }
    }

    // Snow dusting along the top of the head.
    for i in 0..8 {
        let h = fnv(i, 113);
        put(
            (cx - 8.0 + (h % 17) as f32) as i32,
            ((h >> 8) % 3) as i32 + 8,
            rgb(230, 242, 250),
            out,
        );
    }

    // Snowy sparkle on the ledge.
    for i in 0..12 {
        let h = fnv(i, 121);
        sparkle(
            (h % 60) as i32,
            33 + ((h >> 8) % 5) as i32,
            t * 1.7 + i as f32,
            rgb(220, 245, 255),
            out,
        );
    }
}

/// Santa and two reindeer crossing a huge moon, with snowy rooftops below.
pub fn father_christmas(ms: u32, out: &mut BufferTargetRgb888) {
    let t = ms as f32 / 1000.0;

    sky(rgb(4, 6, 24), rgb(28, 22, 60), out);
    stars(t, 71, 75, 26, out);
    glow(16.0, 11.0, 30.0, rgb(170, 170, 220), 0.45, out);
    moon(16, 11, 9, out);

    // Rooftops and chimney smoke.
    for x in 0..WIDTH as i32 {
        let h = 5 + ((x * 7 + 3) % 6);
        for y in (34 - h)..H {
            put(x, y, rgb(10, 10, 22), out);
        }
        if x % 13 == 5 {
            draw_rect_filled(x, 26, 3, 8, rgb(12, 12, 24), out);
            for k in 0..4 {
                let a = 0.10 - k as f32 * 0.02;
                over(x + 1 + (k as i32 / 2), 24 - k as i32 * 2, rgb(120, 120, 140), a, out);
            }
        }
    }
    // Warm windows.
    for (x, y) in [(8, 33), (22, 35), (31, 32), (46, 34), (52, 31)] {
        add(x, y, rgb(255, 172, 62), 0.9, out);
        add(x + 1, y, rgb(255, 140, 40), 0.35, out);
    }

    // The sleigh crosses left-to-right with a gentle bob; the reindeer lead.
    let cycle = 12.0;
    let p = (t % cycle) / cycle;
    let sx = -40.0 + p * (W as f32 + 60.0);
    let bob = sinp(t * 1.6) * 1.0;
    let sy = 14.0 + bob;

    // Reindeer lead the sleigh: two silhouettes, legs cycling, rim-lit so
    // they read against the night sky.
    for (k, dx) in [(0.0, 15.0), (1.0, 29.0)] {
        let rx = sx + dx;
        let ry = sy + if k == 1.0 { -1.2 } else { 0.6 } + sinp(t * 2.2 + k) * 0.4;
        let leg = sinp(t * 5.0 + k * 2.0);

        // Body, neck and head.
        ellipse(rx, ry, 4.4, 2.6, rgb(48, 32, 24), out);
        draw_line(
            (rx + 2.5) as i32,
            (ry - 0.5) as i32,
            (rx + 5.0) as i32,
            (ry - 3.0) as i32,
            rgb(48, 32, 24),
            out,
        );
        ellipse(rx + 5.6, ry - 3.2, 2.0, 1.5, rgb(54, 36, 26), out);
        // Antlers.
        draw_line(
            (rx + 4.5) as i32,
            (ry - 4.5) as i32,
            (rx + 4.0) as i32,
            (ry - 7.0) as i32,
            rgb(110, 82, 54),
            out,
        );
        draw_line(
            (rx + 4.0) as i32,
            (ry - 7.0) as i32,
            (rx + 2.0) as i32,
            (ry - 9.0) as i32,
            rgb(110, 82, 54),
            out,
        );
        draw_line(
            (rx + 4.0) as i32,
            (ry - 7.0) as i32,
            (rx + 6.5) as i32,
            (ry - 9.0) as i32,
            rgb(110, 82, 54),
            out,
        );
        // Legs, two strokes each.
        draw_line(
            (rx - 2.0) as i32,
            (ry + 1.0) as i32,
            (rx - 3.5 + leg * 2.2) as i32,
            (ry + 5.5) as i32,
            rgb(48, 32, 24),
            out,
        );
        draw_line(
            (rx + 2.0) as i32,
            (ry + 1.0) as i32,
            (rx + 3.5 - leg * 2.2) as i32,
            (ry + 5.5) as i32,
            rgb(48, 32, 24),
            out,
        );
        // Warm rim along the back.
        for x in 0..4 {
            put((rx - 3.5) as i32 + x, (ry - 2.0) as i32, rgb(255, 200, 130), out);
        }
        // Rudolph's nose only on the lead deer.
        if k == 1.0 {
            add((rx + 7.2) as i32, (ry - 3.2) as i32, rgb(255, 40, 40), 1.0, out);
            glow(rx + 7.2, ry - 3.2, 4.0, rgb(255, 40, 40), 0.5, out);
        }
        // Harness line back toward the sleigh.
        if k == 0.0 {
            draw_line(
                (rx - 3.0) as i32,
                (ry + 1.0) as i32,
                (sx + 13.0) as i32,
                (sy + 3.0) as i32,
                rgb(200, 166, 90),
                out,
            );
        }
    }

    // Sleigh: right-facing runner, high back on the left, Santa facing the
    // reindeer, sack trailing behind. (The old layout put Santa at the front
    // and the sack at the back, which read as a sleigh travelling backwards.)
    let sx_i = sx as i32;
    let sy_i = sy as i32;
    draw_line(sx_i - 4, sy_i + 5, sx_i + 13, sy_i + 5, rgb(210, 166, 70), out);
    draw_line(sx_i + 11, sy_i + 5, sx_i + 13, sy_i + 2, rgb(210, 166, 70), out);
    draw_line(sx_i - 4, sy_i + 5, sx_i - 6, sy_i + 3, rgb(210, 166, 70), out);
    for k in 0..3 {
        draw_line(sx_i - 2 + k, sy_i + 3, sx_i - 2 + k, sy_i + 2, rgb(210, 166, 70), out);
    }

    // High back, seat and red body.
    draw_rect_filled(sx_i - 3, sy_i - 4, 4, 5, rgb(150, 22, 28), out);
    draw_rect_filled(sx_i - 3, sy_i - 4, 4, 1, rgb(230, 180, 90), out);
    draw_rect_filled(sx_i, sy_i - 1, 10, 3, rgb(190, 34, 36), out);
    ellipse(sx as f32 + 6.0, sy + 1.0, 5.5, 2.5, rgb(170, 28, 32), out);
    draw_line(sx_i, sy_i - 1, sx_i + 9, sy_i - 1, rgb(230, 180, 90), out);

    // Santa in the back seat, facing right toward the reindeer.
    ellipse(sx as f32 + 2.0, sy - 3.0, 2.4, 3.0, rgb(200, 32, 34), out);
    put(sx_i + 4, sy_i - 4, rgb(240, 236, 220), out);
    put(sx_i + 5, sy_i - 4, rgb(240, 236, 220), out);
    put(sx_i + 5, sy_i - 5, rgb(240, 236, 220), out);
    put(sx_i + 4, sy_i - 5, rgb(230, 190, 150), out);
    draw_triangle_filled(
        sx_i + 0,
        sy_i - 7,
        sx_i + 4,
        sy_i - 7,
        sx_i + 2,
        sy_i - 10,
        rgb(200, 32, 34),
        out,
    );
    put(sx_i + 4, sy_i - 7, rgb(240, 236, 220), out);
    // Sack trailing behind the backrest.
    ellipse(sx as f32 - 6.0, sy - 1.0, 2.6, 2.2, rgb(110, 74, 36), out);

    // Stardust trail behind the sleigh.
    for i in 0..12 {
        let age = i as f32 * 0.05;
        let px = (sx - 4.0 - age * 55.0) as i32;
        let py = (sy + 2.0 + sinp(t * 3.0 + i as f32) * 2.0) as i32;
        let b = (1.0 - i as f32 / 12.0) as f32;
        add(px, py, rgb(255, 226, 140), b * 0.8, out);
    }

    snow_layer(t, 31, 20, 2.5, 1.0, out);
}

/// A frost fairy trailing ice-light through a frozen glade.
pub fn frost_fairies(ms: u32, out: &mut BufferTargetRgb888) {
    let t = ms as f32 / 1000.0;

    sky(rgb(2, 7, 24), rgb(10, 32, 52), out);
    stars(t, 13, 55, 30, out);

    // A pale moon behind the glade.
    glow(48.0, 9.0, 14.0, rgb(120, 170, 210), 0.4, out);
    draw_circle_filled(48, 9, 4, rgb(210, 236, 244), out);

    // Frozen ferns in the corners: branching pale-blue lines.
    for (fx, fy, s) in [(3, 40, 1), (57, 40, -1)] {
        for k in 0..6 {
            let ex = fx + s * (2 + k * 2);
            let ey = fy - 6 - k * 3;
            draw_line(fx, fy - 2, ex, ey, rgb(70, 140, 160), out);
            draw_line(ex, ey, ex + s * 3, ey - 1, rgb(100, 180, 200), out);
            draw_line(ex, ey, ex - s * 2, ey - 3, rgb(100, 180, 200), out);
        }
    }

    // Hero fairy: a bright spiral trail and two flapping wings.
    let hero = |tt: f32| {
        let x = 30.0 + sinp(tt * 0.51 + 0.4) * 18.0;
        let y = 18.0 + sinp(tt * 0.73 + 1.7) * 9.0;
        (x, y)
    };

    for i in (0..26).rev() {
        let age = i as f32 * 0.05;
        let (tx, ty) = hero(t - age);
        let f = 1.0 - i as f32 / 26.0;
        let c = if i % 3 == 0 {
            rgb(210, 245, 255)
        } else if i % 3 == 1 {
            rgb(120, 210, 255)
        } else {
            rgb(220, 160, 255)
        };

        add(tx as i32, ty as i32, c, f * 0.9, out);

        if i % 4 == 0 {
            glow(tx, ty, 2.2, c, f * 0.5, out);
        }

        if i % 5 == 0 {
            sparkle(
                tx as i32 + 2,
                ty as i32 - 1,
                t * 2.0 + i as f32,
                rgb(240, 250, 255),
                out,
            );
        }
    }

    let (fx, fy) = hero(t);
    let flap = sinp(t * 13.0) * 0.5 + 0.5;

    glow(fx, fy, 8.0, rgb(140, 220, 255), 0.55, out);
    // Wings.
    draw_triangle_filled(
        (fx + 1.0) as i32,
        (fy - 1.0) as i32,
        (fx + 5.0) as i32,
        (fy - 3.0 - flap * 3.0) as i32,
        (fx + 4.0) as i32,
        (fy + 1.0) as i32,
        rgb(140, 220, 255),
        out,
    );
    draw_triangle_filled(
        (fx + 1.0) as i32,
        (fy + 1.0) as i32,
        (fx + 5.0) as i32,
        (fy + 3.0 + flap * 2.0) as i32,
        (fx + 3.0) as i32,
        (fy + 2.0) as i32,
        rgb(210, 160, 255),
        out,
    );
    put(
        (fx + 4.0) as i32,
        (fy - 2.0 - flap * 2.0) as i32,
        rgb(250, 255, 255),
        out,
    );
    // Body, head and wand.
    ellipse(fx, fy, 0.9, 2.0, rgb(245, 252, 255), out);
    put(fx as i32, (fy - 2.0) as i32, rgb(255, 255, 255), out);
    let wand = sinp(t * 2.1) * 1.2;
    draw_line(
        (fx + 2.0) as i32,
        (fy + 1.0) as i32,
        (fx + 5.0) as i32,
        (fy + 3.0 + wand) as i32,
        rgb(220, 240, 255),
        out,
    );
    sparkle(
        (fx + 5.0) as i32,
        (fy + 3.0 + wand) as i32,
        t * 3.0,
        rgb(255, 255, 255),
        out,
    );

    // Two smaller fairies drifting on wider paths.
    for f in 0..2 {
        let speed = 0.4 + f as f32 * 0.15;
        let path = |tt: f32| {
            let x = 30.0 + sinp(tt * 0.42 + 3.0 + f as f32 * 1.8) * (22.0 + f as f32 * 2.0);
            let y = 16.0 + sinp(tt * 0.65 + 0.8 + f as f32 * 1.4) * (11.0 - f as f32 * 2.0);
            (x, y)
        };

        for i in (0..12).rev() {
            let (tx, ty) = path(t - i as f32 * 0.06 * speed);
            let b = (1.0 - i as f32 / 12.0) * 0.5;
            add(tx as i32, ty as i32, rgb(150, 220, 255), b, out);
        }

        let (sx, sy) = path(t * speed);

        glow(sx, sy, 4.0, rgb(140, 210, 255), 0.5, out);
        put(sx as i32, sy as i32, rgb(255, 255, 255), out);
        put(sx as i32 + 1, sy as i32 - 1, rgb(210, 160, 255), out);
        put(sx as i32 + 1, sy as i32 + 1, rgb(140, 220, 255), out);
    }

    // Frost crystals blooming on the ground.
    for (cx, cy, r) in [(10, 35, 3), (49, 33, 3), (24, 37, 2), (39, 38, 2)] {
        for k in 0..6 {
            let a = k as f32 * PI / 3.0 + t * 0.15;
            let ex = cx + (a.cos() * r as f32) as i32;
            let ey = cy + (a.sin() * r as f32) as i32;

            draw_line(cx, cy, ex, ey, rgb(150, 220, 236), out);
            put(ex, ey, rgb(220, 250, 255), out);
        }

        sparkle(cx, cy, t * 1.1 + cx as f32, rgb(220, 245, 255), out);
    }
}

/// A snow globe: a cottage inside, shaken snow swirling, glass glints and a
/// warm wooden base.
pub fn snow_globe(ms: u32, out: &mut BufferTargetRgb888) {
    let t = ms as f32 / 1000.0;

    sky(rgb(18, 8, 30), rgb(46, 18, 54), out);
    stars(t, 27, 35, 34, out);

    let cx = 30.0;
    let cy = 18.0;
    let r = 14.0;
    let shake = (t * 1.4) % 9.0;
    let energy = (1.0 - shake / 5.0).max(0.0);
    let wobble = sinp(t * 2.4) * energy * 1.2;

    glow(cx + wobble, cy, r * 1.45, rgb(90, 130, 190), 0.4, out);
    draw_circle_filled((cx + wobble) as i32, cy as i32, r as i32, rgb(14, 28, 54), out);
    draw_circle((cx + wobble) as i32, cy as i32, r as i32, rgb(120, 170, 200), out);

    // Inside the globe: cottage, trees, snow banks.
    let ix = (cx + wobble) as i32;
    draw_rect_filled(ix - 7, 16, 9, 8, rgb(52, 38, 38), out);
    draw_triangle_filled(ix - 9, 16, ix + 3, 16, ix - 3, 10, rgb(230, 236, 240), out);
    draw_rect_filled(ix - 5, 18, 2, 2, rgb(255, 190, 90), out);
    draw_rect_filled(ix - 1, 18, 2, 2, rgb(255, 160, 70), out);
    draw_rect_filled(ix - 4, 22, 3, 2, rgb(30, 24, 26), out);
    pine(ix + 8, 25, 7, rgb(20, 58, 44), out);
    pine(ix + 12, 25, 5, rgb(26, 70, 52), out);
    hline(ix - 12, ix + 12, 25, rgb(210, 226, 236), out);
    hline(ix - 10, ix + 10, 26, rgb(170, 196, 214), out);

    // Swirling snow inside the globe. When the shake energy dies down the
    // flakes settle into a drift at the bottom.
    for i in 0..38 {
        let h = fnv(i, 111);
        let base_x = (h % 27) as f32 - 13.0;
        let base_y = ((h >> 8) % 24) as f32 - 12.0;
        let swirl = t * (2.6 + energy * 7.0) + i as f32 * 0.7;
        let dx = base_x + sinp(swirl) * (1.5 + energy * 3.0);
        let dy = base_y + (shake * (3.0 + energy * 5.0) + (h >> 16) as f32 * 0.001) % 24.0 - 12.0;
        let px = ix + dx as i32;
        let py = (cy as i32) + dy as i32;

        if ((px - ix) * (px - ix) + (py - cy as i32) * (py - cy as i32)) < (r as i32 - 1) * (r as i32 - 1) {
            put(px, py, rgb(235, 246, 255), out);
            if i % 5 == 0 {
                add(px + 1, py, rgb(180, 215, 255), 0.5, out);
            }
        }
    }

    // Glass reflections: a bright rim arc and a soft highlight.
    for a in 0..20 {
        let ang = -2.6 + a as f32 * 0.055;
        over(
            (cx + wobble + ang.cos() * r) as i32,
            (cy + ang.sin() * r) as i32,
            rgb(230, 245, 255),
            0.7,
            out,
        );
    }
    over(ix - 7, 10, rgb(255, 255, 255), 0.65, out);
    over(ix - 8, 11, rgb(220, 240, 255), 0.35, out);
    over(ix - 6, 10, rgb(220, 240, 255), 0.35, out);

    // Base.
    draw_rect_filled(ix - 10, 32, 21, 2, rgb(170, 120, 62), out);
    draw_rect_filled(ix - 9, 34, 19, 4, rgb(112, 72, 40), out);
    draw_rect_filled(ix - 9, 35, 19, 1, rgb(210, 168, 80), out);
    draw_rect_filled(ix - 7, 37, 15, 1, rgb(64, 40, 24), out);
    add(ix - 9, 34, rgb(255, 210, 120), 0.35, out);

    // Table bokeh.
    for i in 0..6 {
        let h = fnv(i, 117);
        let bx = ((h % 44) as i32) - 22;
        let by = 31 + ((h >> 8) % 5) as i32;
        if bx < -10 || bx > 10 {
            glow(cx + bx as f32, by as f32, 2.2, rgb(180, 120, 80), 0.35, out);
        }
    }
}

/// Three penguins on the ice: two waddle, one belly-slides through the
/// middle, under a ribbon of southern aurora.
pub fn penguins(ms: u32, out: &mut BufferTargetRgb888) {
    let t = ms as f32 / 1000.0;

    sky(rgb(4, 10, 28), rgb(24, 44, 72), out);
    stars(t, 43, 40, 20, out);

    // A simple aurora wash near the top.
    for layer in 0..2 {
        for x in 0..WIDTH as i32 {
            let wave = sinp(x as f32 * 0.09 + t * (0.35 + layer as f32 * 0.2)) * 4.0;
            let top = 3.0 + layer as f32 * 3.0 + wave;
            for y in (top as i32)..(top as i32 + 8) {
                let f = (y as f32 - top) / 8.0;
                let a = (1.0 - f).max(0.0) * 0.16;
                add(x, y, hsv_to_rgb(0.38 + f * 0.25, 0.8, 0.7), a, out);
            }
        }
    }

    // Ice shelf and water.
    for y in 27usize..HEIGHT {
        for x in 0..WIDTH {
            let f = (y - 27) as f32 / (HEIGHT - 27) as f32;
            let ripple = 0.5 + 0.5 * sinp(x as f32 * 0.4 + t * 1.5 + y as f32);
            out.buffer[y * WIDTH + x] = Rgb888::new(
                (28.0 + 24.0 * f + 8.0 * ripple) as u8,
                (70.0 + 46.0 * f + 10.0 * ripple) as u8,
                (110.0 + 46.0 * f + 14.0 * ripple) as u8,
            );
        }
    }
    draw_rect_filled(0, 25, 60, 4, rgb(206, 226, 238), out);
    draw_rect_filled(0, 29, 60, 2, rgb(158, 194, 214), out);

    // Penguin helper.
    let penguin = |x: f32, y: f32, s: f32, waddle: f32, out: &mut BufferTargetRgb888| {
        let bx = x + waddle * 0.8;
        ellipse(bx, y, 3.6 * s, 5.6 * s, rgb(12, 16, 24), out);
        ellipse(bx + 0.4 * s, y + 0.8 * s, 2.4 * s, 4.0 * s, rgb(232, 236, 240), out);
        ellipse(bx, y - 4.2 * s, 2.7 * s, 2.7 * s, rgb(12, 16, 24), out);
        // Face patch and beak.
        ellipse(bx + 0.4 * s, y - 4.4 * s, 1.8 * s, 1.8 * s, rgb(232, 236, 240), out);
        put((bx - 0.8 * s) as i32, (y - 4.6 * s) as i32, rgb(255, 170, 40), out);
        put((bx - 0.2 * s) as i32, (y - 5.4 * s) as i32, rgb(12, 12, 16), out);
        put((bx - 1.2 * s) as i32, (y - 5.4 * s) as i32, rgb(255, 150, 30), out);
        // Feet.
        put((bx - 1.8 * s) as i32, (y + 5.4 * s) as i32, rgb(255, 150, 40), out);
        put((bx + 1.6 * s) as i32, (y + 5.4 * s) as i32, rgb(255, 150, 40), out);
    };

    let w1 = sinp(t * 3.2) * 0.5;
    let w2 = sinp(t * 3.2 + 2.7) * 0.5;
    penguin(14.0, 22.0, 1.0, w1, out);
    penguin(24.0, 23.0, 0.8, w2, out);

    // Sliding penguin crosses the foreground on its belly.
    let slide_x = 40.0 + sinp(t * 0.8) * 14.0;
    let slide_y = 28.0;
    ellipse(slide_x, slide_y, 5.0, 2.2, rgb(12, 16, 24), out);
    ellipse(slide_x + 0.5, slide_y + 0.4, 3.4, 1.2, rgb(232, 236, 240), out);
    ellipse(slide_x + 5.0, slide_y - 1.0, 2.0, 2.0, rgb(12, 16, 24), out);
    put((slide_x + 6.0) as i32, (slide_y - 1.0) as i32, rgb(255, 160, 40), out);
    put((slide_x + 5.0) as i32, (slide_y - 2.0) as i32, rgb(12, 12, 16), out);
    // Flippers back and snow spray.
    draw_line(
        (slide_x - 2.0) as i32,
        (slide_y - 1.0) as i32,
        (slide_x - 5.0) as i32,
        (slide_y - 3.0) as i32,
        rgb(20, 30, 40),
        out,
    );
    for k in 0..4 {
        add(
            (slide_x - 6.0 - k as f32 * 2.0) as i32,
            (slide_y + 1.0 - k as f32 * 0.5) as i32,
            rgb(220, 240, 255),
            0.4,
            out,
        );
    }

    // Ice sparkle.
    for i in 0..12 {
        let h = fnv(i, 121);
        sparkle(
            (h % 60) as i32,
            25 + ((h >> 8) % 4) as i32,
            t * 1.7 + i as f32,
            rgb(220, 245, 255),
            out,
        );
    }
}

/// A reindeer leaping a snow bank in the last of the daylight.
pub fn reindeer(ms: u32, out: &mut BufferTargetRgb888) {
    let t = ms as f32 / 1000.0;

    sky(rgb(24, 14, 46), rgb(206, 122, 68), out);
    glow(46.0, 24.0, 20.0, rgb(255, 188, 110), 0.45, out);
    draw_circle_filled(46, 24, 5, rgb(255, 218, 150), out);

    for x in (0..WIDTH as i32).step_by(10) {
        pine(x, 34, 10 + ((x * 3) % 6), rgb(26, 18, 30), out);
    }
    // Snow bank.
    for x in 0..WIDTH as i32 {
        let h = 4 + (sinp(x as f32 * 0.12 + 1.0) * 2.0) as i32;
        for y in (33 - h)..H {
            put(x, y, rgb(198, 206, 224), out);
        }
        for y in (35 - h / 2)..H {
            put(x, y, rgb(168, 180, 210), out);
        }
    }

    // Leap cycle: the deer hangs at the top of the arc.
    let phase = (t % 4.2) / 4.2;
    let leap = sinp(phase * PI);
    let cx = -8.0 + phase * (W as f32 + 18.0);
    let cy = 25.0 - leap * 9.0 + sinp(t * 4.0) * 0.3;

    // Body and neck. Dark enough to stand against the sunset, with a warm
    // rim so the silhouette still reads on the panel.
    ellipse(cx, cy, 7.0, 3.4, rgb(54, 30, 20), out);
    ellipse(cx + 1.0, cy - 1.0, 5.5, 1.8, rgb(150, 90, 40), out);
    draw_line(
        (cx - 5.0) as i32,
        (cy - 3.2) as i32,
        (cx + 5.0) as i32,
        (cy - 3.2) as i32,
        rgb(255, 196, 110),
        out,
    );
    // Neck and head.
    for k in 0..2 {
        draw_line(
            (cx + 4.0) as i32 + k,
            (cy - 1.0) as i32,
            (cx + 8.0) as i32 + k,
            (cy - 6.0) as i32,
            rgb(54, 30, 20),
            out,
        );
    }
    ellipse(cx + 9.0, cy - 7.0, 2.8, 2.0, rgb(66, 38, 22), out);
    draw_triangle_filled(
        (cx + 11.0) as i32,
        (cy - 8.0) as i32,
        (cx + 14.0) as i32,
        (cy - 7.0) as i32,
        (cx + 11.0) as i32,
        (cy - 6.0) as i32,
        rgb(80, 48, 30),
        out,
    );
    // Antlers: a clear branching rack, lighter than the body.
    let ax = (cx + 9.0) as i32;
    let ay = (cy - 9.0) as i32;
    for (dx, dy) in [(0, -2), (1, -4), (2, -5), (-1, -4), (3, -3), (2, -7)] {
        draw_line(ax, ay, ax + dx, ay + dy, rgb(174, 124, 58), out);
    }
    for (dx, dy) in [(1, -4), (-1, -4)] {
        draw_line(ax + dx, ay + dy, ax + dx + 2, ay + dy - 2, rgb(174, 124, 58), out);
    }
    put(ax, ay, rgb(255, 210, 140), out);
    // Red scarf fluttering back from the neck.
    let scarf = sinp(t * 3.0) * 1.5;
    draw_line(
        (cx + 6.0) as i32,
        (cy - 3.0) as i32,
        (cx + 1.0) as i32,
        (cy - 4.0 + scarf) as i32,
        rgb(255, 60, 60),
        out,
    );
    draw_line(
        (cx + 1.0) as i32,
        (cy - 4.0 + scarf) as i32,
        (cx - 3.0) as i32,
        (cy - 3.0 + scarf) as i32,
        rgb(255, 105, 80),
        out,
    );
    // Legs stretch with the leap; two-pixel strokes keep them visible.
    for (x0, x1, y1) in [
        (cx - 5.0, cx - 9.0 + leap * 2.0, cy + 7.0),
        (cx - 2.0, cx - 3.0 - leap * 2.0, cy + 8.0),
        (cx + 3.0, cx + 8.0 - leap, cy + 6.0),
    ] {
        draw_line(x0 as i32, (cy + 2.0) as i32, x1 as i32, y1 as i32, rgb(58, 32, 20), out);
        draw_line(
            x0 as i32 + 1,
            (cy + 2.0) as i32,
            x1 as i32 + 1,
            y1 as i32,
            rgb(76, 42, 24),
            out,
        );
        put(x1 as i32, y1 as i32, rgb(35, 20, 14), out);
    }
    // Tail and eye.
    put((cx - 7.0) as i32, (cy - 1.0) as i32, rgb(230, 220, 200), out);
    put((cx + 10.0) as i32, (cy - 7.0) as i32, rgb(20, 12, 10), out);
    put((cx + 11.0) as i32, (cy - 8.0) as i32, rgb(255, 220, 150), out);

    // A fawn trails behind, a little lower and softer.
    let fx = cx - 17.0;
    let fy = cy + 3.0;
    ellipse(fx, fy, 4.0, 2.2, rgb(100, 66, 42), out);
    draw_line(
        (fx + 3.0) as i32,
        (fy - 1.0) as i32,
        (fx + 6.0) as i32,
        (fy - 4.0) as i32,
        rgb(100, 66, 42),
        out,
    );
    ellipse(fx + 6.0, fy - 4.5, 1.8, 1.4, rgb(112, 74, 46), out);
    for (dx, dy) in [(-1, -3), (1, -4), (2, -2)] {
        draw_line(
            (fx + 6.0) as i32,
            (fy - 5.0) as i32,
            (fx + 6.0 + dx as f32) as i32,
            (fy - 5.0 + dy as f32) as i32,
            rgb(120, 82, 50),
            out,
        );
    }
    for (x0, y_off) in [(fx - 2.0, 3.0), (fx + 2.0, 4.0)] {
        draw_line(
            x0 as i32,
            (fy + 1.0) as i32,
            x0 as i32,
            (fy + y_off) as i32,
            rgb(96, 60, 38),
            out,
        );
    }
    put((fx + 7.0) as i32, (fy - 5.0) as i32, rgb(30, 18, 12), out);

    // Rim light and kicked snow.
    for x in 0..6 {
        put((cx - 5.0) as i32 + x, (cy - 3.0) as i32, rgb(255, 180, 100), out);
    }
    put(ax, ay, rgb(255, 210, 140), out);
    for i in 0..10 {
        let h = fnv(i, 131);
        let x = (cx - 11.0) as i32 + (h % 16) as i32;
        let y = 32 + ((h >> 8) % 3) as i32;
        add(x, y, rgb(240, 244, 255), 0.55, out);
    }

    snow_layer(t, 37, 22, 3.0, -1.0, out);
    for k in 0..6 {
        sparkle(
            (8 + k * 9) as i32,
            6 + (k % 3) * 3,
            t * 1.5 + k as f32,
            rgb(255, 220, 160),
            out,
        );
    }
}

// ── Northern Lights ────────────────────────────────────────────────────────

/// The aurora centrepiece: layered curtains with vertical rays, a mountain
/// silhouette and a rippling reflection.
pub fn aurora(ms: u32, out: &mut BufferTargetRgb888) {
    let t = ms as f32 / 1000.0;

    sky(rgb(1, 3, 12), rgb(6, 10, 28), out);
    stars(t, 7, 85, 27, out);

    // Faint Milky Way dust, leaning across the top-left.
    for i in 0..110 {
        let h = fnv(i, 149);
        let x = (h % 60) as f32;
        let y = 28.0 - x * 0.38 + ((h >> 8) % 12) as f32 - 6.0;
        add(x as i32, y as i32, rgb(70, 76, 110), 0.25, out);
    }

    // Three curtains. Each column is a ray whose brightness and vertical
    // extent change with x, so the ribbon keeps folding instead of sliding.
    for layer in 0..3 {
        let speed = 0.28 + layer as f32 * 0.18;
        let hue_base = 0.31 + layer as f32 * 0.075;

        for x in 0..WIDTH as i32 {
            let xf = x as f32;
            let wave = sinp(xf * 0.115 + t * speed) + 0.6 * sinp(xf * 0.31 - t * speed * 1.5);
            let top = 3.0 + layer as f32 * 2.4 + wave * 4.0;
            let len = 20.0 + layer as f32 * 3.0 + 5.0 * sinp(xf * 0.08 + t * 0.55 + layer as f32);
            let ray = 0.50 + 0.50 * sinp(xf * 1.05 + t * 2.1 + layer as f32 * 1.8);
            let fold = 0.65 + 0.35 * sinp(xf * 0.22 - t * 1.1);
            let brightness = (0.46 - layer as f32 * 0.07) * ray * fold;

            let y0 = top as i32;
            let y1 = (top + len) as i32;

            for y in y0..y1 {
                if y < 0 || y >= 30 {
                    continue;
                }

                let f = (y as f32 - top) / len;
                let v = brightness * (1.0 - f * 0.72);

                if v > 0.015 {
                    let hue = hue_base + f * 0.28 + 0.02 * sinp(t * 0.6 + xf * 0.13);
                    let c = hsv_to_rgb(hue, 0.82, v);
                    add(x, y, c, 1.0, out);
                }
            }

            // A brighter hem on the leading edge sells the 3D fold.
            let hem = (top + len * 0.08) as i32;
            add(x, hem, hsv_to_rgb(hue_base + 0.04, 0.35, brightness * 1.4), 1.0, out);
        }
    }

    // Mountains and one foreground pine ridge.
    for x in 0..WIDTH as i32 {
        let xf = x as f32;
        let h = 5.0 + sinp(xf * 0.14 + 1.7) * 3.0 + sinp(xf * 0.33 + 4.0) * 1.5;
        let top = 31 - h as i32;

        for y in top..H {
            let f = (y - top) as f32 / (H - top) as f32;
            put(
                x,
                y,
                Rgb888::new((8.0 - f * 4.0) as u8, (12.0 - f * 5.0) as u8, (20.0 - f * 7.0) as u8),
                out,
            );
        }
    }
    for x in (0..WIDTH as i32).step_by(7) {
        pine(x, 40, 8 + ((x * 11) % 6), rgb(2, 6, 8), out);
    }

    // Aurora reflected in the water at the bottom.
    for y in 35..H {
        let f = (y - 35) as f32 / 5.0;
        for x in 0..WIDTH as i32 {
            let shimmer = 0.5 + 0.5 * sinp(x as f32 * 0.55 + t * 2.2 + y as f32 * 1.7);
            let a = (0.18 - f * 0.10) * shimmer;
            add(x, y, hsv_to_rgb(0.42, 0.8, 0.7), a, out);
        }
    }

    // Owl-light / shooting star.
    let phase = t % 7.0;
    if phase < 0.9 {
        let p = phase / 0.9;
        for k in 0..7 {
            let x = (-4.0 + p * 70.0 - k as f32 * 1.4) as i32;
            let y = (4.0 + p * 12.0 - k as f32 * 0.3) as i32;
            add(x, y, rgb(200, 220, 255), (1.0 - k as f32 / 7.0) * 0.8, out);
        }
    }
}

/// A neon electric guitar on a dark stage — strings shimmer, lightning
/// crackles, and a small buzzcock circles the spotlight.
pub fn electric_guitar(ms: u32, out: &mut BufferTargetRgb888) {
    let t = ms as f32 / 1000.0;

    sky(rgb(5, 1, 14), rgb(26, 5, 32), out);

    // Spotlight cones from the top corners.
    for sx in [6.0, 54.0] {
        for y in 0..34 {
            let x = sx + (y as f32 - 24.0) * 0.18;
            let spread = 2 + y / 12;

            for dx in -spread..=spread {
                add(
                    x as i32 + dx,
                    y,
                    rgb(120, 70, 180),
                    0.035 + 0.02 * sinp(t * 0.7 + y as f32 * 0.1),
                    out,
                );
            }
        }
    }

    // Equalizer skyline behind the stage.
    for i in 0..10 {
        let h = (3.0 + 8.0 * (sinp(t * 2.0 + i as f32 * 0.9) * 0.5 + 0.5)) as i32;
        let x = 2 + i * 6;
        let c = hsv_to_rgb(0.72 + i as f32 * 0.015, 0.8, 0.6);

        for y in (40 - h)..40 {
            add(x, y, c, 0.18, out);
            add(x + 1, y, c, 0.1, out);
        }
    }

    // Lightning bolts in the background.
    for k in 0..2 {
        let h = fnv((t * 2.5) as u32 + k * 7, 157);
        let lx = 8 + (h % 44) as i32;

        if ((h >> 8) % 5) == 0 {
            let mut x = lx;
            let mut y = 1;

            for _ in 0..8 {
                let nx = x + (((h >> 12) % 5) as i32 - 2);
                draw_line(x, y, nx, y + 3, rgb(190, 220, 255), out);
                add(nx, y + 3, rgb(120, 160, 255), 0.7, out);
                x = nx;
                y += 3;
            }
        }
    }

    // Guitar body: an asymmetric double-bout outline (dark edge, hot fill)
    // with a cutaway waist. A polygon reads far better than stacked ellipses
    // at this size — the silhouette is the whole instrument.
    let body = [
        (27, 35),
        (22, 34),
        (19, 31),
        (20, 27),
        (23, 26),
        (24, 22),
        (27, 19),
        (31, 19),
        (34, 21),
        (34, 24),
        (31, 26),
        (33, 29),
        (32, 33),
    ];
    let inner = [
        (27, 34),
        (23, 33),
        (21, 30),
        (22, 28),
        (25, 26),
        (25, 22),
        (28, 21),
        (30, 21),
        (32, 23),
        (32, 25),
        (29, 27),
        (31, 29),
        (30, 32),
    ];

    polygon(&body, rgb(70, 8, 34), out);
    polygon(&inner, rgb(212, 30, 92), out);
    // Bevel and rim highlights.
    ellipse(23.5, 28.0, 2.6, 4.0, rgb(210, 44, 90), out);
    ellipse(28.5, 22.0, 2.2, 1.8, rgb(220, 60, 104), out);
    draw_line(20, 28, 22, 32, rgb(255, 110, 150), out);
    draw_line(25, 21, 29, 21, rgb(255, 130, 162), out);
    // Soundhole, pickguard and bridge.
    draw_circle_filled(27, 26, 2, rgb(22, 6, 18), out);
    draw_circle(27, 26, 2, rgb(230, 190, 100), out);
    ellipse(23.5, 30.0, 2.6, 2.0, rgb(230, 188, 90), out);
    draw_rect_filled(25, 31, 5, 1, rgb(60, 40, 30), out);

    // Neck, frets and headstock.
    let nx0 = 32.0;
    let ny0 = 20.0;
    let nx1 = 49.0;
    let ny1 = 5.0;

    for k in -1..=1 {
        draw_line(
            nx0 as i32 + k,
            ny0 as i32,
            nx1 as i32 + k,
            ny1 as i32,
            rgb(96, 58, 30),
            out,
        );
    }
    draw_line(
        nx0 as i32 + 2,
        ny0 as i32,
        nx1 as i32 + 2,
        ny1 as i32,
        rgb(56, 34, 20),
        out,
    );
    draw_line(
        nx0 as i32 - 2,
        ny0 as i32,
        nx1 as i32 - 2,
        ny1 as i32,
        rgb(210, 150, 80),
        out,
    );
    for f in 1..9 {
        let p = f as f32 / 9.0;
        let x = (nx0 + (nx1 - nx0) * p) as i32;
        let y = (ny0 + (ny1 - ny0) * p) as i32;
        draw_line(x - 1, y + 1, x + 1, y - 1, rgb(130, 110, 88), out);
    }
    draw_rect_filled(47, 2, 6, 5, rgb(28, 16, 28), out);
    for k in 0..3 {
        put(48 + k * 2, 2, rgb(220, 210, 180), out);
    }

    // Strings vibrate across the body. Keep them thin and cool-grey — a
    // white bundle turns the neck into a plank at this size.
    let off = (sinp(t * 14.0) * 1.3) as i32;

    for k in 0..2 {
        draw_line(26 + k, 31, 48 + k, 5, rgb(120, 180, 210), out);
        draw_line(26 + k, 31, 33 + k, 20 + off, rgb(100, 150, 180), out);
    }
    draw_rect_filled(25, 30, 5, 1, rgb(230, 188, 90), out);

    // Neon glow around the body.
    glow(27.0, 25.0, 17.0, rgb(255, 40, 120), 0.28, out);

    // Music notes floating up from the soundhole.
    for i in 0..4 {
        let phase = (t * 0.45 + i as f32 * 0.31) % 1.0;
        let x = 8 + i * 12 + (sinp(t * 1.3 + i as f32) * 2.0) as i32;
        let y = 33 - (phase * 26.0) as i32;

        put(x, y, rgb(180, 240, 255), out);
        put(x + 1, y, rgb(120, 200, 255), out);
        put(x + 1, y - 1, rgb(200, 250, 255), out);
        add(x, y + 1, rgb(120, 200, 255), 0.4, out);
    }

    // The Buzzcock: a striped wasp with amber wings, orbiting the guitar.
    let a = t * 1.5;
    let wx = 30.0 + a.cos() * 22.0;
    let wy = 15.0 + a.sin() * 8.0;

    for k in 0..4 {
        put(
            (wx - k as f32) as i32,
            wy as i32,
            if k % 2 == 1 { rgb(20, 16, 18) } else { rgb(255, 200, 30) },
            out,
        );
    }
    draw_line(
        (wx + 1.0) as i32,
        (wy - 1.0) as i32,
        (wx + 3.0) as i32,
        (wy - 3.0) as i32,
        rgb(210, 235, 255),
        out,
    );
    draw_line(
        (wx + 1.0) as i32,
        (wy + 1.0) as i32,
        (wx + 3.0) as i32,
        (wy + 2.0) as i32,
        rgb(210, 235, 255),
        out,
    );
    put((wx - 4.0) as i32, wy as i32, rgb(20, 16, 18), out);
    for k in 1..7 {
        add(
            (wx - 4.0 - k as f32 * 1.5) as i32,
            (wy + sinp(t * 8.0 + k as f32) * 1.5) as i32,
            rgb(255, 180, 30),
            0.3,
            out,
        );
    }
}

/// Two records spinning under club lights, a tonearm, and neon music notes.
pub fn vinyl_records(ms: u32, out: &mut BufferTargetRgb888) {
    let t = ms as f32 / 1000.0;

    sky(rgb(8, 4, 18), rgb(34, 8, 38), out);

    // Club light beams.
    for k in 0..4 {
        let a = t * 0.4 + k as f32 * 1.1;
        let x = 30.0 + (a.cos() * 28.0) as f32;
        for y in 0..40 {
            let xx = x + (y as f32 - 20.0) * 0.3;
            add(xx as i32, y, hsv_to_rgb(0.75 + k as f32 * 0.05, 0.8, 0.5), 0.06, out);
        }
    }

    let record = |cx: f32, cy: f32, r: f32, rot: f32, label: Rgb888, out: &mut BufferTargetRgb888| {
        glow(cx, cy, r * 1.35, rgb(80, 30, 100), 0.35, out);
        draw_circle_filled(cx as i32, cy as i32, r as i32, rgb(10, 10, 14), out);

        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                let dx = x as f32 - cx;
                let dy = y as f32 - cy;
                let d = (dx * dx + dy * dy).sqrt();

                if d > r || d < r * 0.34 {
                    continue;
                }

                let angle = dy.atan2(dx);
                let groove = 0.5 + 0.5 * sinp(d * 3.0);
                let gloss = (cosp(angle - rot) * 0.5 + 0.5).powi(6);
                let v = 10.0 + groove * 9.0 + gloss * 42.0;
                let idx = y * WIDTH + x;

                out.buffer[idx] = Rgb888::new((v * 0.8) as u8, (v * 0.82) as u8, v as u8);
            }
        }

        // Label and spindle.
        draw_circle_filled(cx as i32, cy as i32, (r * 0.34) as i32, label, out);
        draw_circle_filled(cx as i32, cy as i32, 1, rgb(10, 10, 10), out);
        let lx = (cx + rot.cos() * r * 0.22) as i32;
        let ly = (cy + rot.sin() * r * 0.22) as i32;
        put(lx, ly, rgb(255, 236, 190), out);
        // Rim light.
        draw_circle(cx as i32, cy as i32, r as i32, rgb(80, 180, 255), out);
    };

    record(24.0, 18.0, 16.0, t * 2.2, rgb(214, 54, 60), out);
    record(48.0, 27.0, 9.0, -t * 2.8, rgb(240, 150, 40), out);

    // Tonearm reaching toward the big record.
    draw_line(4, 2, 12, 9, rgb(180, 190, 200), out);
    draw_line(12, 9, 12, 13, rgb(180, 190, 200), out);
    draw_circle_filled(12, 13, 1, rgb(220, 230, 240), out);

    // Notes and sparkle.
    for i in 0..5 {
        let phase = (t * 0.4 + i as f32 * 0.21) % 1.0;
        let x = 8 + i * 10 + (sinp(t + i as f32) * 2.0) as i32;
        let y = 37 - (phase * 30.0) as i32;
        put(x, y, rgb(255, 120, 200), out);
        put(x + 1, y, rgb(255, 160, 220), out);
        put(x + 1, y - 1, rgb(255, 210, 240), out);
    }
}

/// A sprinter crossing the line with a gold medal and confetti — the Keely
/// moment from the brief.
pub fn track_champion(ms: u32, out: &mut BufferTargetRgb888) {
    let t = ms as f32 / 1000.0;

    // Stadium night.
    sky(rgb(10, 14, 38), rgb(46, 44, 78), out);

    // Floodlights and crowd bokeh.
    for (x, y) in [(8, 5), (52, 5)] {
        glow(x as f32, y as f32, 10.0, rgb(210, 220, 255), 0.4, out);
        draw_circle_filled(x, y, 2, rgb(255, 255, 220), out);
        for k in 0..6 {
            add(x - 2 + k, y + 2, rgb(160, 180, 255), 0.25, out);
        }
    }
    for i in 0..26 {
        let h = fnv(i, 163);
        let x = (h % 60) as i32;
        let y = 8 + ((h >> 8) % 7) as i32;
        let c = hsv_to_rgb(
            (i as f32 * 0.11) % 1.0,
            0.4,
            0.5 + 0.4 * (sinp(t * 2.0 + i as f32) * 0.5 + 0.5),
        );
        add(x, y, c, 0.5, out);
    }

    // Track lanes: warm terracotta with a finish line.
    for y in 28..40 {
        let f = (y - 28) as f32 / 12.0;
        for x in 0..WIDTH {
            out.buffer[y * WIDTH + x] = Rgb888::new(
                (150.0 - f * 70.0) as u8,
                (58.0 - f * 28.0) as u8,
                (48.0 - f * 22.0) as u8,
            );
        }
    }
    for x in 0..WIDTH as i32 {
        hline(x, x, 28, rgb(230, 230, 220), out);
        hline(x, x, 31, rgb(210, 210, 200), out);
    }
    for y in 28..40 {
        for x in 42..45 {
            if ((x + y) & 1) == 0 {
                put(x, y, rgb(240, 240, 230), out);
            }
        }
    }

    // Finish line: two posts and a red ribbon across the track.
    draw_rect_filled(45, 19, 1, 9, rgb(210, 214, 220), out);
    draw_rect_filled(54, 19, 1, 9, rgb(210, 214, 220), out);
    draw_line(45, 22, 54, 22, rgb(230, 54, 60), out);
    draw_line(45, 23, 54, 23, rgb(190, 36, 46), out);

    // Scoreboard banner: KEELY, in a tiny 3x5 pixel font.
    let letters = [
        [0b101, 0b101, 0b110, 0b101, 0b101], // K
        [0b111, 0b100, 0b110, 0b100, 0b111], // E
        [0b111, 0b100, 0b110, 0b100, 0b111], // E
        [0b100, 0b100, 0b100, 0b100, 0b111], // L
        [0b101, 0b101, 0b010, 0b010, 0b010], // Y
    ];

    for (li, rows) in letters.iter().enumerate() {
        for (ry, row) in rows.iter().enumerate() {
            for col in 0..3 {
                if (row >> (2 - col)) & 1 == 1 {
                    let x = 23 + li as i32 * 4 + col;
                    let y = 2 + ry as i32;
                    put(x, y, rgb(255, 210, 90), out);
                    add(x, y, rgb(255, 180, 60), 0.35, out);
                }
            }
        }
    }

    // Runner: fast leg cycle, forward lean, ponytail streaming.
    let run = t * 9.0;
    let l1 = sinp(run);
    let l2 = sinp(run + PI);
    let bob = (sinp(run * 2.0) * 0.8) as i32;
    let rx = 29;
    let ry = 20 + bob;

    // Motion streaks behind.
    for k in 0..4 {
        let y = ry - 2 + k * 3;
        draw_line(2 + k, y, 18 - k * 2, y, rgb(255, 170, 80), out);
    }
    // Shadow on the track.
    ellipse(rx as f32 + 2.0, 32.0, 7.0, 1.2, rgb(90, 30, 30), out);

    // Legs: thick strokes with bright shoes.
    for (dx, shade) in [(l1 * 6.0, 0), (l2 * 6.0, 1)] {
        let lx = rx + dx as i32;
        let ly = 32 - (dx.abs() as i32 / 3).min(2);
        let c = if shade == 0 {
            rgb(238, 196, 150)
        } else {
            rgb(214, 172, 130)
        };

        draw_line(rx + 2, ry + 5, lx, ly, c, out);
        draw_line(rx + 3, ry + 5, lx + 1, ly, c, out);
        put(lx, ly, rgb(255, 240, 210), out);
        put(lx, ly - 1, rgb(230, 60, 60), out);
        put(lx + 1, ly - 1, rgb(250, 90, 80), out);
    }

    // Torso in a gold top, hips in dark shorts.
    for k in 0..2 {
        draw_line(rx + 1 + k, ry + 3, rx + 5 + k, ry - 6, rgb(248, 196, 76), out);
    }
    draw_rect_filled(rx + 1, ry + 3, 5, 2, rgb(36, 44, 96), out);
    // Head, face and ponytail.
    draw_circle_filled(rx + 6, ry - 8, 2, rgb(232, 190, 148), out);
    put(rx + 7, ry - 9, rgb(60, 34, 22), out);
    put(rx + 8, ry - 8, rgb(232, 190, 148), out);
    let tail = sinp(run * 2.0) * 2.0;
    draw_line(rx + 4, ry - 9, rx - 4, ry - 10 - tail as i32, rgb(54, 30, 20), out);
    draw_line(rx + 4, ry - 8, rx - 4, ry - 8 - tail as i32, rgb(76, 42, 26), out);
    // Arms pumping.
    draw_line(rx + 5, ry - 4, rx + 11 + l2 as i32 * 2, ry - 7, rgb(232, 190, 148), out);
    draw_line(rx + 4, ry - 3, rx - 2 + l1 as i32 * 2, ry + 1, rgb(232, 190, 148), out);

    // Gold medal swinging at the chest, with a ribbon and a "1".
    let mx = rx + 6;
    let my = ry + 1 + (sinp(run) * 1.2) as i32;

    draw_line(rx + 3, ry - 6, mx - 1, my - 2, rgb(220, 40, 45), out);
    draw_line(rx + 5, ry - 6, mx + 1, my - 2, rgb(220, 40, 45), out);
    glow(mx as f32, my as f32, 6.0, rgb(255, 200, 60), 0.55, out);
    draw_circle_filled(mx, my, 3, rgb(255, 206, 60), out);
    draw_circle(mx, my, 3, rgb(255, 240, 160), out);
    // A five-pixel-tall "1" on the medal face.
    put(mx, my - 2, rgb(190, 120, 20), out);
    put(mx, my - 1, rgb(190, 120, 20), out);
    put(mx, my, rgb(190, 120, 20), out);
    put(mx, my + 1, rgb(190, 120, 20), out);
    put(mx - 1, my + 1, rgb(190, 120, 20), out);
    put(mx + 1, my + 1, rgb(190, 120, 20), out);
    put(mx - 1, my - 1, rgb(255, 250, 190), out);

    // Confetti.
    for i in 0..22 {
        let h = fnv(i, 173);
        let x = (h % 60) as i32;
        let y = ((h >> 8) % 30) as i32;
        let speed = 3.0 + ((h >> 3) % 5) as f32;
        let fall = ((t * speed + y as f32) % 40.0) as i32;
        let c = hsv_to_rgb((i as f32 * 0.17) % 1.0, 0.85, 0.95);
        put(x, fall, c, out);
    }
}

/// A macaw on a jungle branch, wings and tail in constant motion.
pub fn parrot(ms: u32, out: &mut BufferTargetRgb888) {
    let t = ms as f32 / 1000.0;

    sky(rgb(4, 26, 34), rgb(12, 62, 46), out);
    // Sun shafts through the canopy.
    for k in 0..4 {
        for y in 0..H {
            let x = 12 + k * 13 + y / 3;
            add(x, y, rgb(180, 240, 140), 0.035, out);
        }
    }
    // Hanging vines and leaves.
    for x in (2..WIDTH as i32).step_by(9) {
        for y in 0..(10 + (x * 5) % 10) {
            put(
                x + (sinp(y as f32 * 0.2 + x as f32) * 2.0) as i32,
                y,
                rgb(16, 64, 38),
                out,
            );
        }
        for k in 0..3 {
            let lx = x + ((k * 7 + x) % 9) - 4;
            let ly = 4 + k * 5;
            leaf(lx, ly, 2, sinp(t + k as f32 + x as f32) * 0.6, rgb(28, 110, 54), out);
        }
    }

    // Branch.
    draw_line(0, 27, 60, 31, rgb(88, 56, 30), out);
    draw_line(0, 28, 60, 32, rgb(64, 40, 22), out);
    draw_line(0, 29, 60, 33, rgb(48, 30, 18), out);
    leaf(8, 24, 3, 0.2, rgb(20, 90, 46), out);
    leaf(52, 27, 2, 2.4, rgb(28, 110, 54), out);

    // Parrot perched center-right.
    let bob = sinp(t * 2.2) * 0.6;
    let cx = 31.0;
    let cy = 20.0 + bob;
    let flap = sinp(t * 7.5) * 0.5 + 0.5;

    // Tail feathers sweep down-left.
    for k in 0..3 {
        let ex = cx - 12.0 - k as f32 * 1.5;
        let ey = cy + 13.0 + k as f32 * 1.5;
        draw_line(
            (cx - 4.0) as i32,
            (cy + 3.0) as i32,
            ex as i32,
            ey as i32,
            if k == 1 { rgb(32, 80, 210) } else { rgb(220, 34, 44) },
            out,
        );
    }
    // Body and wing.
    ellipse(cx, cy, 5.5, 8.0, rgb(210, 32, 40), out);
    ellipse(cx + 1.0, cy + 1.0, 3.6, 6.0, rgb(238, 64, 48), out);
    let wing_y = cy - 2.0 - flap * 4.0;
    ellipse(cx - 3.0, wing_y, 3.6, 5.5, rgb(32, 90, 220), out);
    draw_line(
        (cx - 5.0) as i32,
        (wing_y - 2.0) as i32,
        (cx - 2.0) as i32,
        (wing_y + 4.0) as i32,
        rgb(255, 200, 40),
        out,
    );
    for k in 0..2 {
        draw_line(
            (cx - 5.0) as i32,
            (wing_y + k as f32) as i32,
            (cx - 8.0) as i32,
            (wing_y + 3.0 + k as f32) as i32,
            rgb(32, 90, 220),
            out,
        );
    }
    // Head, face patch, beak and eye.
    ellipse(
        cx + 2.0 + sinp(t * 0.9) * 0.8,
        cy - 7.0,
        3.6,
        3.6,
        rgb(220, 38, 44),
        out,
    );
    ellipse(cx + 4.0, cy - 7.0, 2.0, 2.2, rgb(245, 238, 220), out);
    put(cx as i32 + 3, cy as i32 - 8, rgb(10, 10, 10), out);
    put(cx as i32 + 4, cy as i32 - 7, rgb(255, 255, 255), out);
    draw_triangle_filled(
        (cx + 6.0) as i32,
        (cy - 8.0) as i32,
        (cx + 10.0) as i32,
        (cy - 6.0) as i32,
        (cx + 6.0) as i32,
        (cy - 4.0) as i32,
        rgb(28, 24, 24),
        out,
    );
    // Feet.
    draw_line(
        (cx + 1.0) as i32,
        (cy + 7.0) as i32,
        (cx + 3.0) as i32,
        29,
        rgb(180, 140, 60),
        out,
    );
    draw_line(
        (cx + 4.0) as i32,
        (cy + 7.0) as i32,
        (cx + 6.0) as i32,
        29,
        rgb(180, 140, 60),
        out,
    );

    // Flower and sparkles.
    for k in 0..5 {
        let a = k as f32 * PI * 0.4 + t * 0.2;
        draw_circle_filled(
            8 + (a.cos() * 2.0) as i32,
            18 + (a.sin() * 2.0) as i32,
            1,
            rgb(255, 90, 180),
            out,
        );
    }
    put(8, 18, rgb(255, 220, 120), out);

    for i in 0..7 {
        let h = fnv(i, 181);
        sparkle(
            4 + (h % 52) as i32,
            2 + ((h >> 8) % 22) as i32,
            t * 1.8 + i as f32,
            rgb(190, 255, 160),
            out,
        );
    }
}

// ── Heritage in Motion ─────────────────────────────────────────────────────

/// A narrowboat in the last of the golden hour, with mill, bridge, swan and
/// a proper reflection in the cut.
pub fn canal_boats(ms: u32, out: &mut BufferTargetRgb888) {
    let t = ms as f32 / 1000.0;
    let water_top = 23;

    // Sunset sky.
    for y in 0..water_top {
        let f = y as f32 / water_top as f32;
        for x in 0..WIDTH {
            out.buffer[y as usize * WIDTH + x] = Rgb888::new(
                (48.0 + 170.0 * f * f) as u8,
                (28.0 + 80.0 * f * f) as u8,
                (78.0 - 40.0 * f) as u8,
            );
        }
    }
    // Cloud bands.
    for k in 0..3 {
        let y = 4 + k * 4;
        for x in 0..WIDTH as i32 {
            let a = 0.18 + 0.10 * sinp(x as f32 * 0.15 + k as f32);
            over(x, y, rgb(240, 170, 120), a, out);
            over(x, y + 1, rgb(200, 120, 100), a * 0.5, out);
        }
    }
    // Sun on the horizon.
    glow(50.0, 20.0, 16.0, rgb(255, 190, 110), 0.45, out);
    draw_circle_filled(50, 20, 4, rgb(255, 224, 160), out);

    // Mill silhouette with a turning waterwheel.
    draw_rect_filled(39, 8, 10, 15, rgb(22, 16, 24), out);
    draw_rect_filled(43, 3, 2, 5, rgb(22, 16, 24), out);
    draw_triangle_filled(38, 8, 48, 8, 43, 4, rgb(30, 20, 28), out);
    draw_circle(39, 19, 4, rgb(40, 30, 40), out);
    draw_circle(39, 19, 2, rgb(40, 30, 40), out);
    for k in 0..4 {
        let a = t * 1.1 + k as f32 * PI / 2.0;
        draw_line(
            39,
            19,
            39 + (a.cos() * 4.0) as i32,
            19 + (a.sin() * 4.0) as i32,
            rgb(40, 30, 40),
            out,
        );
    }
    // A couple of warm windows in the mill.
    for (x, y) in [(41, 11), (45, 13), (42, 17)] {
        put(x, y, rgb(255, 190, 100), out);
    }
    // Stone bridge with a proper arch and a parapet.
    draw_rect_filled(0, 12, 18, 11, rgb(36, 30, 38), out);
    draw_rect_filled(0, 12, 18, 1, rgb(74, 64, 70), out);
    draw_rect_filled(0, 13, 18, 1, rgb(52, 44, 50), out);
    draw_circle_filled(9, 24, 5, rgb(24, 34, 58), out);
    draw_circle(9, 24, 5, rgb(68, 58, 64), out);
    draw_circle(9, 24, 6, rgb(48, 40, 48), out);
    for k in 0..4 {
        put(2 + k * 4, 15, rgb(58, 48, 54), out);
        put(3 + k * 4, 18, rgb(58, 48, 54), out);
    }
    for x in 0..18 {
        for y in 24..26 {
            put(x, y, rgb(24, 34, 58), out);
        }
    }

    // Water.
    for y in water_top..H {
        let f = (y - water_top) as f32 / (H - water_top) as f32;
        for x in 0..WIDTH {
            let ripple = 0.5 + 0.5 * sinp(x as f32 * 0.45 + t * 1.8 + y as f32 * 1.3);
            out.buffer[y as usize * WIDTH + x] = Rgb888::new(
                (16.0 + 22.0 * f + 8.0 * ripple) as u8,
                (26.0 + 18.0 * f + 10.0 * ripple) as u8,
                (54.0 + 24.0 * f + 18.0 * ripple) as u8,
            );
        }
    }
    // Sun glitter.
    for y in (water_top + 1)..H {
        let f = (y - water_top) as f32 / (H - water_top) as f32;
        for dx in -4i32..=4 {
            let x = 50 + dx;
            let lx = x + (sinp(y as f32 * 1.7 + t * 3.0) * 1.8) as i32;
            let v = (1.0 - f) * (0.4 + 0.6 * sinp(x as f32 * 2.0 + t * 5.0));
            if v > 0.15 {
                add(lx, y, rgb(255, 190, 100), v * 0.9, out);
            }
        }
    }

    // Narrowboat: ornate hull, cabin, chimney, tiller and a reflection.
    let cycle = 22.0;
    let p = t % cycle;
    let bx = -22.0 + (p / cycle) * (W as f32 + 40.0);
    let bi = bx as i32;
    let by = 20;

    // Hull: pointed bow on the right, dark base and a red rubbing strake.
    draw_rect_filled(bi, by, 18, 3, rgb(32, 30, 38), out);
    draw_rect_filled(bi, by + 1, 18, 1, rgb(180, 46, 40), out);
    draw_triangle_filled(bi + 17, by, bi + 21, by + 1, bi + 17, by + 3, rgb(32, 30, 38), out);
    put(bi + 20, by + 1, rgb(180, 46, 40), out);
    put(bi + 18, by + 2, rgb(210, 196, 160), out);
    // Stern deck.
    draw_rect_filled(bi - 2, by, 2, 2, rgb(210, 196, 160), out);
    // Cratch cover over the bow.
    draw_triangle_filled(
        bi + 13,
        by - 4,
        bi + 17,
        by - 4,
        bi + 15,
        by - 7,
        rgb(160, 160, 150),
        out,
    );
    // Cabin.
    draw_rect_filled(bi + 2, by - 5, 12, 5, rgb(210, 196, 160), out);
    draw_rect_filled(bi + 2, by - 5, 12, 1, rgb(44, 74, 54), out);
    draw_rect_filled(bi + 3, by - 4, 2, 2, rgb(255, 200, 100), out);
    draw_rect_filled(bi + 7, by - 4, 2, 2, rgb(255, 190, 80), out);
    draw_rect_filled(bi + 11, by - 4, 2, 2, rgb(255, 200, 100), out);
    draw_rect_filled(bi + 2, by - 2, 12, 1, rgb(120, 40, 40), out);
    // Roof pots and chimney.
    draw_rect_filled(bi + 4, by - 7, 2, 2, rgb(160, 90, 40), out);
    put(bi + 4, by - 8, rgb(220, 120, 60), out);
    put(bi + 5, by - 8, rgb(255, 160, 80), out);
    draw_rect_filled(bi + 9, by - 7, 2, 2, rgb(150, 100, 50), out);
    put(bi + 9, by - 8, rgb(120, 200, 90), out);
    put(bi + 10, by - 8, rgb(220, 70, 70), out);
    draw_rect_filled(bi + 12, by - 8, 1, 3, rgb(60, 50, 50), out);
    for k in 0..3 {
        let a = 0.18 - k as f32 * 0.05;
        over(bi + 12 + k as i32 / 2, by - 10 - k * 2, rgb(180, 170, 170), a, out);
    }
    // Tiller at the stern.
    draw_line(bi - 1, by - 3, bi - 3, by - 6, rgb(40, 34, 32), out);
    // A nameplate and bow flash.
    put(bi + 16, by + 1, rgb(255, 220, 140), out);

    // Reflection: the boat upside down in the water, dimmed.
    for y in 0..4 {
        for x in 0..18 {
            let src = out.buffer[((by + y) as usize) * WIDTH + (bi + x).clamp(0, W - 1) as usize];
            let f = 0.25 - y as f32 * 0.05;
            over(bi + x, water_top + 2 + y, src, f.max(0.0), out);
        }
    }

    // Swan gliding ahead of the boat.
    let sx = bi + 24;
    ellipse(sx as f32, 22.0, 3.0, 2.0, rgb(238, 238, 228), out);
    draw_line(sx + 2, 21, sx + 4, 17, rgb(238, 238, 228), out);
    put(sx + 4, 17, rgb(250, 180, 40), out);
    put(sx + 3, 17, rgb(20, 20, 20), out);
    over(sx - 1, 24, rgb(238, 238, 228), 0.18, out);

    // Bank-side reeds.
    for x in (0..WIDTH as i32).step_by(5) {
        draw_line(x, 39, x + 1, 34 - (x % 3), rgb(16, 38, 28), out);
    }

    for i in 0..8 {
        let h = fnv(i, 53);
        sparkle(
            (h % 60) as i32,
            33 + ((h >> 8) % 6) as i32,
            t * 1.4 + i as f32,
            rgb(190, 220, 255),
            out,
        );
    }
}

/// The big top at night: striped canvas, bunting, sweeping spotlights and a
/// juggling clown out front.
pub fn circus_tent(ms: u32, out: &mut BufferTargetRgb888) {
    let t = ms as f32 / 1000.0;

    sky(rgb(6, 4, 22), rgb(24, 10, 34), out);
    stars(t, 59, 40, 18, out);

    // Sweeping spotlights.
    for k in 0..3 {
        let a = t * 0.5 + k as f32 * 2.1;
        let base_x = 30.0;
        for y in 0..30 {
            let x = base_x + (a.sin() * (y as f32 * 0.9 + 2.0));
            add(x as i32, y, rgb(255, 220, 150), 0.05, out);
        }
    }

    // Tent: a domed big top with alternating red/cream stripes.
    let cx = 30.0;
    for y in 4..30 {
        let half = ((y as f32 - 4.0) * 1.4) as i32;
        for x in (cx as i32 - half)..=(cx as i32 + half) {
            let stripe = ((x - cx as i32).abs() / 3) % 2 == 0;
            let c = if stripe { rgb(196, 36, 48) } else { rgb(238, 224, 196) };
            let fade = 0.65 + 0.35 * (y as f32 / 30.0);
            over(
                x,
                y,
                Rgb888::new(
                    (c.r() as f32 * fade) as u8,
                    (c.g() as f32 * fade) as u8,
                    (c.b() as f32 * fade) as u8,
                ),
                1.0,
                out,
            );
        }
    }
    // Scalloped roof edge and flag.
    for x in 10..51 {
        put(x, 30, rgb(180, 28, 40), out);
        if x % 4 == 0 {
            put(x, 31, rgb(238, 224, 196), out);
        }
    }
    draw_line(30, 4, 30, 0, rgb(220, 200, 150), out);
    draw_triangle_filled(31, 0, 37, 2, 31, 4, rgb(220, 50, 50), out);

    // Entrance: warm glowing arch.
    ellipse(cx, 30.0, 5.0, 7.0, rgb(255, 190, 90), out);
    ellipse(cx, 30.0, 3.5, 5.5, rgb(80, 20, 30), out);

    // Bunting between tent and ground.
    for k in 0..2 {
        let y = 8 + k * 3;
        for x in 6..54 {
            let yy = y + (sinp(x as f32 * 0.18 + k as f32) * 1.2) as i32;
            put(x, yy, rgb(160, 150, 140), out);
            if x % 7 == 0 {
                let c = if (x / 7) % 3 == 0 {
                    rgb(220, 70, 70)
                } else if (x / 7) % 3 == 1 {
                    rgb(250, 200, 80)
                } else {
                    rgb(90, 180, 220)
                };
                put(x, yy + 1, c, out);
                put(x + 1, yy + 1, c, out);
            }
        }
    }

    // Sawdust ring.
    for x in 0..WIDTH as i32 {
        let y = 34 + (sinp(x as f32 * 0.15) * 1.0) as i32;
        hline(x, x, y, rgb(180, 130, 70), out);
        hline(x, x, y + 1, rgb(120, 82, 46), out);
    }

    // Clown on a ball, juggling three balls.
    let cxx = 46.0;
    let bob = sinp(t * 2.0) * 0.6;
    let ball_y = 33.0 + bob;
    draw_circle_filled(cxx as i32, ball_y as i32, 3, rgb(210, 60, 80), out);
    draw_line(
        cxx as i32,
        (ball_y - 3.0) as i32,
        cxx as i32,
        (ball_y - 10.0) as i32,
        rgb(240, 240, 230),
        out,
    );
    // Arms.
    draw_line(
        cxx as i32,
        (ball_y - 8.0) as i32,
        (cxx - 4.0) as i32,
        (ball_y - 12.0) as i32,
        rgb(240, 240, 230),
        out,
    );
    draw_line(
        cxx as i32,
        (ball_y - 8.0) as i32,
        (cxx + 4.0) as i32,
        (ball_y - 12.0) as i32,
        rgb(240, 240, 230),
        out,
    );
    // Head, hat, nose, bow tie.
    draw_circle_filled(cxx as i32, (ball_y - 13.0) as i32, 2, rgb(250, 220, 180), out);
    draw_triangle_filled(
        (cxx - 2.0) as i32,
        (ball_y - 15.0) as i32,
        (cxx + 2.0) as i32,
        (ball_y - 15.0) as i32,
        cxx as i32,
        (ball_y - 19.0) as i32,
        rgb(220, 60, 70),
        out,
    );
    put(cxx as i32, (ball_y - 13.0) as i32, rgb(230, 40, 40), out);
    draw_rect_filled((cxx - 2.0) as i32, (ball_y - 11.0) as i32, 4, 1, rgb(60, 100, 220), out);
    for k in 0..3 {
        let ph = t * 2.2 + k as f32 * 2.1;
        let jx = cxx + (sinp(ph) * 7.0) as f32;
        let jy = ball_y - 16.0 - (cosp(ph).abs() * 5.0) as f32;
        draw_circle_filled(
            jx as i32,
            jy as i32,
            1,
            hsv_to_rgb((k as f32 * 0.3 + 0.1) % 1.0, 0.9, 1.0),
            out,
        );
        glow(
            jx,
            jy,
            2.5,
            hsv_to_rgb((k as f32 * 0.3 + 0.1) % 1.0, 0.9, 1.0),
            0.3,
            out,
        );
    }

    for i in 0..12 {
        let h = fnv(i, 191);
        sparkle(
            (h % 60) as i32,
            2 + ((h >> 8) % 12) as i32,
            t * 1.6 + i as f32,
            rgb(255, 230, 160),
            out,
        );
    }
}

/// Painted elephants in a festival parade — ears flapping, trunk swinging,
/// marigolds and confetti.
pub fn elephants(ms: u32, out: &mut BufferTargetRgb888) {
    let t = ms as f32 / 1000.0;

    sky(rgb(24, 7, 40), rgb(68, 18, 56), out);

    // Bunting across the top.
    for x in 0..WIDTH as i32 {
        let y = 4 + (sinp(x as f32 * 0.16) * 1.5) as i32;
        put(x, y, rgb(180, 150, 140), out);

        if x % 8 == 0 {
            let c = hsv_to_rgb((x as f32 / 60.0 + 0.3) % 1.0, 0.75, 0.9);
            draw_triangle_filled(x, y + 1, x + 3, y + 1, x + 1, y + 4, c, out);
        }
    }
    // Distant festival lights.
    for i in 0..16 {
        let h = fnv(i, 223);
        glow(
            (h % 60) as f32,
            (27 + ((h >> 8) % 6)) as f32,
            2.0,
            rgb(255, 190, 100),
            0.25,
            out,
        );
    }

    // Sawdust ring and marigold garlands.
    draw_rect_filled(0, 34, 60, 6, rgb(46, 14, 32), out);
    for x in 0..WIDTH as i32 {
        if x % 3 == 0 {
            put(x, 34 + (x % 2), rgb(255, 160, 40), out);
        }
    }

    // Reusable elephant painter. s scales the whole animal; phase drives
    // the walk so each animal has its own gait.
    let elephant = |cx: f32,
                    cy: f32,
                    s: f32,
                    phase: f32,
                    body: Rgb888,
                    dark: Rgb888,
                    ear: Rgb888,
                    out: &mut BufferTargetRgb888| {
        let walk = sinp(phase);
        let flap = sinp(phase * 1.9) * 0.8;

        // Tail.
        draw_line(
            (cx - 10.0 * s) as i32,
            (cy - 1.0 * s) as i32,
            (cx - 13.0 * s) as i32,
            (cy + 3.0 * s) as i32,
            dark,
            out,
        );
        put((cx - 13.0 * s) as i32, (cy + 3.0 * s) as i32, rgb(255, 210, 120), out);

        // Legs, with a gold anklet.
        for k in 0..4 {
            let x = cx + (-7.0 + k as f32 * 4.5) * s;
            let dx = sinp(phase + k as f32 * 1.5) * 1.1 * s;
            let top = (cy + 3.0 * s) as i32;
            let h = (5.0 * s) as i32;

            draw_rect_filled((x + dx) as i32, top, (2.8 * s) as i32, h, dark, out);
            put((x + dx) as i32, top + h - 1, rgb(255, 202, 90), out);
            put((x + dx) as i32 + (2.4 * s) as i32, top + h - 1, rgb(255, 202, 90), out);
        }

        // Body.
        ellipse(cx, cy, 10.0 * s, 6.0 * s, body, out);
        ellipse(cx - 1.0 * s, cy - 1.0 * s, 8.2 * s, 4.4 * s, body, out);
        ellipse(cx - 1.0 * s, cy + 2.5 * s, 7.0 * s, 2.0 * s, dark, out);
        // Moonlit rim along the back.
        draw_line(
            (cx - 9.0 * s) as i32,
            (cy - 4.0 * s) as i32,
            (cx + 2.0 * s) as i32,
            (cy - 5.0 * s) as i32,
            rgb(255, 220, 140),
            out,
        );

        // Decorative blanket with gold trim and tassels.
        let bx = (cx - 6.0 * s) as i32;
        let by = (cy - 5.0 * s) as i32;
        let bw = (12.0 * s) as i32;
        let bh = (5.0 * s) as i32;

        draw_rect_filled(bx, by, bw, bh, rgb(210, 50, 78), out);
        draw_rect_filled(bx, by, bw, 1, rgb(255, 204, 80), out);
        draw_rect_filled(bx, by + bh - 1, bw, 1, rgb(255, 204, 80), out);

        for k in 0..3 {
            put(bx + 2 + k * ((bw - 2) / 3), by + 2, rgb(255, 226, 130), out);
        }
        for k in 0..3 {
            put(bx + 1 + k * 4, by + bh, rgb(255, 190, 70), out);
        }

        // Head and ear.
        let hx = cx + 10.0 * s;
        let hy = cy - 1.0 * s;

        ellipse(hx, hy, 5.2 * s, 5.2 * s, body, out);
        ellipse(hx - 2.0 * s, hy + (0.4 + flap) * s, 3.8 * s, 4.6 * s, ear, out);
        ellipse(
            hx - 2.0 * s,
            hy + (0.4 + flap) * s,
            2.4 * s,
            3.0 * s,
            rgb(246, 186, 210),
            out,
        );

        // Trunk: a curl of shrinking discs.
        let swing = sinp(phase * 0.8) * 0.8;
        for k in 0..8 {
            let f = k as f32 / 7.0;
            let tx = hx + (3.6 + f * 2.6 + swing * f) * s;
            let ty = hy + (2.0 + f * 7.5) * s;
            let r = ((2.1 - f * 1.2) * s).max(0.6);

            draw_circle_filled(tx as i32, ty as i32, r as i32, body, out);
        }
        // Tusk, eye and a gold headband.
        draw_line(
            (hx + 2.5 * s) as i32,
            (hy + 2.5 * s) as i32,
            (hx + 5.5 * s) as i32,
            (hy + 4.2 * s) as i32,
            rgb(248, 242, 214),
            out,
        );
        put((hx + 1.2 * s) as i32, (hy - 1.2 * s) as i32, rgb(22, 12, 22), out);
        put((hx + 1.2 * s) as i32, (hy - 2.0 * s) as i32, rgb(255, 255, 255), out);
        draw_line(
            (hx - 3.0 * s) as i32,
            (hy - 3.2 * s) as i32,
            (hx + 3.0 * s) as i32,
            (hy - 3.2 * s) as i32,
            rgb(255, 206, 90),
            out,
        );

        // Headdress plume.
        draw_triangle_filled(
            (hx - 1.0 * s) as i32,
            (hy - 4.0 * s) as i32,
            (hx + 3.0 * s) as i32,
            (hy - 4.0 * s) as i32,
            hx as i32,
            (hy - 10.0 * s) as i32,
            rgb(246, 82, 104),
            out,
        );
        draw_circle_filled(hx as i32, (hy - 4.0 * s) as i32, 1, rgb(255, 214, 86), out);
        put((hx - 1.0 * s) as i32, (hy - 5.0 * s) as i32, rgb(255, 236, 150), out);

        // Walk bob.
        let _ = walk;
    };

    // A smaller companion behind, then the lead elephant.
    elephant(
        13.0,
        27.0,
        0.62,
        t * 2.0 + 1.4,
        rgb(156, 118, 190),
        rgb(106, 74, 140),
        rgb(236, 168, 204),
        out,
    );
    elephant(
        33.0,
        23.5,
        1.0,
        t * 2.2,
        rgb(206, 156, 232),
        rgb(146, 94, 174),
        rgb(255, 178, 214),
        out,
    );

    // Confetti.
    for i in 0..26 {
        let h = fnv(i, 197);
        let x = (h % 60) as i32;
        let y = ((h >> 8) % 40) as i32;
        let speed = 2.0 + ((h >> 3) % 4) as f32;
        let fall = ((t * speed + y as f32) % 40.0) as i32;

        put(x, fall, hsv_to_rgb((i as f32 * 0.19) % 1.0, 0.8, 0.9), out);
    }

    // Sparkles around the parade.
    for i in 0..8 {
        let h = fnv(i, 199);
        sparkle(
            4 + (h % 52) as i32,
            8 + ((h >> 8) % 20) as i32,
            t * 1.5 + i as f32,
            rgb(255, 220, 150),
            out,
        );
    }
}

/// A cottage spinning wheel: the great wheel turns, the wool glows, a cat
/// sleeps in the warm light.
pub fn spinning_wheel(ms: u32, out: &mut BufferTargetRgb888) {
    let t = ms as f32 / 1000.0;

    // Cottage wall and warm window.
    sky(rgb(28, 16, 18), rgb(58, 32, 24), out);
    draw_rect_filled(43, 5, 14, 14, rgb(18, 26, 44), out);
    draw_rect_filled(44, 6, 12, 12, rgb(50, 70, 108), out);
    draw_line(50, 6, 50, 18, rgb(30, 30, 40), out);
    draw_line(44, 12, 56, 12, rgb(30, 30, 40), out);
    draw_circle_filled(52, 8, 3, rgb(230, 230, 210), out);
    // Window glow.
    glow(50.0, 12.0, 14.0, rgb(120, 150, 200), 0.25, out);

    // Floorboards.
    for y in (30..40).step_by(3) {
        hline(0, 59, y, rgb(46, 26, 18), out);
    }

    // Spinning wheel: rim and rotating spokes.
    let cx = 23.0;
    let cy = 20.0;
    let r = 12.0;
    draw_circle(cx as i32, cy as i32, r as i32, rgb(190, 136, 64), out);
    draw_circle(cx as i32, cy as i32, r as i32 - 1, rgb(56, 34, 20), out);
    draw_circle(cx as i32, cy as i32, 2, rgb(230, 180, 90), out);
    let rot = t * 2.5;
    for k in 0..8 {
        let a = rot + k as f32 * PI / 4.0;
        draw_line(
            cx as i32,
            cy as i32,
            (cx + a.cos() * (r - 1.0)) as i32,
            (cy + a.sin() * (r - 1.0)) as i32,
            rgb(220, 168, 86),
            out,
        );
    }
    // Treadle and stand.
    draw_line(23, 32, 14, 37, rgb(70, 44, 24), out);
    draw_line(23, 32, 32, 37, rgb(70, 44, 24), out);
    draw_line(14, 37, 32, 37, rgb(58, 34, 20), out);

    // Distaff and a fluffy cloud of fleece.
    draw_line(10, 14, 18, 31, rgb(80, 52, 28), out);
    glow(8.0, 11.0, 8.0, rgb(255, 226, 180), 0.5, out);
    for (x, y, rx, ry) in [
        (6.0, 11.0, 3.0, 2.4),
        (9.0, 9.5, 3.2, 2.6),
        (7.0, 8.0, 2.4, 2.0),
        (11.0, 11.5, 2.5, 2.2),
        (9.0, 12.5, 2.6, 2.0),
    ] {
        ellipse(x, y, rx, ry, rgb(236, 224, 200), out);
    }
    ellipse(8.5, 10.0, 2.2, 1.6, rgb(255, 248, 230), out);

    // Thread from fleece to spindle, with a travelling glint.
    for k in 0..10 {
        let x = 10 + k;
        let y = 13 + k;
        put(x, y, rgb(230, 210, 170), out);
    }
    let glint = (t * 1.4) % 1.0;
    add(
        (10.0 + glint * 10.0) as i32,
        (13.0 + glint * 10.0) as i32,
        rgb(255, 250, 220),
        0.9,
        out,
    );

    // Basket of yarn in the corner.
    draw_rect_filled(2, 34, 8, 5, rgb(110, 72, 38), out);
    draw_rect_filled(2, 34, 8, 1, rgb(160, 110, 58), out);
    draw_line(2, 36, 10, 36, rgb(76, 48, 26), out);
    for (x, y, c) in [
        (4, 33, rgb(210, 60, 80)),
        (7, 32, rgb(80, 160, 210)),
        (5, 31, rgb(250, 190, 70)),
    ] {
        draw_circle_filled(x, y, 1, c, out);
        put(x, y - 1, rgb(255, 240, 210), out);
    }

    // Cat curled on the rug, breathing, tail flicking.
    let breath = (sinp(t * 1.1) * 0.6) as i32;
    ellipse(47.0, (36.0 - breath as f32) as f32, 7.0, 3.0, rgb(44, 30, 24), out);
    ellipse(41.0, (34.0 - breath as f32) as f32, 3.2, 2.6, rgb(50, 34, 28), out);
    draw_triangle_filled(38, 32 - breath, 40, 32 - breath, 40, 29 - breath, rgb(44, 30, 24), out);
    draw_triangle_filled(42, 32 - breath, 44, 32 - breath, 43, 29 - breath, rgb(44, 30, 24), out);
    // Warm rim light along the back and two closed eyes.
    for x in 41..54 {
        put(x, 33 - breath, rgb(230, 180, 110), out);
    }
    put(40, 34 - breath, rgb(230, 180, 110), out);
    put(42, 34 - breath, rgb(230, 180, 110), out);
    let tail = sinp(t * 1.8) * 2.0;
    draw_line(53, 36, 57, (34.0 + tail) as i32, rgb(44, 30, 24), out);
    put(57, (33.5 + tail) as i32, rgb(180, 130, 80), out);

    // Hearth glow and dust motes.
    glow(30.0, 39.0, 16.0, rgb(255, 140, 60), 0.22, out);
    for i in 0..12 {
        let h = fnv(i, 211);
        let x = (h % 60) as i32;
        let y = 4 + ((h >> 8) % 28) as i32;
        add(x, y, rgb(255, 220, 160), 0.25 + 0.2 * sinp(t + i as f32), out);
    }
}

/// The mill town at dusk: chimney smoke, sawtooth roofs, a turning gear and
/// the canal catching the last light.
pub fn mill_buildings(ms: u32, out: &mut BufferTargetRgb888) {
    let t = ms as f32 / 1000.0;

    sky(rgb(6, 10, 30), rgb(30, 30, 58), out);
    stars(t, 67, 45, 18, out);
    glow(45.0, 22.0, 16.0, rgb(255, 170, 80), 0.35, out);
    draw_circle_filled(45, 22, 3, rgb(255, 210, 140), out);

    // Mills: sawtooth roofs and rows of warm windows.
    for (bx, bw, bh) in [(4, 14, 12), (20, 16, 16), (39, 12, 10)] {
        draw_rect_filled(bx, 34 - bh, bw, bh, rgb(20, 18, 30), out);
        // Sawtooth roofline with a pale top edge.
        for k in 0..(bw / 4) {
            let x = bx + k * 4;
            draw_triangle_filled(x, 34 - bh, x + 4, 34 - bh, x + 4, 34 - bh - 3, rgb(38, 34, 52), out);
            put(x + 4, 34 - bh - 3, rgb(130, 165, 210), out);
            put(x + 3, 34 - bh - 2, rgb(90, 120, 160), out);
            put(x + 2, 34 - bh - 1, rgb(70, 94, 128), out);
        }
        // Windows: warm, some flickering.
        for wy in (34 - bh + 3)..33 {
            if (wy % 3) != 0 {
                continue;
            }
            for wx in (bx + 2)..(bx + bw - 1) {
                if (wx % 3) == 0 {
                    let flick = if ((wx + wy) as f32 * 0.7 + t).sin() > -0.6 {
                        1.0
                    } else {
                        0.2
                    };
                    add(wx, wy, rgb(255, 180, 80), 0.9 * flick, out);
                    if flick > 0.5 {
                        add(wx, wy + 1, rgb(255, 140, 50), 0.25, out);
                    }
                }
            }
        }
    }

    // Two tall chimneys behind the roofline, with a moonlit edge.
    for (x, h) in [(10, 10), (44, 13)] {
        draw_rect_filled(x, 12 - h, 2, h + 22, rgb(24, 20, 30), out);
        draw_rect_filled(x - 1, 11 - h, 4, 1, rgb(42, 36, 48), out);
        put(x, 11 - h, rgb(150, 180, 220), out);
        put(x - 1, 21 - h, rgb(90, 110, 144), out);
        put(x - 1, 29 - h, rgb(90, 110, 144), out);
    }

    // Chimney smoke.
    for k in 0..6 {
        let y = 10 - k as i32 + ((t * 2.0 + k as f32) as i32 % 3);
        let x = 45 + (sinp(t * 0.5 + k as f32 * 0.7) * 7.0) as i32 + k as i32;

        if y >= 0 {
            glow(
                x as f32,
                y as f32,
                2.5,
                rgb(130, 130, 150),
                0.12 - k as f32 * 0.012,
                out,
            );
        }
    }

    // Big cog turning in the foreground.
    let gx = 30.0;
    let gy = 29.0;
    let gr = 8.0;
    draw_circle(gx as i32, gy as i32, gr as i32, rgb(80, 70, 80), out);
    draw_circle(gx as i32, gy as i32, gr as i32 - 1, rgb(30, 26, 34), out);
    for k in 0..12 {
        let a = t * 0.8 + k as f32 * PI / 6.0;
        let tx = gx + a.cos() * (gr + 0.6);
        let ty = gy + a.sin() * (gr + 0.6);
        draw_rect_filled(tx as i32 - 1, ty as i32 - 1, 2, 2, rgb(104, 88, 78), out);
    }
    for k in 0..10 {
        let a = t * 0.8 + k as f32 * PI / 5.0;
        draw_line(
            gx as i32,
            gy as i32,
            (gx + a.cos() * gr as f32) as i32,
            (gy + a.sin() * gr as f32) as i32,
            rgb(100, 84, 76),
            out,
        );
    }
    draw_circle_filled(gx as i32, gy as i32, 2, rgb(150, 120, 70), out);

    // Canal in the foreground.
    for y in 35..H {
        let f = (y - 35) as f32 / 5.0;
        for x in 0..WIDTH {
            let ripple = 0.5 + 0.5 * sinp(x as f32 * 0.5 + t * 1.6 + y as f32);
            out.buffer[y as usize * WIDTH + x] = Rgb888::new(
                (12.0 + 10.0 * f + 4.0 * ripple) as u8,
                (26.0 + 18.0 * f + 6.0 * ripple) as u8,
                (46.0 + 26.0 * f + 10.0 * ripple) as u8,
            );
        }
    }
    for x in 0..WIDTH as i32 {
        if x % 4 == 0 {
            add(
                x,
                36 + (x % 3),
                rgb(255, 160, 70),
                0.35 * (0.5 + 0.5 * sinp(t * 2.0 + x as f32)),
                out,
            );
        }
    }

    // Birds crossing the dusk.
    for i in 0..3 {
        let x = ((t * 7.0 + i as f32 * 13.0) % 70.0) as i32 - 5;
        let y = 10 + i * 2;
        draw_line(x, y, x + 2, y - 1, rgb(20, 18, 28), out);
        draw_line(x + 2, y - 1, x + 4, y, rgb(20, 18, 28), out);
    }
}

// ── small shared math ──────────────────────────────────────────────────────

// micromath's sin/cos are only accurate within -π..π, so fold first.
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

// Deterministic seed → pseudo-random u32 (FNV-style).
fn fnv(i: u32, salt: u32) -> u32 {
    i.wrapping_mul(1_677_761_993) ^ salt.wrapping_mul(2_246_822_519)
}
