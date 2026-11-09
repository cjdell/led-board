//! The heartbeat: a short status-LED blink every five seconds, so "the
//! firmware loop is alive" is visible on the board itself (Pico: cyw43
//! or C3 LED; simulator: window title).

use embassy_time::{Duration, Timer};

use crate::board::Board;

pub async fn run<B: Board>(board: B) -> ! {
    let delay = Duration::from_millis(5_000);
    let blink = Duration::from_millis(100);

    loop {
        Timer::after(delay).await;

        board.blink(true).await;
        Timer::after(blink).await;
        board.blink(false).await;
    }
}
