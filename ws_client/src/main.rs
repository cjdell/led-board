use std::{thread::sleep, time::Duration};
use tungstenite::{
    Bytes, ClientRequestBuilder, Message, Utf8Bytes, client::connect_with_config, connect, http::Uri,
    protocol::WebSocketConfig,
};

fn main() {
    env_logger::init();

    // let url = "ws://led-board.local/api/ws";
    let url = "ws://192.168.7.10/api/ws";

    let builder = ClientRequestBuilder::new(Uri::from_static(url)).with_sub_protocol("json");

    let (mut socket, response) = connect(builder).expect("cant connect");

    println!("Connected to the server");
    println!("Response HTTP code: {}", response.status());
    println!("Response contains the following headers:");
    for (header, _value) in response.headers() {
        println!("* {header}");
    }

    socket.send(Message::Text(Utf8Bytes::from_static(r#"{"PowerLimit":1.0}"#)));

    socket.send(Message::Text(Utf8Bytes::from_static(
        r#"{"Playlist":{"playlist":[["Intensity",10000]],"save":false}}"#,
    )));

    let mut t = 0;
    let mut buf = [0u8; 100 * 4];

    loop {
        // if let Ok(msg) = socket.read() {
        //     println!("Received: {msg}");
        // }

        for i in 0..100 {
            let beat = ((((t as f32) * 0.0005 * std::f32::consts::PI * 2.0).sin() + 1.0) * 0.5 * 255.0) as u8;
            // println!("Beat: {}", beat);
            buf[i * 4 + 0] = beat;
            t += 10;
        }

        socket
            .send(Message::Binary(Bytes::copy_from_slice(&buf)))
            .expect("Failed to send binary");

        socket.flush().expect("Unable to flush");

        sleep(Duration::from_millis(1_000));
    }
}
