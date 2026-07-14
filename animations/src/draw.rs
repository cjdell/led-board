use crate::common::{BufferTargetRgb888, HEIGHT, WIDTH};
use embedded_graphics::pixelcolor::Rgb888;
use micromath::F32Ext;

// Simple Bresenham line drawing (for wireframe edges)
pub fn draw_line(x0: i32, y0: i32, x1: i32, y1: i32, color: Rgb888, target: &mut BufferTargetRgb888) {
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

// Draw a filled rectangle
pub fn draw_rect_filled(x: i32, y: i32, width: i32, height: i32, color: Rgb888, target: &mut BufferTargetRgb888) {
    let x_end = x + width;
    let y_end = y + height;

    for py in y..y_end {
        for px in x..x_end {
            if px >= 0 && px < WIDTH as i32 && py >= 0 && py < HEIGHT as i32 {
                target.buffer[py as usize * WIDTH + px as usize] = color;
            }
        }
    }
}

// Draw an empty (outline only) rectangle
pub fn draw_rect_outline(x: i32, y: i32, width: i32, height: i32, color: Rgb888, target: &mut BufferTargetRgb888) {
    // Top edge
    draw_line(x, y, x + width - 1, y, color, target);
    // Bottom edge
    draw_line(x, y + height - 1, x + width - 1, y + height - 1, color, target);
    // Left edge
    draw_line(x, y, x, y + height - 1, color, target);
    // Right edge
    draw_line(x + width - 1, y, x + width - 1, y + height - 1, color, target);
}

// Draw a circle using the midpoint circle algorithm
pub fn draw_circle(x0: i32, y0: i32, radius: i32, color: Rgb888, target: &mut BufferTargetRgb888) {
    let mut x = radius;
    let mut y = 0;
    let mut err = 0;

    while x >= y {
        // Draw all 8 octants
        draw_pixel(x0 + x, y0 + y, color, target);
        draw_pixel(x0 - x, y0 + y, color, target);
        draw_pixel(x0 + x, y0 - y, color, target);
        draw_pixel(x0 - x, y0 - y, color, target);
        draw_pixel(x0 + y, y0 + x, color, target);
        draw_pixel(x0 - y, y0 + x, color, target);
        draw_pixel(x0 + y, y0 - x, color, target);
        draw_pixel(x0 - y, y0 - x, color, target);

        y += 1;
        err += 1 + 2 * y;
        if 2 * (err - x) + 1 > 0 {
            x -= 1;
            err -= 1 + 2 * x;
        }
    }
}

// Draw a filled circle using the midpoint circle algorithm (fixed version)
pub fn draw_circle_filled(x0: i32, y0: i32, radius: i32, color: Rgb888, target: &mut BufferTargetRgb888) {
    if radius <= 0 {
        return; // Nothing to draw
    }

    let mut x = 0;
    let mut y = radius;
    let mut err = 2 * (1 - radius);

    while x <= y {
        // For each x,y pair, draw a horizontal line from (x0 - x, y0 + y) to (x0 + x, y0 + y)
        // and its symmetric counterparts across all 4 quadrants
        draw_hline(x0 - x, x0 + x, y0 + y, color, target); // Top-right quadrant
        draw_hline(x0 - x, x0 + x, y0 - y, color, target); // Bottom-right quadrant
        draw_hline(x0 - y, x0 + y, y0 + x, color, target); // Top-left quadrant
        draw_hline(x0 - y, x0 + y, y0 - x, color, target); // Bottom-left quadrant

        // Midpoint circle algorithm update
        let e2 = err;
        if e2 <= x {
            x += 1;
            err += 2 * x + 1;
        }
        if e2 > -y {
            y -= 1;
            err -= 2 * y - 1;
        }
    }
}

// Helper: Draw a horizontal line from x_start to x_end (inclusive) at fixed y
fn draw_hline(x_start: i32, x_end: i32, y: i32, color: Rgb888, target: &mut BufferTargetRgb888) {
    if y < 0 || y >= HEIGHT as i32 {
        return; // Out of vertical bounds
    }

    let x_min = x_start.max(0);
    let x_max = x_end.min(WIDTH as i32 - 1);

    if x_min <= x_max {
        let y_idx = y as usize * WIDTH;
        for x in x_min..=x_max {
            target.buffer[y_idx + x as usize] = color;
        }
    }
}

// Draw a single pixel (helper function used by circle)
fn draw_pixel(x: i32, y: i32, color: Rgb888, target: &mut BufferTargetRgb888) {
    if x >= 0 && x < WIDTH as i32 && y >= 0 && y < HEIGHT as i32 {
        target.buffer[y as usize * WIDTH + x as usize] = color;
    }
}

// Draw a triangle (filled)
pub fn draw_triangle_filled(
    x1: i32,
    y1: i32,
    x2: i32,
    y2: i32,
    x3: i32,
    y3: i32,
    color: Rgb888,
    target: &mut BufferTargetRgb888,
) {
    // Sort vertices by y-coordinate (bottom to top)
    let mut vertices = [(x1, y1), (x2, y2), (x3, y3)];
    vertices.sort_by_key(|&(_, y)| y);

    let (x_bottom, y_bottom) = vertices[0];
    let (x_middle, y_middle) = vertices[1];
    let (x_top, y_top) = vertices[2];

    if y_bottom == y_top {
        return; // Degenerate triangle
    }

    // Calculate slopes for the three edges
    let slope1 = if y_top != y_bottom {
        (x_top - x_bottom) as f32 / (y_top - y_bottom) as f32
    } else {
        0.0
    };

    let slope2 = if y_middle != y_bottom {
        (x_middle - x_bottom) as f32 / (y_middle - y_bottom) as f32
    } else {
        0.0
    };

    let slope3 = if y_top != y_middle {
        (x_top - x_middle) as f32 / (y_top - y_middle) as f32
    } else {
        0.0
    };

    // Draw scanlines from bottom to middle
    for y in y_bottom..=y_middle {
        let x_left = if y_bottom != y_middle {
            x_bottom as f32 + (y - y_bottom) as f32 * slope2
        } else {
            x_bottom as f32
        };

        let x_right = if y_bottom != y_top {
            x_bottom as f32 + (y - y_bottom) as f32 * slope1
        } else {
            x_bottom as f32
        };

        let x_start = x_left.min(x_right).floor() as i32;
        let x_end = x_left.max(x_right).ceil() as i32;

        for x in x_start..=x_end {
            if x >= 0 && x < WIDTH as i32 && y >= 0 && y < HEIGHT as i32 {
                target.buffer[y as usize * WIDTH + x as usize] = color;
            }
        }
    }

    // Draw scanlines from middle to top
    for y in y_middle + 1..=y_top {
        let x_left = if y_middle != y_top {
            x_middle as f32 + (y - y_middle) as f32 * slope3
        } else {
            x_middle as f32
        };

        let x_right = if y_bottom != y_top {
            x_bottom as f32 + (y - y_bottom) as f32 * slope1
        } else {
            x_bottom as f32
        };

        let x_start = x_left.min(x_right).floor() as i32;
        let x_end = x_left.max(x_right).ceil() as i32;

        for x in x_start..=x_end {
            if x >= 0 && x < WIDTH as i32 && y >= 0 && y < HEIGHT as i32 {
                target.buffer[y as usize * WIDTH + x as usize] = color;
            }
        }
    }
}

// Draw an empty triangle (outline only)
pub fn draw_triangle_outline(
    x1: i32,
    y1: i32,
    x2: i32,
    y2: i32,
    x3: i32,
    y3: i32,
    color: Rgb888,
    target: &mut BufferTargetRgb888,
) {
    draw_line(x1, y1, x2, y2, color, target);
    draw_line(x2, y2, x3, y3, color, target);
    draw_line(x3, y3, x1, y1, color, target);
}
