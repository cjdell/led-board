//! One-shot WS control poke for bench forensics:
//!   cargo run -p ws_client --example diag -- [url] [json-message]
//!
//! Defaults to setting the panel to a fully static Blank playlist — the
//! driver-vs-content discriminator: a static frame cannot change pixels,
//! so anything flickering on the panel is a bitstream/timing artefact.

use tungstenite::{ClientRequestBuilder, Message, Utf8Bytes, connect, http::Uri};

fn main() {
    let url = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "ws://10.3.2.212/api/ws".into());
    let msg = std::env::args().nth(2).unwrap_or_else(|| {
        r#"{"Playlist":{"playlist":[["Blank",60000]],"save":false}}"#.into()
    });

    let builder = ClientRequestBuilder::new(Uri::try_from(url.as_str()).unwrap()).with_sub_protocol("json");
    let (mut socket, response) = connect(builder).expect("connect");
    println!("connected ({}), sending: {msg}", response.status());

    socket
        .send(Message::Text(Utf8Bytes::from(msg)))
        .expect("send");
    socket.flush().expect("flush");

    // Drain a few frames so the server definitely processes the send
    // (picoserve reads client frames between pushes).
    for _ in 0..3 {
        let _ = socket.read();
    }
    println!("done — panel state now driven by the message above");
}
