//! UART link server: frame transport, RPC dispatch, stream slots, and events.
//!
//! The Pico is the client of this link; the C3/C6 answers its RPCs and relays
//! TCP streams. Frames leaving the C3 go through one queue + writer task so
//! RPC replies, events, and stream data interleave safely at frame
//! granularity. Incoming frames are decoded by a single reader task and
//! dispatched.
//!
//! ## Flow control (lossless by construction)
//!
//! Stream payload in each direction is gated by [`radio_link::StreamWindow`]:
//! the sender may keep at most [`radio_link::STREAM_WINDOW_FRAMES`] un-acked
//! *frames* per stream, and the receiver echoes a *cumulative* consumed-frame
//! count in `STREAM_ACK` frames as it hands frames to the consumer. The
//! per-stream queue here has exactly that many slots, so a Pico that sends
//! correctly can never overflow it — the invariant holds for any frame sizes
//! (a byte window would not: small frames would pipeline past the slot
//! count). A slow browser therefore pauses picoserve — it does not lose
//! bytes.
//!
//! Nothing on the link is dropped on pressure any more: `FRAME_TX.send()`
//! waits, and the writer task always drains because the UART always drains.
//! Every anomaly that would once have corrupted a byte stream (CRC failure,
//! queue overrun, a peer that stopped acking) now *closes that stream*, which
//! costs the client one retriable request. Stream teardown destroys all
//! window state, so no accounting error can leak across connections.

use alloc::{string::String, vec::Vec};
use core::sync::atomic::{AtomicBool, AtomicU16, AtomicU32, AtomicUsize, Ordering};

use embassy_executor::Spawner;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use embassy_time::Timer;
use embedded_io_async::Write as _;
use esp_hal::dma::DmaRxBuf;
use esp_hal::uart::uhci::UhciRx;
use esp_hal::uart::UartTx;

use radio_link::frame_type::*;
use radio_link::{
    Frame, HelloInfo, RadioEvent, RadioMode, RadioStatus, Reply, Rpc, RpcRequest, RpcResponse, SEQ_SIZE,
    StreamOpenInfo, StreamWindow, MAX_PAYLOAD, PROTOCOL_VERSION, STREAM_WINDOW_FRAMES,
};

use crate::state::{self, SharedMutex};

// ---------------------------------------------------------------------------
// Transport
// ---------------------------------------------------------------------------

/// Size of the UHCI receive buffer (bytes).
///
/// This is also the UHCI `chunk_limit`: a DMA transfer completes when the
/// buffer is full, or earlier when the line goes idle (`idle_eof`), so short
/// frames are handed over immediately instead of waiting to fill 4 KB. While a
/// transfer is in flight the DMA engine drains the UART FIFO without CPU
/// involvement, which is what makes high baud rates viable under WiFi interrupt
/// load.
pub const RX_DMA_CHUNK: u16 = 4092;

/// Frames waiting to hit the wire. `send_frame` waits for room rather than
/// dropping; the writer task always drains (the UART always does), so this
/// queue is link backpressure, not a lossy buffer. Depth is bounded on heap:
/// every queued frame owns a Vec payload (worst case 1 KB), and the radio
/// heap is shared with the stream queues and WiFi.
static FRAME_TX: Channel<CriticalSectionRawMutex, Frame, 16> = Channel::new();

/// Teardown frames bypass the data queue: `close_stream` runs from the rx
/// task, which must never await behind a jammed `FRAME_TX` — the UHCI DMA is
/// unarmed while the rx task waits, and at 921600 the FIFO overflows within
/// ~1.4 ms, cascading corruption. Losing a close frame is recoverable (the
/// peer's 30 s window timeout unwinds the stream), so `try_send` is the
/// right contract here.
static CLOSE_TX: Channel<CriticalSectionRawMutex, Frame, 8> = Channel::new();

/// Queued RPC requests from the Pico (worker task consumes them).
static RPC_QUEUE: Channel<CriticalSectionRawMutex, RpcRequest, 4> = Channel::new();

/// Stream receive queues: an empty `Vec` marks "closed". Depth equals
/// [`STREAM_WINDOW_FRAMES`] — the invariant that makes receive overflow
/// structurally impossible for a frame-count-gated sender.
pub type StreamQueue = Channel<CriticalSectionRawMutex, Vec<u8>, STREAM_WINDOW_FRAMES>;

/// Per-stream slots. Sized for the relay acceptors (3 STA + 2 AP) with
/// margin. A slot is claimed at stream open and only reclaimed after the
/// relay task finishes with it, so a long-lived board never leaks or mixes
/// stream state.
const STREAM_POOL_SIZE: usize = 8;

/// Sender-side flow-control state for one stream.
struct TxState {
    win: StreamWindow,
    /// Next outbound data-frame sequence (per stream, wraps).
    next_seq: u16,
    /// Stream is gone (torn down): a blocked sender aborts instead of
    /// stalling forever.
    closed: bool,
}

/// Everything this link tracks about one relayed connection.
pub struct StreamEntry {
    /// Stream id; 0 means the slot is free.
    id: AtomicU16,
    /// Teardown started: late data is dropped, the consumer gets Eof.
    closing: AtomicBool,
    /// Data arriving from the Pico (toward the browser).
    pub queue: StreamQueue,
    /// Cumulative frames taken from `queue` and handed to the TCP socket.
    /// Echoed to the Pico in `STREAM_ACK` frames (never reset mid-stream).
    rx_frames: AtomicU32,
    /// Next expected inbound data-frame sequence (the rx task is the only
    /// writer). A gap proves frames vanished on the wire.
    rx_seq: AtomicU16,
    /// Window gating bytes we push toward the Pico (toward picoserve).
    tx: SharedMutex<TxState>,
}

impl StreamEntry {
    const NEW: Self = Self {
        id: AtomicU16::new(0),
        closing: AtomicBool::new(false),
        queue: Channel::new(),
        rx_frames: AtomicU32::new(0),
        rx_seq: AtomicU16::new(1),
        tx: SharedMutex::new(TxState {
            win: StreamWindow::new(STREAM_WINDOW_FRAMES as u32),
            next_seq: 1,
            closed: false,
        }),
    };

    pub fn id(&self) -> u16 {
        self.id.load(Ordering::Relaxed)
    }
}

static STREAM_POOL: [StreamEntry; STREAM_POOL_SIZE] = [StreamEntry::NEW; STREAM_POOL_SIZE];

/// Next stream id. (The rv32imc core has no lock-free RMW atomics, so this is
/// a plain mutex — allocation happens once per connection.)
static NEXT_STREAM: SharedMutex<u32> = SharedMutex::new(0);

/// Status-LED commands from RPCs.
static LED_TX: Channel<CriticalSectionRawMutex, bool, 2> = Channel::new();

/// Send a frame to the Pico. Never drops: waits for queue space, which the
/// writer task always frees at line rate.
pub async fn send_frame(frame: Frame) {
    FRAME_TX.send(frame).await;
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

// ---------------------------------------------------------------------------
// Stream slots
// ---------------------------------------------------------------------------

fn find_entry(id: u16) -> Option<&'static StreamEntry> {
    if id == 0 {
        return None;
    }
    STREAM_POOL.iter().find(|e| e.id.load(Ordering::Relaxed) == id)
}

/// Register a relayed stream and tell the Pico it opened.
pub async fn open_stream(port: u16) -> Option<&'static StreamEntry> {
    let id = {
        let mut next = NEXT_STREAM.lock().await;
        *next = if *next >= u16::MAX as u32 { 1 } else { *next + 1 };
        *next as u16
    };

    let entry = STREAM_POOL
        .iter()
        .find(|e| e.id.compare_exchange(0, id, Ordering::Relaxed, Ordering::Relaxed).is_ok())?;

    // Fresh state for this connection. (release_stream already drained; be
    // paranoid anyway — a stale chunk would poison a byte stream.)
    while entry.queue.try_receive().is_ok() {}
    entry.rx_frames.store(0, Ordering::Relaxed);
    entry.rx_seq.store(1, Ordering::Relaxed);
    entry.closing.store(false, Ordering::Relaxed);
    {
        let mut tx = entry.tx.lock().await;
        tx.win = StreamWindow::new(STREAM_WINDOW_FRAMES as u32);
        tx.next_seq = 1;
        tx.closed = false;
    }

    let mut frame = Frame::empty(STREAM_OPEN);
    frame.stream = id;
    if let Ok(payload) = serde_json::to_vec(&StreamOpenInfo { port }) {
        frame.payload = payload;
    }
    send_frame(frame).await;

    Some(entry)
}

/// Begin teardown of a stream (idempotent; callable from either side's
/// error paths): stop accepting data, wake any blocked sender, tell the Pico,
/// and put an Eof marker to the consumer. The slot itself is reclaimed by
/// [`release_stream`] once the relay task is finished with it — that ordering
/// is what guarantees a dying stream's bytes never bleed into its successor.
pub async fn close_stream(id: u16) {
    let Some(entry) = find_entry(id) else { return };
    if entry.closing.swap(true, Ordering::AcqRel) {
        return; // teardown already started by whoever got here first
    }

    entry.tx.lock().await.closed = true;

    if let Ok(frame) = Frame::new(STREAM_CLOSE, id, Vec::new()) {
        if CLOSE_TX.try_send(frame).is_err() {
            log::warn!("link: close frame for stream {} dropped (peer unwinds via timeout)", id);
        }
    }

    // Force the Eof marker in. The consumer is actively draining (single
    // executor: it runs while we wait), so a momentarily full queue frees up
    // within one of these steps.
    for _ in 0..5 {
        if entry.queue.try_send(Vec::new()).is_ok() {
            return;
        }
        Timer::after_millis(20).await;
    }
    log::warn!("link: stream {} Eof marker lost — relay must exit via its socket", id);
}

/// Reclaim a finished stream slot. The relay task calls this as the very last
/// thing it does with the entry.
pub fn release_stream(id: u16) {
    let Some(entry) = find_entry(id) else { return };
    while entry.queue.try_receive().is_ok() {}
    entry.rx_frames.store(0, Ordering::Relaxed);
    entry.rx_seq.store(1, Ordering::Relaxed);
    entry.closing.store(false, Ordering::Relaxed);
    entry.id.store(0, Ordering::Relaxed);
    // A wake permit left over from teardown only costs the next connection
    // one harmless spurious re-check of its freshly reset window.
}

/// Feed payload from a Pico stream frame (payload carries the leading
/// sequence number; the queued chunk is body-only).
pub async fn feed_stream(id: u16, payload: Vec<u8>) {
    let Some(entry) = find_entry(id) else { return };
    if entry.closing.load(Ordering::Relaxed) {
        return; // teardown started: data for a dead stream is simply gone
    }

    let Some((seq, body)) = radio_link::split_data(&payload) else {
        log::warn!("link: stream {} data frame malformed — closing", id);
        close_stream(id).await;
        return;
    };

    // Sequence check: a gap is proof bytes vanished on the wire (noise can
    // eat whole frames without ever tripping a CRC, because the decoder
    // skips an orphaned tail as garbage).
    let want = entry.rx_seq.load(Ordering::Relaxed);
    if seq != want {
        log::warn!("link: stream {} seq gap (got {} want {}) — bytes lost", id, seq, want);
        close_stream(id).await;
        return;
    }
    entry.rx_seq.store(want.wrapping_add(1), Ordering::Relaxed);

    // The Pico is gated to `STREAM_WINDOW_FRAMES` un-acked frames per stream
    // and this queue has exactly that many slots, so there is always room.
    // Failing here means the invariant was violated (firmware skew or a bug)
    // — tear the stream down recoverably rather than wedge or corrupt it.
    if entry.queue.try_send(body.to_vec()).is_err() {
        log::error!("link: stream {} overran flow-control window", id);
        close_stream(id).await;
    }
}

/// Report that one queue frame was consumed (handed to the TCP socket) and
/// echo the cumulative frame count to the Pico so its send window opens
/// again. Counting frames (not bytes) is what keeps the sender's gate and
/// this queue's slot count exactly aligned for any frame sizes.
pub async fn ack_stream(id: u16) {
    let Some(entry) = find_entry(id) else { return };
    if entry.closing.load(Ordering::Relaxed) {
        return;
    }
    let consumed = entry.rx_frames.fetch_add(1, Ordering::Relaxed).wrapping_add(1);
    send_frame(Frame::ack(id, consumed)).await;
}

/// Send bytes on an open stream, chunked to fit frames and gated by the
/// Pico's window. Returns false once the stream has gone (closed by either
/// side, or the Pico stopped acking for 30 s) — the caller must abandon the
/// connection rather than assume the bytes landed.
pub async fn send_stream_data(entry: &'static StreamEntry, data: &[u8]) -> bool {
    let id = entry.id();
    for chunk in data.chunks(MAX_PAYLOAD - SEQ_SIZE) {
        // Wait for window space. A blocked sender re-checks every 5 ms —
        // only ever while backlogged, and at that cadence the window drains
        // as fast as the wire allows; the 30 s cap guarantees we abort when
        // the peer is dead and never acking at all.
        let mut waited: u32 = 0;
        let frame = loop {
            let gated = {
                let mut tx = entry.tx.lock().await;
                if tx.closed {
                    return false;
                }
                if tx.win.can_send_frame() {
                    tx.win.note_sent_frame();
                    let seq = tx.next_seq;
                    tx.next_seq = tx.next_seq.wrapping_add(1);
                    Some(Frame::data(id, seq, chunk))
                } else {
                    None
                }
            };
            match gated {
                Some(Ok(frame)) => break frame,
                Some(Err(_)) => return false,
                None => {
                    Timer::after_millis(5).await;
                    waited += 1;
                    if waited >= 6000 {
                        log::warn!("link: stream {} window closed 30s — Pico stopped acking", id);
                        close_stream(id).await;
                        return false;
                    }
                }
            }
        };
        send_frame(frame).await;
    }
    true
}

// ---------------------------------------------------------------------------
// Tasks
// ---------------------------------------------------------------------------

#[embassy_executor::task]
async fn tx_task(mut tx: UartTx<'static, esp_hal::Async>) {
    use embassy_futures::select::{Either, select};
    loop {
        // Teardown frames over data frames whenever both are ready: they are
        // tiny, and prompt STREAM_CLOSE delivery stops a stranded sender on
        // the Pico from pumping a dead window.
        let frame = match select(CLOSE_TX.receive(), FRAME_TX.receive()).await {
            Either::First(f) => f,
            Either::Second(f) => f,
        };
        let bytes = frame.encode();
        if let Err(err) = tx.write_all(&bytes).await {
            log::error!("link: uart write error {:?}", err);
            embassy_time::Timer::after_millis(50).await;
        }
    }
}

#[embassy_executor::task]
async fn rx_task(
    mut rx: UhciRx<'static, esp_hal::Async>,
    mut dma_rx_a: DmaRxBuf,
    dma_rx_b: DmaRxBuf,
) {
    let mut decoder = radio_link::Decoder::new();

    // Ping-pong DMA: the moment a transfer completes we re-arm the *other*
    // buffer BEFORE touching any bytes. The old single-buffer loop re-armed
    // after decoding + handle_frame().await, so the UHCI chain sat disarmed
    // while the 128-byte RX FIFO soaked up the arriving stream — bytes were
    // lost mid-frame under load ("header valid, CRC bad" frames). Only the
    // executor wake latency now sits between the DMA and the next buffer.
    let mut idle = dma_rx_b;
    let mut transfer = loop {
        match rx.read(dma_rx_a) {
            Ok(transfer) => break transfer,
            Err((err, rx_back, buf)) => {
                log::warn!("link: uart dma start error {:?}", err);
                rx = rx_back;
                dma_rx_a = buf;
                embassy_time::Timer::after_millis(10).await;
            }
        }
    };

    loop {
        transfer.wait_for_done().await;
        let (result, rx_back, ready) = transfer.wait();
        rx = rx_back;

        // Re-arm first — decoding must never run with the DMA disarmed.
        transfer = loop {
            match rx.read(idle) {
                Ok(transfer) => break transfer,
                Err((err, rx_back, buf)) => {
                    log::warn!("link: uart dma start error {:?}", err);
                    rx = rx_back;
                    idle = buf;
                    embassy_time::Timer::after_millis(10).await;
                }
            }
        };
        // Park the just-filled buffer back in the pool before any branch.
        idle = ready;

        if let Err(err) = result {
            log::warn!("link: uart dma error {:?}", err);
            decoder.reset();
            embassy_time::Timer::after_millis(10).await;
            continue;
        }

        let n = idle.number_of_received_bytes();
        if n == 0 {
            // The idle-EOF mode can end a transfer with nothing received;
            // don't spin hot if that ever happens.
            embassy_time::Timer::after_millis(1).await;
            continue;
        }

        RX_BYTES.fetch_add(n, Ordering::Relaxed);
        decoder.push(&idle.as_slice()[..n]);

        loop {
            match decoder.next_frame() {
                Ok(Some(frame)) => {
                    RX_FRAMES.fetch_add(1, Ordering::Relaxed);
                    handle_frame(frame).await;
                }
                Ok(None) => break,
                Err(corrupt) => {
                    // Bytes belonging to that stream were lost — close it.
                    // (The Pico tears the mirror stream down when it sees our
                    // matching STREAM_CLOSE, so both directions unwind.)
                    log::warn!(
                        "link: corrupt frame type={} stream={} — closing stream",
                        corrupt.ftype,
                        corrupt.stream
                    );
                    if corrupt.stream != 0 {
                        close_stream(corrupt.stream).await;
                    }
                }
            }
        }
    }
}

// Link RX counters, surfaced in the supervisor heartbeat — one glance shows
// whether the UART link is alive, dead, or stuck.
static RX_BYTES: AtomicUsize = AtomicUsize::new(0);
static RX_FRAMES: AtomicUsize = AtomicUsize::new(0);

/// (bytes, frames) received on the link UART since boot.
pub fn rx_counters() -> (usize, usize) {
    (RX_BYTES.load(Ordering::Relaxed), RX_FRAMES.load(Ordering::Relaxed))
}

async fn handle_frame(frame: Frame) {
    match frame.ftype {
        HELLO => {
            state::pico_seen();
            if let Ok(hello) = frame.payload_json::<HelloInfo>() {
                if hello.protocol != PROTOCOL_VERSION {
                    log::warn!(
                        "link: PROTOCOL MISMATCH pico={} c6={}",
                        hello.protocol,
                        PROTOCOL_VERSION
                    );
                }
            }
            let ack = HelloInfo {
                board: String::from("esp32c3"),
                firmware: String::from(crate::state::FIRMWARE),
                protocol: PROTOCOL_VERSION,
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
        STREAM_ACK => {
            if let Some(consumed) = frame.consumed() {
                apply_ack(frame.stream, consumed).await;
            }
        }
        STREAM_CLOSE => close_stream(frame.stream).await,
        _ => {}
    }
}

/// Apply a cumulative consume-ack to the stream's send window.
async fn apply_ack(id: u16, consumed: u32) {
    let Some(entry) = find_entry(id) else { return };

    let applied = entry.tx.lock().await.win.note_ack(consumed);
    if !applied {
        log::warn!("link: bogus ack frames={} for stream {}", consumed, id);
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

/// Start the link tasks with owned, async UART halves, the two ping-pong RX
/// DMA buffers, and the status LED.
pub fn start(
    spawner: &Spawner,
    rx: UhciRx<'static, esp_hal::Async>,
    tx: UartTx<'static, esp_hal::Async>,
    dma_rx_a: DmaRxBuf,
    dma_rx_b: DmaRxBuf,
    led: esp_hal::gpio::Output<'static>,
) {
    spawner.spawn(tx_task(tx).unwrap());
    spawner.spawn(rx_task(rx, dma_rx_a, dma_rx_b).unwrap());
    spawner.spawn(rpc_task().unwrap());
    spawner.spawn(led_task(led).unwrap());
}
