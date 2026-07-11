use animations::{
    AnimationFader, AnimationRunner, BufferTarget, Playlist, ScrollingMessage, TOTAL_PIXELS, TextMessage,
    apply_power_limit,
};
use embedded_graphics::pixelcolor::{IntoStorage, Rgb888, RgbColor as _};
use minifb::{Key, Scale, Window, WindowOptions};
use sim::sim_screen::{HEIGHT, SimScreen, WIDTH};
use std::{
    thread::sleep,
    time::{self, Duration, SystemTime},
};

fn main() {
    let mut screen_buffer = [0u32; TOTAL_PIXELS];

    let mut window_options = WindowOptions::default();
    window_options.scale = Scale::X8;
    let mut window = Window::new("LED Sim", WIDTH, HEIGHT, window_options).unwrap();

    let mut key_down_time: Option<SystemTime> = None;

    let mut buffer_1 = BufferTarget::new();
    let mut buffer_2 = BufferTarget::new();

    let mut runner = AnimationRunner::new();
    let mut last_time = time::SystemTime::now();

    while window.is_open() && !window.is_key_down(Key::Escape) {
        let now = time::SystemTime::now();
        let time_since_last_frame = now.duration_since(last_time).unwrap();
        let delta_ms = time_since_last_frame.as_millis() as u32;

        last_time = now;

        // println!("delta_ms: {delta_ms}");

        if let Some(time) = key_down_time {
            if now.duration_since(time).unwrap().as_millis() > 1000 {
                key_down_time = None;
            }
        }

        if key_down_time.is_none() {
            if window.is_key_down(Key::Space) {
                key_down_time = Some(now);

                runner.next();
            }

            if window.is_key_down(Key::T) {
                key_down_time = Some(now);

                runner.set_override_animation(Box::new(TextMessage {}), 1_000);
            }

            if window.is_key_down(Key::Y) {
                key_down_time = Some(now);

                runner.set_override_animation(
                    Box::new(ScrollingMessage {
                        msg: "The quick brown fox jumped over the lazy dog.".to_string(),
                    }),
                    4_500,
                );
            }
        }

        runner.update(delta_ms, &mut buffer_1, &mut buffer_2);

        apply_power_limit(&mut buffer_1, 0.5);

        let mut i = 0;
        for c in buffer_1.buffer {
            let raw: u32 = c.into_storage();
            screen_buffer[i] = raw | 0xFF000000;
            i += 1;
        }

        window.update_with_buffer(&screen_buffer, WIDTH, HEIGHT).unwrap();

        sleep(Duration::from_millis(10));
    }
}

fn set_pixel(buffer: &mut Vec<u32>, x: usize, y: usize, color: u32) {
    if x < WIDTH && y < HEIGHT {
        buffer[y * WIDTH + x] = color;
    }
}
