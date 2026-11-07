//! ESP32-C3 slave-radio link (UART0 ⇄ `radio_link` protocol).
//!
//! When the firmware is built with the `radio` feature, networking is provided
//! by an ESP32-C3 attached on UART0 (GP12=TX → C3 RX, GP13=RX ← C3 TX). The C3
//! owns WiFi entirely: station join with fallback to an open AP + captive
//! portal, mDNS, a built-in provisioning UI, and termination of TCP :80.
//! Inbound browser connections are relayed to the Pico over the multiplexed
//! frame protocol and served by picoserve through
//! `crate::tasks::http::serve_tunnel` — the Pico runs no IP stack at all.
//!
//! Boot behaviour: once the HELLO handshake completes, the Pico pushes its
//! saved WiFi config to the C3 (or pulls the C3's stored networks when its own
//! config file is empty). Config saves are mirrored live via [`sync_config`].

use alloc::boxed::Box;
use alloc::string::{String, ToString as _};
use alloc::vec::Vec;

use crate::tasks::http;
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
    RpcRequest, RpcResponse, ScanEntry, StreamOpenInfo,
};

/// Link baud rate (must match the C3 firmware).
pub const BAUD: u32 = 921_600;

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

/// Per-stream receive queue.
pub type StreamChannel = Channel<CriticalSectionRawMutex, StreamChunk, 8>;

/// Pre-allocated stream slots — channels are leaked once at first use and then
/// recycled per connection, so there is no per-connection heap growth.
struct Slot {
    in_use: bool,
    id: u16,
    queue: &'static StreamChannel,
}

static SLOTS: SyncMutex<Vec<Slot>> = Mutex::new(Vec::new());

async fn slot_open(id: u16) -> Option<&'static StreamChannel> {
    let mut slots = SLOTS.lock().await;

    if slots.len() < TUNNEL_POOL + 2 {
        while slots.len() < TUNNEL_POOL + 2 {
            let queue: &'static StreamChannel = Box::leak(Box::new(StreamChannel::new()));
            slots.push(Slot {
                in_use: false,
                id: 0,
                queue,
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
    while slot.queue.try_receive().is_ok() {}
    slot.in_use = true;
    slot.id = id;
    Some(slot.queue)
}

async fn slot_queue(id: u16) -> Option<&'static StreamChannel> {
    let slots = SLOTS.lock().await;
    slots
        .iter()
        .find(|slot| slot.in_use && slot.id == id)
        .map(|slot| slot.queue)
}

/// Mark a stream's slot free (idempotent).
pub async fn close_stream(id: u16) {
    let mut slots = SLOTS.lock().await;
    for slot in slots.iter_mut() {
        if slot.in_use && slot.id == id {
            slot.in_use = false;
            slot.id = 0;
        }
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

    RadioLed
}

// ---------------------------------------------------------------------------
// Frames
// ---------------------------------------------------------------------------

async fn send_frame(frame: &Frame) {
    let mut tx = UART_TX.lock().await;
    let Some(tx) = tx.as_mut() else { return };
    let bytes = frame.encode();
    let _ = (**tx).write_all(&bytes).await;
    let _ = (**tx).flush().await;
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
        return;
    }

    // Give the C3 a beat to settle its radio.
    Timer::after_millis(500).await;

    let data = config_file.get_data().await;

    if !data.known_wifi_networks.is_empty() || matches!(data.wifi_mode, crate::config::WifiMode::AccessPoint) {
        // Pico config wins — push it.
        sync_config(config_file).await;
    } else if let Some(link) = link().await {
        // Fresh Pico — adopt whatever the C3 already has stored.
        if let Some((networks, ap_ssid, mode)) = link.get_networks().await {
            if !networks.is_empty() {
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
                defmt::info!("radio: adopted networks from C3");
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
                while let Some(frame) = decoder.next_frame() {
                    handle_frame(frame).await;
                }
            }
            Err(_) => {
                decoder.reset();
                Timer::after_millis(5).await;
            }
        }
    }
}

async fn handle_frame(frame: Frame) {
    match frame.ftype {
        HELLO_ACK => match frame.payload_json::<HelloInfo>() {
            Ok(info) => {
                if LINK_INFO.lock().await.is_none() {
                    defmt::info!("radio: C3 hello ack (fw {})", info.firmware.as_str());
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

            let Some(queue) = slot_open(id).await else {
                defmt::warn!("radio: no free slot for stream {}", id);
                if let Ok(close) = Frame::new(STREAM_CLOSE, id, Vec::new()) {
                    send_frame(&close).await;
                }
                return;
            };

            if let Some(spawner) = *SPAWNER.lock().await {
                if let Ok(token) = serve_conn_task(id, queue) {
                    spawner.0.spawn(token);
                } else {
                    defmt::warn!("radio: tunnel pool exhausted for stream {}", id);
                    if let Ok(close) = Frame::new(STREAM_CLOSE, id, Vec::new()) {
                        send_frame(&close).await;
                    }
                    close_stream(id).await;
                }
            }
        }
        STREAM_DATA => {
            if frame.payload.is_empty() {
                return;
            }
            let Some(queue) = slot_queue(frame.stream).await else {
                return;
            };

            // Backpressure: if the consumer stalls past the grace period,
            // drop the stream rather than wedging the whole link.
            let chunk = StreamChunk::Data(frame.payload);
            if with_timeout(Duration::from_secs(5), queue.send(chunk)).await.is_err() {
                defmt::warn!("radio: stream {} stalled, closing", frame.stream);
                if let Some(queue) = slot_queue(frame.stream).await {
                    let _ = queue.try_send(StreamChunk::Eof);
                }
            }
        }
        STREAM_CLOSE => {
            if let Some(queue) = slot_queue(frame.stream).await {
                let _ = queue.try_send(StreamChunk::Eof);
            }
        }
        _ => {}
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
async fn serve_conn_task(id: u16, queue: &'static StreamChannel) {
    defmt::info!("radio: serving tunneled connection {}", id);

    let socket = http::TunnelSocket::new(id, queue);
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
        send_frame(&close).await;
    }
    close_stream(id).await;
}

/// Send bytes on an open stream (chunked to fit the frame payload).
pub async fn send_stream_data(id: u16, data: &[u8]) {
    for chunk in data.chunks(MAX_PAYLOAD) {
        if let Ok(frame) = Frame::new(STREAM_DATA, id, chunk.to_vec()) {
            send_frame(&frame).await;
        }
    }
}

/// Signal stream write close to the C3.
pub async fn send_stream_close(id: u16) {
    if let Ok(close) = Frame::new(STREAM_CLOSE, id, Vec::new()) {
        send_frame(&close).await;
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
