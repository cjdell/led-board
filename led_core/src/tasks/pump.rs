//! The WebSocket control pump: turns incoming browser messages into
//! display-worker commands, playlist persistence, and activity pings.
//!
//! This is the same loop the Pico runs on core0 next to the network task;
//! on the simulator it shares the host executor thread with the display
//! worker.

use alloc::vec;

use animations::AnimationParams;
use embedded_graphics::pixelcolor::Rgb888;

use crate::config::DEFAULT_PLAYLIST_FILENAME;
use crate::error;
use embedded_tools::{info, warn};
use crate::types::{
    DisplayWorkerMessage, DisplayWorkerSender, WebSocketIncomingMessage, WebSocketIncomingReceiver,
};
use embedded_tools::local_fs::LocalFsTrait;
use embedded_tools::Debug2Format;

pub async fn run<F: LocalFsTrait + Clone>(
    receiver: WebSocketIncomingReceiver,
    display: DisplayWorkerSender,
    local_fs: F,
    activity_watch_sender: crate::types::ActivityWatchSender,
) -> ! {
    loop {
        match receiver.receive().await {
            WebSocketIncomingMessage::Ping => {
                activity_watch_sender.send(embassy_time::Instant::now().as_millis());
            }
            WebSocketIncomingMessage::FrameBuffer(items) => {
                let pixels = items
                    .chunks(3)
                    .map(|chunk| Rgb888::new(chunk[0], chunk[1], chunk[2]))
                    .collect::<alloc::vec::Vec<Rgb888>>();

                // Streaming paths: drop instead of blocking the pump when the
                // display queue is saturated (new frames supersede old ones).
                let _ = display.try_send(DisplayWorkerMessage::FrameBuffer(pixels));
            }
            WebSocketIncomingMessage::Next => {
                display.send(DisplayWorkerMessage::Next).await;
            }
            WebSocketIncomingMessage::Animation(animation_enum, length_ms) => {
                info!("preview animation for {} ms (playlist paused behind it)", length_ms);
                display.send(DisplayWorkerMessage::Animation(animation_enum, length_ms)).await;
            }
            WebSocketIncomingMessage::Playlist { playlist, save } => {
                if save {
                    match serde_json::to_string(&playlist) {
                        Ok(json) => {
                            let _ = local_fs.write_text_file(DEFAULT_PLAYLIST_FILENAME, json).await;
                            info!("playlist saved to flash: {} item(s)", playlist.len());
                        }
                        Err(err) => {
                            error!("Could not serialise playlist JSON: {}", Debug2Format(&err));
                        }
                    }
                } else {
                    info!("playlist loaded (live only, {} item(s))", playlist.len());
                }

                display.send(DisplayWorkerMessage::Playlist(playlist)).await;
            }
            WebSocketIncomingMessage::Reset => {
                warn!("playlist RESET — deleting saved file, built-in default resumes");
                if let Err(err) = local_fs.delete(DEFAULT_PLAYLIST_FILENAME).await {
                    error!("Could not delete playlist JSON: {}", Debug2Format(&err));
                }

                display.send(DisplayWorkerMessage::Playlist(vec![])).await;
            }
            WebSocketIncomingMessage::ParamsBuffer(bytes) => {
                let params_buffer = bytes
                    .chunks(4)
                    .map(|chunk| AnimationParams::new(chunk[0], chunk[1], chunk[2], chunk[3]))
                    .collect::<alloc::vec::Vec<AnimationParams>>();

                let _ = display.try_send(DisplayWorkerMessage::ParamsBuffer(params_buffer));
            }
            WebSocketIncomingMessage::PowerLimit(power_limit) => {
                display.send(DisplayWorkerMessage::PowerLimit(power_limit)).await;
            }
        }
    }
}
