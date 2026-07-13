use crate::types::*;
use alloc::vec::Vec;
use animations::TOTAL_PIXELS;
use defmt::*;
use picoserve::{
    futures::Either,
    response::ws::{Message, SocketRx, SocketTx, WebSocketCallback},
};
use utils::sleep;

pub struct WebSocketHandler {
    web_socket_incoming_sender: WebSocketIncomingSender,
}

impl WebSocketHandler {
    pub fn new(web_socket_incoming_sender: WebSocketIncomingSender) -> Self {
        Self {
            web_socket_incoming_sender,
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

        let close_reason = loop {
            let message = match rx.next_message(&mut message_buffer, sleep(1_000_000)).await? {
                Either::First(Ok(message)) => message,
                Either::First(Err(error)) => {
                    warn!("Websocket error");
                    break Some((error.code(), "Websocket Error"));
                }
                Either::Second(()) => {
                    continue;
                }
            };

            // info!("Message: {:?}", message);

            match message {
                Message::Text(message) => {
                    let message = match serde_json::from_str(message) {
                        Ok(message) => message,
                        Err(err) => {
                            error!("Serde Error: {:?}", defmt::Debug2Format(&err));
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
            };
        };

        let close_fut = tx.close(close_reason).await;

        info!("WebSocket closed");

        close_fut
    }
}
