//! The web server thread: tokio accepts on localhost, picoserve serves the
//! shared led-core router — same routes, handlers, WebSocket, and OTA path
//! the Pico runs. picoserve's `tokio` runtime support (`Server::new_tokio`,
//! `Socket for tokio::net::TcpStream`) stands in for embassy-net here, the
//! same way the radio tunnel does on the Pico.
//!
//! A `current_thread` runtime + `LocalSet` mirrors the embedded topology:
//! one executor running all connection futures, so router state needs
//! neither `Sync` nor `Send`, exactly like the single-executor device.

use std::net::SocketAddr;

use led_core::tasks::http::{AppProps, CONFIG};
use picoserve::AppRouter;
use picoserve::Server;
use tokio::net::TcpListener;

use crate::board::SimBoard;

/// Accept connections forever, spawning one picoserve task per connection.
pub async fn accept_loop(app: &'static AppRouter<AppProps<SimBoard>>, addr: SocketAddr) -> std::io::Result<()> {
    let listener = TcpListener::bind(addr).await?;

    println!("(sim) web UI: http://{addr}/   (WebSocket at {addr}/api/ws)");

    loop {
        let (stream, _peer) = listener.accept().await?;

        tokio::task::spawn_local(async move {
            // Same 8 KB HTTP buffer shape the Pico's tunnel path uses.
            let mut http_buffer = vec![0u8; 8 * 1024];

            match Server::new_tokio(app, &CONFIG, http_buffer.as_mut()).serve(stream).await {
                Ok(info) => println!("(sim) connection closed after {} requests", info.handled_requests_count),
                Err(err) => println!("(sim) connection ended: {err:?}"),
            }
        });
    }
}
