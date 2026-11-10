use crate::types::{PlaylistWatch, StatusWatch, WebSocketIncomingMessage, WebSocketIncomingSender};
use alloc::format;
use alloc::vec::Vec;
use animations::TOTAL_PIXELS;
use embedded_tools::{Debug2Format, error, info, sleep, warn};
use picoserve::{
    futures::Either,
    response::ws::{Message, SocketRx, SocketTx, WebSocketCallback},
};

/// Status/playlist push cadence: the rx timeout doubles as the "tick" that
/// flushes progress updates (elapsed ms) to every connected client.
const PUSH_TICK_MS: u64 = 1_000;

pub struct WebSocketHandler {
    web_socket_incoming_sender: WebSocketIncomingSender,
    status_watch: &'static StatusWatch,
    playlist_watch: &'static PlaylistWatch,
}

impl WebSocketHandler {
    pub fn new(
        web_socket_incoming_sender: WebSocketIncomingSender,
        status_watch: &'static StatusWatch,
        playlist_watch: &'static PlaylistWatch,
    ) -> Self {
        Self {
            web_socket_incoming_sender,
            status_watch,
            playlist_watch,
        }
    }
}

impl WebSocketCallback for WebSocketHandler {
    async fn run<R: picoserve::io::Read, W: picoserve::io::Write<Error = R::Error>>(
        self,
        mut rx: SocketRx<R>,
        mut tx: SocketTx<W>,
    ) -> Result<(), W::Error> {
        use Message;

        info!("WebSocket opened");

        let mut message_buffer = Vec::new();
        message_buffer.resize(8 * 1024, 0u8);

        // Live-state feeds for this connection. Receiver slots are shared
        // with GET /api/status — if the board is saturated, this connection
        // simply works receive-only (control still flows).
        let mut status_rx = self.status_watch.receiver();
        let mut playlist_rx = self.playlist_watch.receiver();

        // A fresh page should paint the board state immediately, not after
        // the first tick.
        if let Some(rx) = status_rx.as_mut()
            && let Some(json) = rx.try_get()
        {
            tx.send_text(&format!("{{\"status\":{json}}}")).await?;
        }

        if let Some(rx) = playlist_rx.as_mut()
            && let Some(json) = rx.try_get()
        {
            tx.send_text(&format!("{{\"playlist\":{json}}}")).await?;
        }

        let close_reason = loop {
            let message = match rx.next_message(&mut message_buffer, sleep(PUSH_TICK_MS)).await? {
                Either::First(Ok(message)) => message,
                Either::First(Err(error)) => {
                    warn!("Websocket error");
                    break Some((error.code(), "Websocket Error"));
                }
                Either::Second(()) => {
                    // Tick: forward anything the display worker published.
                    if let Some(rx) = status_rx.as_mut()
                        && let Some(json) = rx.try_changed()
                    {
                        tx.send_text(&format!("{{\"status\":{json}}}")).await?;
                    }

                    if let Some(rx) = playlist_rx.as_mut()
                        && let Some(json) = rx.try_changed()
                    {
                        tx.send_text(&format!("{{\"playlist\":{json}}}")).await?;
                    }

                    continue;
                }
            };

            // info!("Message: {:?}", message);

            match message {
                Message::Text(message) => {
                    let message = match serde_json::from_str(message) {
                        Ok(message) => message,
                        Err(err) => {
                            error!("Serde Error: {:?}", Debug2Format(&err));
                            continue;
                        }
                    };
                    self.web_socket_incoming_sender.send(message).await;
                }
                Message::Binary(message) => {
                    if message.len() == 100 * 4 {
                        self.web_socket_incoming_sender
                            .send(WebSocketIncomingMessage::ParamsBuffer(message.to_vec()))
                            .await;
                    }

                    if message.len() == TOTAL_PIXELS * 3 {
                        self.web_socket_incoming_sender
                            .send(WebSocketIncomingMessage::FrameBuffer(message.to_vec()))
                            .await;
                    }
                }
                Message::Close(reason) => {
                    info!("Websocket close reason: {:?}", reason);
                    break None;
                }
                Message::Ping(ping) => tx.send_pong(ping).await?,
                Message::Pong(_) => (),
            }
        };

        let close_fut = tx.close(close_reason).await;

        info!("WebSocket closed");

        close_fut
    }
}
