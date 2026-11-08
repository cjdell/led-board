//! ESP32-C3 slave-radio link (UART0 ⇄ `radio_link` protocol).
//!
//! When the firmware is built with the `radio` feature, networking is provided
//! by an ESP32 slave radio (C3 or C6) attached on UART0 (GP12=TX → radio RX,
//! GP13=RX ← radio TX). The radio owns WiFi entirely: station join with
//! fallback to an AP + captive portal, mDNS, a built-in provisioning UI, and
//! termination of TCP :80.
//! Inbound browser connections are relayed to the Pico over the multiplexed
//! frame protocol and served by picoserve through
//! `crate::tasks::http::serve_tunnel` — the Pico runs no IP stack at all.
//!
//! The relay is lossless: stream payload is gated by the cumulative-ack
//! flow-control window defined in `radio_link` (see its module docs), so a
//! slow consumer pauses its producer instead of overflowing or killing it.
//! Anything that would have lost bytes (CRC corruption, queue overrun, a peer
//! that stopped acking) tears down *that stream* — one failed request the
//! client can retry — and never corrupts a kept-open byte stream.
//!
//! Boot behaviour: once the HELLO handshake completes, the Pico pushes its
//! saved WiFi config to the C3 (or pulls the C3's stored networks when its own
//! config file is empty). Config saves are mirrored live via [`sync_config`].

use alloc::boxed::Box;
use alloc::string::{String, ToString as _};
use alloc::vec::Vec;

use crate::tasks::http;
use crate::tasks::usb_dbg;
use crate::types::{EthernetSignal, EthernetSignalMessage, LedBoardConfigFile};
use embassy_executor::Spawner;
use embassy_rp::uart::{BufferedUartRx, BufferedUartTx};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use embassy_sync::mutex::Mutex;
use embassy_time::{Duration, Timer, with_timeout};
use embedded_io_async::{Read as _, Write as _};
use embedded_tools::config::ConfigFileTrait as _;

use radio_link::frame_type::*;
use radio_link::{
    Decoder, Frame, HelloInfo, MAX_PAYLOAD, Network, PROTOCOL_VERSION, RadioEvent, RadioMode, RadioStatus, Reply, Rpc,
    RpcRequest, RpcResponse, SEQ_SIZE, STREAM_WINDOW_FRAMES, ScanEntry, StreamOpenInfo, StreamWindow,
};

/// Link baud rate — shared with the slave radio through [`radio_link::BAUD`]
/// so the two ends cannot disagree.
///
/// The link used to run at 115200 because the ESP32's 128-byte UART FIFO
/// overflowed under esp-radio's interrupt load at higher rates. That was never
/// a wiring limit: the slave radio now drains its FIFO with UHCI UART DMA, so
/// this is 8× faster and can go higher (see `radio_link::BAUD`).
pub const BAUD: u32 = radio_link::BAUD;

/// How many browser connections can be served through the tunnel at once.
const TUNNEL_POOL: usize = 6;

type SyncMutex<T> = Mutex<CriticalSectionRawMutex, T>;

// ---------------------------------------------------------------------------
// Chunk plumbing
// ---------------------------------------------------------------------------

/// One chunk of a relayed TCP stream. `Eof` marks that the remote side closed
/// the connection (or that we gave up on it).
#[derive(Clone)]
pub enum StreamChunk {
    Data(Vec<u8>),
    Eof,
}

/// Per-stream receive queue. Depth equals [`radio_link::STREAM_WINDOW_FRAMES`]
/// — that identity is what makes receive overflow structurally impossible
/// for a frame-count-gated sender (any frame sizes: a byte window would let
/// small frames pipeline past the slot count).
pub type StreamChannel = Channel<CriticalSectionRawMutex, StreamChunk, STREAM_WINDOW_FRAMES>;

/// Sender-side flow control for one stream (frames we push toward the radio).
/// The tunnel task gates on the window; the rx task applies incoming
/// consume-acks.
pub struct TxSlot {
    state: SyncMutex<TxState>,
}

struct TxState {
    win: StreamWindow,
    /// Next outbound stream-frame sequence number (per stream, wraps).
    next_seq: u16,
    /// Stream is gone (closed, peer dead, or CRC corruption): a blocked
    /// writer aborts instead of stalling forever.
    closed: bool,
}

impl TxSlot {
    const fn new() -> Self {
        Self {
            state: SyncMutex::new(TxState {
                win: StreamWindow::new(STREAM_WINDOW_FRAMES as u32),
                next_seq: 1,
                closed: false,
            }),
        }
    }

    /// Fresh state for a new connection on a recycled slot. A stale wake
    /// permit from a previous stream only causes one harmless re-check.
    async fn reset(&'static self) {
        let mut st = self.state.lock().await;
        st.win = StreamWindow::new(STREAM_WINDOW_FRAMES as u32);
        st.next_seq = 1;
        st.closed = false;
    }
}

/// Per-slot leaked inbound-state (references must survive `SLOTS` Vec
/// reallocation, hence separate leaks alongside queue/tx).
struct SlotEnds {
    queue: &'static StreamChannel,
    /// Next inbound data-frame sequence (validated in the rx task, the only
    /// writer). A mismatch proves frames vanished on the wire.
    rx_seq: core::sync::atomic::AtomicU16,
    /// Cumulative inbound frames handed to picoserve (via `TunnelReader`),
    /// echoed to the radio in `STREAM_ACK` frames so its upload window opens.
    rx_frames: core::sync::atomic::AtomicU32,
}

/// Pre-allocated stream slots — channels are leaked once at first use and then
/// recycled per connection, so there is no per-connection heap growth.
struct Slot {
    in_use: bool,
    id: u16,
    tx: &'static TxSlot,
    ends: &'static SlotEnds,
}

static SLOTS: SyncMutex<Vec<Slot>> = Mutex::new(Vec::new());

async fn slot_open(id: u16) -> Option<(&'static StreamChannel, &'static TxSlot)> {
    let mut slots = SLOTS.lock().await;

    if slots.len() < TUNNEL_POOL + 2 {
        while slots.len() < TUNNEL_POOL + 2 {
            let queue: &'static StreamChannel = Box::leak(Box::new(StreamChannel::new()));
            let tx: &'static TxSlot = Box::leak(Box::new(TxSlot::new()));
            let ends: &'static SlotEnds = Box::leak(Box::new(SlotEnds {
                queue,
                rx_seq: core::sync::atomic::AtomicU16::new(1),
                rx_frames: core::sync::atomic::AtomicU32::new(0),
            }));
            slots.push(Slot {
                in_use: false,
                id: 0,
                tx,
                ends,
            });
        }
    }

    // Reap anything the C3 already closed while we were busy.
    for slot in slots.iter_mut() {
        if slot.in_use && slot.id == id {
            slot.in_use = false;
        }
    }

    let slot = slots.iter_mut().find(|slot| !slot.in_use)?;
    // Discard any stale chunks from a previous connection on this slot.
    while slot.ends.queue.try_receive().is_ok() {}
    slot.in_use = true;
    slot.id = id;
    slot.ends.rx_seq.store(1, core::sync::atomic::Ordering::Relaxed);
    slot.ends.rx_frames.store(0, core::sync::atomic::Ordering::Relaxed);
    let queue = slot.ends.queue;
    let tx = slot.tx;
    drop(slots);

    tx.reset().await;
    Some((queue, tx))
}

async fn slot_queue(id: u16) -> Option<&'static StreamChannel> {
    let slots = SLOTS.lock().await;
    slots
        .iter()
        .find(|slot| slot.in_use && slot.id == id)
        .map(|slot| slot.ends.queue)
}

async fn slot_ends(id: u16) -> Option<&'static SlotEnds> {
    let slots = SLOTS.lock().await;
    slots
        .iter()
        .find(|slot| slot.in_use && slot.id == id)
        .map(|slot| slot.ends)
}

/// Mark a stream's slot free (idempotent). Also aborts any writer blocked on
/// the flow-control window, so teardown never leaves tasks hanging.
pub async fn close_stream(id: u16) {
    let mut txs: Vec<&'static TxSlot> = Vec::new();
    {
        let mut slots = SLOTS.lock().await;
        for slot in slots.iter_mut() {
            if slot.in_use && slot.id == id {
                slot.in_use = false;
                slot.id = 0;
                txs.push(slot.tx);
            }
        }
    }
    for tx in txs {
        tx.state.lock().await.closed = true;
    }
}

// ---------------------------------------------------------------------------
// Link state
// ---------------------------------------------------------------------------

// The link runs on a single-core executor; these handles are process-global
// singletons whose use is ordered by that executor, which makes storing them
// in shared statics sound even though their types are not Send/Sync.
struct Shared<T>(T);
unsafe impl<T> Send for Shared<T> {}
unsafe impl<T> Sync for Shared<T> {}
impl<T> core::ops::Deref for Shared<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.0
    }
}
impl<T> core::ops::DerefMut for Shared<T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.0
    }
}

#[derive(Copy, Clone)]
struct SharedSpawner(Spawner);
unsafe impl Send for SharedSpawner {}
unsafe impl Sync for SharedSpawner {}

static UART_TX: SyncMutex<Option<Shared<BufferedUartTx>>> = Mutex::new(None);
static SPAWNER: SyncMutex<Option<SharedSpawner>> = Mutex::new(None);
static SIGNAL: SyncMutex<Option<Shared<&'static EthernetSignal>>> = Mutex::new(None);

/// HELLO_ACK received (and cached) — the link is live.
static LINK_INFO: SyncMutex<Option<HelloInfo>> = Mutex::new(None);

/// Correlated RPC replies. One in-flight request at a time (serialized by
/// the gate); queue depth tolerates strays.
static RPC_GATE: SyncMutex<()> = Mutex::new(());
static RPC_RX: Channel<CriticalSectionRawMutex, (u8, Reply), 4> = Channel::new();
static RPC_ID: core::sync::atomic::AtomicU8 = core::sync::atomic::AtomicU8::new(1);

/// The link handle (created during init).
static LINK: SyncMutex<Option<&'static RadioLink>> = Mutex::new(None);

/// The C3-backed network link. All state is global; grab it with [`link()`].
pub struct RadioLink {
    _private: (),
}

/// Fetch the global link. `None` before [`init`] runs.
pub async fn link() -> Option<&'static RadioLink> {
    *LINK.lock().await
}

/// `true` once the C3 has answered HELLO.
pub async fn is_up() -> bool {
    LINK_INFO.lock().await.is_some()
}

/// LED control that blinks the C3's status LED over the link. Implements
/// [`LedControl`](crate::tasks::common::LedControl) so the heartbeat task
/// works unchanged on radio builds.
#[derive(Clone, Copy)]
pub struct RadioLed;

impl crate::tasks::common::LedControl for RadioLed {
    fn blink(&self, on: bool) -> impl core::future::Future<Output = ()> {
        async move {
            if let Some(link) = link().await {
                link.led(on).await;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Init
// ---------------------------------------------------------------------------

/// Bring up the slave-radio link: take the split buffered-UART halves, spawn
/// the link tasks, and return the LED control for the heartbeat task.
pub async fn init(
    spawner: Spawner,
    tx: BufferedUartTx,
    rx: BufferedUartRx,
    ethernet_signal: &'static EthernetSignal,
    config_file: LedBoardConfigFile,
) -> RadioLed {
    *UART_TX.lock().await = Some(Shared(tx));
    *SIGNAL.lock().await = Some(Shared(ethernet_signal));
    *SPAWNER.lock().await = Some(SharedSpawner(spawner));

    let link: &'static RadioLink = crate::make_static!(RadioLink, RadioLink { _private: () });
    *LINK.lock().await = Some(link);

    spawner.spawn(rx_task(rx).unwrap());
    spawner.spawn(control_task().unwrap());
    spawner.spawn(boot_sync_task(config_file).unwrap());

    usb_dbg::log(format_args!("radio: init done (baud {})", BAUD));

    RadioLed
}

// ---------------------------------------------------------------------------
// Frames
// ---------------------------------------------------------------------------

/// Write one frame to the link. Never drops: the buffered UART applies its
/// own backpressure, so callers simply wait. Returns false on UART error —
/// the stream/task unwinds recoverably and the hello handshake re-establishes
/// the link when the radio is back.
async fn send_frame(frame: &Frame) -> bool {
    let mut tx = UART_TX.lock().await;
    let Some(tx) = tx.as_mut() else { return false };
    let bytes = frame.encode();
    if (**tx).write_all(&bytes).await.is_err() {
        usb_dbg::log(format_args!("radio: uart write error"));
        return false;
    }
    // Keep the per-frame flush: it guarantees the frame is actually on the
    // wire before the (mutex-serialized) next frame is queued.
    if (**tx).flush().await.is_err() {
        usb_dbg::log(format_args!("radio: uart flush error"));
        return false;
    }
    true
}

// ---------------------------------------------------------------------------
// The link itself
// ---------------------------------------------------------------------------

impl RadioLink {
    async fn rpc(&self, rpc: Rpc) -> Option<Reply> {
        let _gate = RPC_GATE.lock().await;

        let id = RPC_ID.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        let Ok(frame) = Frame::json(RPC_REQ, &RpcRequest { id, rpc }) else {
            return None;
        };
        send_frame(&frame).await;

        loop {
            match with_timeout(Duration::from_secs(15), RPC_RX.receive()).await {
                Ok((rid, reply)) => {
                    if rid == id {
                        return Some(reply);
                    }
                    // Stale reply from an earlier request — keep waiting.
                }
                Err(_) => return None,
            }
        }
    }

    /// Ask the C3 to scan and return the results (cached on the C3).
    pub async fn scan(&self) -> Vec<ScanEntry> {
        match self.rpc(Rpc::Scan).await {
            Some(Reply::ScanResults(entries)) => entries,
            _ => Vec::new(),
        }
    }

    /// Store + connect a network now.
    pub async fn join(&self, ssid: String, pass: String) -> Option<RadioStatus> {
        match self.rpc(Rpc::Join { ssid, pass }).await {
            Some(Reply::Status(status)) => Some(status),
            _ => None,
        }
    }

    /// Push the Pico's authoritative network list to the C3.
    pub async fn set_networks(&self, networks: Vec<Network>, ap_ssid: String, mode: RadioMode) -> Option<RadioStatus> {
        match self
            .rpc(Rpc::SetNetworks {
                networks,
                ap_ssid,
                mode,
            })
            .await
        {
            Some(Reply::Status(status)) => Some(status),
            _ => None,
        }
    }

    /// Pull the C3's stored networks.
    pub async fn get_networks(&self) -> Option<(Vec<Network>, String, RadioMode)> {
        match self.rpc(Rpc::GetNetworks).await {
            Some(Reply::Networks(networks, ap_ssid, mode)) => Some((networks, ap_ssid, mode)),
            _ => None,
        }
    }

    pub async fn status(&self) -> Option<RadioStatus> {
        match self.rpc(Rpc::Status).await {
            Some(Reply::Status(status)) => Some(status),
            _ => None,
        }
    }

    /// Tell the C3 to drop back to station-only (leaves portal/AP mode).
    pub async fn use_station(&self) -> Option<RadioStatus> {
        match self.rpc(Rpc::UseStation).await {
            Some(Reply::Status(status)) => Some(status),
            _ => None,
        }
    }

    /// Fire-and-forget status LED heartbeat.
    pub async fn led(&self, on: bool) {
        if let Ok(frame) = Frame::json(
            RPC_REQ,
            &RpcRequest {
                id: 0,
                rpc: Rpc::Led(on),
            },
        ) {
            send_frame(&frame).await;
        }
    }
}

// ---------------------------------------------------------------------------
// Config mirroring
// ---------------------------------------------------------------------------

/// Push the current Pico config (networks + AP SSID + mode) to the C3.
/// Called after boot sync and after every config save in the radio build.
pub async fn sync_config(config_file: LedBoardConfigFile) {
    let Some(link) = link().await else { return };

    let data = config_file.get_data().await;
    let mode = match data.wifi_mode {
        crate::config::WifiMode::Station => RadioMode::Station,
        crate::config::WifiMode::AccessPoint => RadioMode::AccessPoint,
    };
    let networks = data
        .known_wifi_networks
        .into_iter()
        .map(|n| Network {
            ssid: n.ssid,
            pass: n.pass,
        })
        .collect();

    match link.set_networks(networks, data.ap_ssid, mode).await {
        Some(_) => defmt::info!("radio: config pushed to C3"),
        None => defmt::warn!("radio: config push timed out"),
    }
}

#[embassy_executor::task]
async fn boot_sync_task(config_file: LedBoardConfigFile) {
    // Wait (up to ~60s) for the C3 to answer HELLO.
    for _ in 0..300 {
        if LINK_INFO.lock().await.is_some() {
            break;
        }
        Timer::after_millis(200).await;
    }

    if LINK_INFO.lock().await.is_none() {
        defmt::warn!("radio: C3 never answered HELLO — continuing anyway");
        usb_dbg::log(format_args!("radio: no HELLO_ACK in 60s"));
        return;
    }
    usb_dbg::log(format_args!("radio: link up, syncing config"));

    // Give the C3 a beat to settle its radio.
    Timer::after_millis(500).await;

    // The C3 owns the WiFi config: provisioning (portal / phone) writes to the
    // C3's journal directly, and set_networks *replaces* the whole list — so
    // pushing Pico's copy on every boot silently erased portal-added networks
    // (that was the boot-sync overwrite bug). The Pico instead ADOPTS the C3's
    // list for display, and only ever pushes as a one-time migration when the
    // C3 is empty (e.g. first boot of a freshly flashed C3).
    let data = config_file.get_data().await;

    if let Some(link) = link().await {
        match link.get_networks().await {
            Some((networks, ap_ssid, mode)) if !networks.is_empty() => {
                let mut fresh = crate::config::LedBoardConfig::default();
                fresh.ap_ssid = ap_ssid;
                fresh.wifi_mode = match mode {
                    RadioMode::AccessPoint => crate::config::WifiMode::AccessPoint,
                    _ => crate::config::WifiMode::Station,
                };
                fresh.known_wifi_networks = networks
                    .into_iter()
                    .map(|n| crate::config::KnownWifiNetwork {
                        ssid: n.ssid,
                        pass: n.pass,
                    })
                    .collect();
                config_file.set_data(fresh).await;
                let _ = config_file.save().await;
                defmt::info!("radio: adopted networks from C3 (C3 owns WiFi config)");
                usb_dbg::log(format_args!("radio: adopted networks from C3"));
            }
            _ => {
                // C3 has nothing stored — migrate the Pico's legacy config once.
                if !data.known_wifi_networks.is_empty()
                    || matches!(data.wifi_mode, crate::config::WifiMode::AccessPoint)
                {
                    sync_config(config_file).await;
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// RX / dispatch
// ---------------------------------------------------------------------------

#[embassy_executor::task]
async fn rx_task(mut rx: BufferedUartRx) {
    let mut decoder = Decoder::new();
    let mut buf = [0u8; 512];

    loop {
        match rx.read(&mut buf).await {
            Ok(0) => {}
            Ok(n) => {
                decoder.push(&buf[..n]);
                loop {
                    match decoder.next_frame() {
                        Ok(Some(frame)) => handle_frame(frame).await,
                        Ok(None) => break,
                        Err(corrupt) => {
                            // Bytes belonging to this stream were lost. A
                            // kept-open stream would silently corrupt TCP;
                            // a torn-down one just costs the client a retry.
                            usb_dbg::log(format_args!(
                                "radio: corrupt frame (type {} stream {}) -> closing stream",
                                corrupt.ftype, corrupt.stream
                            ));
                            if corrupt.stream != 0 {
                                abort_corrupt_stream(corrupt.stream).await;
                            }
                        }
                    }
                }
            }
            Err(_) => {
                usb_dbg::log(format_args!("radio: uart read error"));
                decoder.reset();
                Timer::after_millis(5).await;
            }
        }
    }
}

/// Tear down a stream whose bytes were corrupted or lost: wake the consumer
/// with an Eof, tell the radio, free the slot. All state for the stream is
/// destroyed, so nothing can leak across into the next connection.
async fn abort_corrupt_stream(id: u16) {
    if let Some(queue) = slot_queue(id).await {
        force_eof(queue).await;
    }
    if let Ok(close) = Frame::new(STREAM_CLOSE, id, Vec::new()) {
        let _ = send_frame(&close).await;
    }
    close_stream(id).await;
}

/// Push the Eof marker, retrying briefly if the queue is momentarily full.
/// Only used on teardown paths, where the consumer is draining.
async fn force_eof(queue: &'static StreamChannel) {
    for _ in 0..5 {
        if queue.try_send(StreamChunk::Eof).is_ok() {
            return;
        }
        Timer::after_millis(20).await;
    }
}

async fn handle_frame(frame: Frame) {
    match frame.ftype {
        HELLO_ACK => match frame.payload_json::<HelloInfo>() {
            Ok(info) => {
                if LINK_INFO.lock().await.is_none() {
                    defmt::info!("radio: C3 hello ack (fw {})", info.firmware.as_str());
                    usb_dbg::log(format_args!("radio: HELLO_ACK (fw {})", info.firmware.as_str()));
                }
                if info.protocol != PROTOCOL_VERSION {
                    usb_dbg::log(format_args!(
                        "radio: PROTOCOL MISMATCH radio={} pico={}",
                        info.protocol, PROTOCOL_VERSION
                    ));
                }
                *LINK_INFO.lock().await = Some(info);
            }
            Err(err) => defmt::warn!("radio: bad HELLO_ACK {}", defmt::Debug2Format(&err)),
        },
        RPC_RESP => match frame.payload_json::<RpcResponse>() {
            Ok(resp) => {
                let _ = RPC_RX.try_send((resp.id, resp.reply));
            }
            Err(err) => defmt::warn!("radio: bad RPC_RESP {}", defmt::Debug2Format(&err)),
        },
        EVENT => match frame.payload_json::<RadioEvent>() {
            Ok(event) => handle_event(event).await,
            Err(err) => defmt::warn!("radio: bad EVENT {}", defmt::Debug2Format(&err)),
        },
        STREAM_OPEN => {
            let id = frame.stream;
            if id == 0 {
                return; // stream 0 is reserved for control traffic
            }
            let _port: u16 = frame.payload_json::<StreamOpenInfo>().map(|i| i.port).unwrap_or(80);

            let Some((queue, tx)) = slot_open(id).await else {
                defmt::warn!("radio: no free slot for stream {}", id);
                if let Ok(close) = Frame::new(STREAM_CLOSE, id, Vec::new()) {
                    let _ = send_frame(&close).await;
                }
                return;
            };

            if let Some(spawner) = *SPAWNER.lock().await {
                if let Ok(token) = serve_conn_task(id, queue, tx) {
                    spawner.0.spawn(token);
                } else {
                    defmt::warn!("radio: tunnel pool exhausted for stream {}", id);
                    if let Ok(close) = Frame::new(STREAM_CLOSE, id, Vec::new()) {
                        let _ = send_frame(&close).await;
                    }
                    close_stream(id).await;
                }
            }
        }
        STREAM_DATA => {
            let Some(ends) = slot_ends(frame.stream).await else {
                return;
            };
            let Some((seq, body)) = frame.data_parts() else {
                usb_dbg::log(format_args!("radio: stream {} data frame malformed", frame.stream));
                abort_corrupt_stream(frame.stream).await;
                return;
            };

            // Sequence check: a gap is proof bytes vanished on the wire —
            // noise can eat a whole frame without any CRC ever failing,
            // because the decoder skips an orphaned tail as garbage.
            use core::sync::atomic::Ordering::Relaxed;
            let want = ends.rx_seq.load(Relaxed);
            if seq != want {
                usb_dbg::log(format_args!(
                    "radio: stream {} seq gap (got {} want {}) — bytes lost",
                    frame.stream, seq, want
                ));
                abort_corrupt_stream(frame.stream).await;
                return;
            }
            ends.rx_seq.store(want.wrapping_add(1), Relaxed);

            // The radio is gated to `radio_link::STREAM_WINDOW_FRAMES`
            // un-acked frames per stream, and this queue has exactly that
            // many slots, so there is always room. Failure means the
            // flow-control invariant was violated (firm skew or a bug) — tear
            // the stream down recoverably rather than wedge the link.
            if ends.queue.try_send(StreamChunk::Data(body.to_vec())).is_err() {
                defmt::error!("radio: stream {} overran flow-control window", frame.stream);
                usb_dbg::log(format_args!("radio: stream {} window overrun", frame.stream));
                abort_corrupt_stream(frame.stream).await;
            }
        }
        STREAM_ACK => {
            if let Some(consumed) = frame.consumed() {
                apply_ack(frame.stream, consumed).await;
            }
        }
        STREAM_CLOSE => {
            if let Some(queue) = slot_queue(frame.stream).await {
                force_eof(queue).await;
            }
        }
        _ => {}
    }
}

/// Apply a cumulative consumed-frame ack from the radio to the stream's send
/// window.
async fn apply_ack(id: u16, consumed: u32) {
    let tx = {
        let slots = SLOTS.lock().await;
        slots.iter().find(|s| s.in_use && s.id == id).map(|s| s.tx)
    };
    let Some(tx) = tx else { return };

    let applied = tx.state.lock().await.win.note_ack(consumed);
    if !applied {
        defmt::warn!("radio: bogus ack frames={} for stream {}", consumed, id);
    }
}

async fn handle_event(event: RadioEvent) {
    let signal = { SIGNAL.lock().await.as_ref().map(|s| **s) };
    let Some(signal) = signal else { return };
    let sender = signal.sender();

    match event {
        RadioEvent::Joined { ssid, ip } => {
            defmt::info!("radio: joined {} ({})", ssid.as_str(), defmt::Debug2Format(&ip));
            sender.send(EthernetSignalMessage::Connected(ip.into()));
        }
        RadioEvent::ApStarted { ssid, ip } => {
            defmt::info!("radio: AP up {} ({})", ssid.as_str(), defmt::Debug2Format(&ip));
            sender.send(EthernetSignalMessage::Connected(ip.into()));
        }
        RadioEvent::ApStopped => {
            defmt::info!("radio: AP stopped");
            sender.send(EthernetSignalMessage::Disconnected);
        }
        RadioEvent::Interrupted => defmt::info!("radio: connection interrupted"),
        RadioEvent::PortalJoined => defmt::info!("radio: portal client joined"),
        RadioEvent::Status(status) => {
            if let Some(message) = station_message(&status) {
                sender.send(message);
            }
        }
    }
}

fn station_message(status: &RadioStatus) -> Option<EthernetSignalMessage> {
    if let Some(ip) = status.ip {
        Some(EthernetSignalMessage::Connected(core::net::Ipv4Addr::from(ip)))
    } else if let Some(ip) = status.ap_ip {
        Some(EthernetSignalMessage::Connected(core::net::Ipv4Addr::from(ip)))
    } else {
        Some(EthernetSignalMessage::Disconnected)
    }
}

// ---------------------------------------------------------------------------
// Tunnel serving
// ---------------------------------------------------------------------------

#[embassy_executor::task(pool_size = TUNNEL_POOL)]
async fn serve_conn_task(id: u16, queue: &'static StreamChannel, tx: &'static TxSlot) {
    defmt::info!("radio: serving tunneled connection {}", id);

    let socket = http::TunnelSocket::new(id, queue, tx);
    match http::serve_tunnel(socket).await {
        Ok(info) => defmt::debug!(
            "radio: connection {} closed ({} requests)",
            id,
            info.handled_requests_count
        ),
        Err(err) => defmt::debug!("radio: connection {} error {}", id, defmt::Debug2Format(&err)),
    }

    // Tell the C3 the stream is done, then free the slot.
    if let Ok(close) = Frame::new(STREAM_CLOSE, id, Vec::new()) {
        let _ = send_frame(&close).await;
    }
    close_stream(id).await;
}

/// Report that `len` stream bytes were handed up to picoserve and echo the
/// cumulative count to the radio, opening its window for request bodies and
/// OTA uploads. `len` is body length; the `SEQ_SIZE` header of the source
/// frame is counted so both sides tally identical byte totals.
/// Report that one inbound frame was fully handed to picoserve and echo the
/// cumulative frame count to the radio so its upload window opens again.
/// Frame counting (not bytes) keeps the radio's sender gate and this queue's
/// slot count exactly aligned for any frame sizes.
pub async fn ack_stream(id: u16) {
    let ends = slot_ends(id).await;
    let Some(ends) = ends else { return };
    let total = ends
        .rx_frames
        .fetch_add(1, core::sync::atomic::Ordering::Relaxed)
        .wrapping_add(1);
    let _ = send_frame(&Frame::ack(id, total)).await;
}

/// Send bytes on an open stream, gated by the flow-control window. Returns
/// false when the stream has gone (closed, link error, or a peer that
/// stopped acking) — the caller must abandon the connection rather than
/// assume the bytes landed.
pub async fn send_stream_data(id: u16, tx: &'static TxSlot, data: &[u8]) -> bool {
    for chunk in data.chunks(MAX_PAYLOAD - SEQ_SIZE) {
        // Wait for window space. A blocked sender re-checks every 5 ms —
        // only ever while backlogged, and at that cadence the window drains
        // as fast as the wire allows; the 30 s cap guarantees we abort when
        // the peer is dead and never acking.
        let mut waited: u32 = 0;
        let frame = loop {
            let gated = {
                let mut st = tx.state.lock().await;
                if st.closed {
                    return false;
                }
                if st.win.can_send_frame() {
                    st.win.note_sent_frame();
                    let seq = st.next_seq;
                    st.next_seq = st.next_seq.wrapping_add(1);
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
                        defmt::warn!("radio: stream {} window closed 30s — peer stopped acking", id);
                        usb_dbg::log(format_args!("radio: stream {} window timeout, closing", id));
                        close_stream(id).await;
                        return false;
                    }
                }
            }
        };

        if !send_frame(&frame).await {
            close_stream(id).await;
            return false;
        }
    }
    true
}

/// Signal stream write close to the C3.
pub async fn send_stream_close(id: u16) {
    if let Ok(close) = Frame::new(STREAM_CLOSE, id, Vec::new()) {
        let _ = send_frame(&close).await;
    }
}

// ---------------------------------------------------------------------------
// Control task (hello + keepalive)
// ---------------------------------------------------------------------------

#[embassy_executor::task]
async fn control_task() {
    let mut ticks: u32 = 0;

    loop {
        Timer::after_millis(1000).await;
        ticks = ticks.wrapping_add(1);

        let acked = LINK_INFO.lock().await.is_some();

        if !acked {
            if ticks <= 3 || ticks % 10 == 0 {
                usb_dbg::log(format_args!("radio: hello #{}", ticks));
            }
            let hello = HelloInfo {
                board: "rp2350".to_string(),
                firmware: env!("CARGO_PKG_VERSION").to_string(),
                protocol: PROTOCOL_VERSION,
            };
            if let Ok(frame) = Frame::json(HELLO, &hello) {
                send_frame(&frame).await;
            }
        } else if ticks % 5 == 0 {
            if let Ok(frame) = Frame::new(KEEPALIVE, 0, Vec::new()) {
                send_frame(&frame).await;
            }
        }
    }
}
