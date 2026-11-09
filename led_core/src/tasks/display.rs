//! The display worker: runs the animation playlist into the screen.
//!
//! On the Pico this is the core1 task; on the simulator it is a task on the
//! host executor thread. The loop body is identical — frames land on the
//! screen through [`Ws2812ParallelDriver`].

use alloc::boxed::Box;
use animations::{Animation, AnimationEnum, AnimationParams, AnimationRunner, BufferTarget, Playlist, apply_power_limit};
use embedded_graphics::pixelcolor::Rgb888;

use crate::info;
use crate::led_screen::{LedScreen, Ws2812ParallelDriver, PIXELS_PER_STRIP};
use crate::types::{DisplayWorkerMessage, DisplayWorkerReceiver};

pub async fn run<D: Ws2812ParallelDriver<PIXELS_PER_STRIP>>(
    receiver: DisplayWorkerReceiver,
    mut led_screen: LedScreen<D>,
) -> ! {
    info!("display worker");

    led_screen.copy_eg_buffer(&[Rgb888::new(0x10, 0x10, 0x0); crate::led_screen::TOTAL_PIXELS]);
    led_screen.flush().await;

    let mut buffer_1 = BufferTarget::new();
    let mut buffer_2 = BufferTarget::new();

    let mut runner = AnimationRunner::new(Playlist::new(Playlist::get_default_playlist_data()));
    let mut last_time = embassy_time::Instant::now();

    let mut power_limit = 0.1f32;
    // Status-screen mode: when set, the animation is drawn over the still
    // advancing playlist until StatusClear (see DisplayWorkerMessage).
    let mut status_screen: Option<(AnimationEnum, u32)> = None;

    loop {
        let now = embassy_time::Instant::now();
        let time_since_last_frame = now.duration_since(last_time);
        let delta_ms = time_since_last_frame.as_millis() as u32;

        last_time = now;

        if let Ok(msg) = receiver.try_receive() {
            match msg {
                DisplayWorkerMessage::Next => {
                    info!("Next");
                    runner.next();
                }
                DisplayWorkerMessage::Animation(animation, length_ms) => {
                    info!("Override Animation");
                    runner.set_override_animation(Box::new(animation), length_ms);
                }
                DisplayWorkerMessage::Playlist(playlist_data) => {
                    info!("Update playlist");
                    if playlist_data.len() > 0 {
                        runner.update_playlist(Playlist::new(playlist_data));
                    } else {
                        runner.update_playlist(Playlist::new(Playlist::get_default_playlist_data()));
                    }
                }
                DisplayWorkerMessage::PowerLimit(new_power_limit) => power_limit = new_power_limit,
                DisplayWorkerMessage::ParamsBuffer(params_buffer) => {
                    info!("Params Buffer");
                    for params in params_buffer {
                        runner.push_params(params);
                    }
                }
                DisplayWorkerMessage::FrameBuffer(pixels) => {
                    runner.set_suspend(10_000);
                    let mut i = 0;
                    for pixel in pixels {
                        buffer_1.buffer[i] = pixel;
                        i += 1;
                    }
                }
                DisplayWorkerMessage::Status(animation) => {
                    info!("Status screen");
                    status_screen = Some((animation, 0));
                }
                DisplayWorkerMessage::StatusClear => {
                    info!("Status cleared");
                    status_screen = None;
                }
            }
        }

        runner.update(delta_ms, &mut buffer_1, &mut buffer_2);

        if let Some((animation, ms)) = status_screen.as_mut() {
            *ms += delta_ms;
            animation.draw(*ms, &AnimationParams::default(), &mut buffer_1);
        }

        apply_power_limit(&mut buffer_1, power_limit);

        led_screen.copy_eg_buffer(&buffer_1.buffer);
        led_screen.flush().await;
    }
}
