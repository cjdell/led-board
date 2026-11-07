//! UART link server: frame transport, RPC dispatch, stream slots, and events.
//!
//! The Pico is the client of this link; the C3 answers its RPCs and relays
//! TCP streams. Frames leaving the C3 go through one queue + writer task so
//! RPC replies, events, and stream data interleave safely at frame
//! granularity. Incoming frames are decoded by a single reader task and
//! dispatched.

use alloc::{boxed::Box, string::String, vec::Vec};

use embassy_executor::Spawner;
use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex,
    channel::Channel,
};
use embassy_time::Duration;
use embedded_io_async::Write as _;
use esp_hal::uart::{UartRx, UartTx};

use radio_link::frame_type::*;
use radio_link::{
    Frame, HelloInfo, RadioEvent, RadioMode, RadioStatus, Reply, Rpc, RpcRequest, RpcResponse, StreamOpenInfo,
};

use crate::state::{self, FIRMWARE};

// ---------------------------------------------------------------------------
// Transport
// ---------------------------------------------------------------------------

/// Frames waiting to hit the wire. Depth 32 ≈ 32 KB worst case.
static FRAME_TX: Channel<CriticalSectionRawMutex, Frame, 32> = Channel::new();

/// Queued RPC requests from the Pico (worker task consumes them).
static RPC_QUEUE: Channel<CriticalSectionRawMutex, RpcRequest, 4> = Channel::new();

/// Stream receive queues: (stream id, queue). An empty `Vec` means "closed".
pub type StreamQueue = Channel<CriticalSectionRawMutex, Vec<u8>, 8>;
static STREAMS: state::SharedMutex<Vec<(u16, &'static StreamQueue)>> = state::SharedMutex::new(Vec::new());

/// Next stream id. (The rv32imc core has no lock-free RMW atomics, so this is
/// a plain mutex — allocation happens once per connection.)
static NEXT_STREAM: state::SharedMutex<u32> = state::SharedMutex::new(0);

/// Status-LED commands from RPCs.
static LED_TX: Channel<CriticalSectionRawMutex, bool, 2> = Channel::new();

/// Send a frame to the Pico (waits briefly for queue space).
pub async fn send_frame(frame: Frame) {
    if embassy_time::with_timeout(Duration::from_secs(2), FRAME_TX.send(frame))
        .await
        .is_err()
    {
        log::warn!("link: tx queue full, dropping frame");
    }
}

pub async fn push_event(event: RadioEvent) {
    if let Ok(frame) = Frame::json(EVENT, &event) {
        send_frame(frame).await;
    }
}

/// Called by [`crate::state::publish`] when the status snapshot changes.
pub async fn status_changed(status: &RadioStatus) {
    push_event(RadioEvent::Status(status.clone())).await;
}

/// Tell the Pico a stream is gone and deregister it.
pub async fn close_stream(id: u16) {
    if let Ok(frame) = Frame::new(STREAM_CLOSE, id, Vec::new()) {
        send_frame(frame).await;
    }
    let mut streams = STREAMS.lock().await;
    streams.retain(|(sid, _)| *sid != id);
}

// ---------------------------------------------------------------------------
// Stream slots
// ---------------------------------------------------------------------------

/// Register a relayed stream and tell the Pico it opened.
pub async fn open_stream(port: u16) -> Option<(u16, &'static StreamQueue)> {
    let id = {
        let mut next = NEXT_STREAM.lock().await;
        *next = if *next >= u16::MAX as u32 { 1 } else { *next + 1 };
        *next as u16
    };

    let queue: &'static StreamQueue = Box::leak(Box::new(StreamQueue::new()));

    {
        let mut streams = STREAMS.lock().await;
        streams.retain(|(sid, _)| *sid != id);
        streams.push((id, queue));
    }

    let mut frame = Frame::empty(STREAM_OPEN);
    frame.stream = id;
    if let Ok(payload) = serde_json::to_vec(&StreamOpenInfo { port }) {
        frame.payload = payload;
    }
    send_frame(frame).await;

    Some((id, queue))
}

/// Feed payload from a Pico stream frame. An empty payload means close.
pub async fn feed_stream(id: u16, payload: Vec<u8>) {
    let queue = {
        let streams = STREAMS.lock().await;
        streams.iter().find(|(sid, _)| *sid == id).map(|(_, q)| *q)
    };
    let Some(queue) = queue else { return };

    if queue.try_send(payload).is_err() {
        log::warn!("link: stream {} consumer stalled, closing", id);
        let _ = queue.try_send(Vec::new());
    }
}

/// Send raw bytes on an open stream, chunked to fit frames.
pub async fn send_stream_data(id: u16, data: &[u8]) {
    for chunk in data.chunks(radio_link::MAX_PAYLOAD) {
        if let Ok(frame) = Frame::new(STREAM_DATA, id, chunk.to_vec()) {
            send_frame(frame).await;
        }
    }
}

// ---------------------------------------------------------------------------
// Tasks
// ---------------------------------------------------------------------------

#[embassy_executor::task]
async fn tx_task(mut tx: UartTx<'static, esp_hal::Async>) {
    loop {
        let frame = FRAME_TX.receive().await;
        let bytes = frame.encode();
        if let Err(err) = tx.write_all(&bytes).await {
            log::error!("link: uart write error {:?}", err);
            embassy_time::Timer::after_millis(50).await;
        }
    }
}

#[embassy_executor::task]
async fn rx_task(mut rx: UartRx<'static, esp_hal::Async>) {
    let mut decoder = radio_link::Decoder::new();
    let mut buf = [0u8; 512];

    loop {
        let read = embedded_io_async::Read::read(&mut rx, &mut buf).await;
        match read {
            Ok(0) => {}
            Ok(n) => {
                decoder.push(&buf[..n]);
                while let Some(frame) = decoder.next_frame() {
                    handle_frame(frame).await;
                }
            }
            Err(err) => {
                log::warn!("link: uart read error {:?}", err);
                decoder.reset();
                embassy_time::Timer::after_millis(10).await;
            }
        }
    }
}

async fn handle_frame(frame: Frame) {
    match frame.ftype {
        HELLO => {
            state::pico_seen();
            let ack = HelloInfo {
                board: String::from("esp32c3"),
                firmware: String::from(FIRMWARE),
                protocol: radio_link::PROTOCOL_VERSION,
            };
            if let Ok(ack) = Frame::json(HELLO_ACK, &ack) {
                send_frame(ack).await;
            }
            // Re-push current status so the Pico resyncs after its reset.
            if let Some(status) = state::snapshot().await {
                push_event(RadioEvent::Status(status)).await;
            }
        }
        KEEPALIVE => {
            state::pico_seen();
        }
        RPC_REQ => {
            state::pico_seen();
            match frame.payload_json::<RpcRequest>() {
                Ok(req) => {
                    if RPC_QUEUE.try_send(req).is_err() {
                        log::warn!("link: rpc queue full");
                    }
                }
                Err(err) => log::warn!("link: bad RPC_REQ {:?}", err),
            }
        }
        STREAM_DATA => {
            if frame.stream != 0 {
                feed_stream(frame.stream, frame.payload).await;
            }
        }
        STREAM_CLOSE => feed_stream(frame.stream, Vec::new()).await,
        _ => {}
    }
}

#[embassy_executor::task]
async fn led_task(mut led: esp_hal::gpio::Output<'static>) {
    loop {
        let on = LED_TX.receive().await;
        // Onboard LEDs on C3 boards are usually active-low.
        if on {
            led.set_low();
        } else {
            led.set_high();
        }
    }
}

// ---------------------------------------------------------------------------
// RPC server
// ---------------------------------------------------------------------------

#[embassy_executor::task]
async fn rpc_task() {
    loop {
        let RpcRequest { id, rpc } = RPC_QUEUE.receive().await;

        let reply = serve_rpc(rpc).await;

        if let Some(reply) = reply {
            let response = RpcResponse { id, reply };
            if let Ok(frame) = Frame::json(RPC_RESP, &response) {
                send_frame(frame).await;
            }
        }
    }
}

async fn serve_rpc(rpc: Rpc) -> Option<Reply> {
    match rpc {
        Rpc::Scan => {
            let entries = crate::supervisor::request_scan().await;
            Some(Reply::ScanResults(entries))
        }
        Rpc::Join { ssid, pass } => {
            crate::supervisor::join_network(&ssid, &pass).await;
            Some(Reply::Status(current_status().await))
        }
        Rpc::Forget { ssid } => {
            crate::supervisor::forget_network(&ssid).await;
            Some(Reply::Status(current_status().await))
        }
        Rpc::SetNetworks { networks, ap_ssid, mode } => {
            crate::supervisor::apply_networks(networks, ap_ssid, mode).await;
            Some(Reply::Status(current_status().await))
        }
        Rpc::GetNetworks => Some(crate::supervisor::stored_networks().await),
        Rpc::Status => Some(Reply::Status(current_status().await)),
        Rpc::Led(on) => {
            let _ = LED_TX.try_send(on);
            None // fire and forget
        }
        Rpc::UseStation => {
            crate::supervisor::use_station().await;
            Some(Reply::Status(current_status().await))
        }
    }
}

async fn current_status() -> RadioStatus {
    state::snapshot()
        .await
        .unwrap_or_else(|| state::build_status(RadioMode::Station, None, None, None, None, None))
}

// ---------------------------------------------------------------------------
// Start
// ---------------------------------------------------------------------------

/// Start the link tasks with owned, async UART halves and the status LED.
pub fn start(
    spawner: &Spawner,
    rx: UartRx<'static, esp_hal::Async>,
    tx: UartTx<'static, esp_hal::Async>,
    led: esp_hal::gpio::Output<'static>,
) {
    spawner.spawn(tx_task(tx).unwrap());
    spawner.spawn(rx_task(rx).unwrap());
    spawner.spawn(rpc_task().unwrap());
    spawner.spawn(led_task(led).unwrap());
}
