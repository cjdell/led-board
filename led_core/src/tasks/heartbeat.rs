//! The heartbeat: a short status-LED blink every five seconds, so "the
//! firmware loop is alive" is visible on the board itself (Pico: cyw43
//! or C3 LED; simulator: window title).

use embassy_time::{Duration, Timer};

use crate::board::Board;

pub async fn run<B: Board>(board: B) -> ! {
    let delay = Duration::from_millis(5_000);
    let blink = Duration::from_millis(100);

    let mut beats = 0u32;

    loop {
        Timer::after(delay).await;

        board.blink(true).await;
        Timer::after(blink).await;
        board.blink(false).await;

        // Every ~30 s: a liveness line for the log streams (and the console
        // breadcrumb ring), cheap enough to leave in forever.
        beats += 1;
        if beats % 6 == 0 {
            embedded_tools::info!("hb: alive, beat {} (up {} s)", beats, beats as u64 * 5);
        }
    }
}
