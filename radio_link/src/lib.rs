//! Wire protocol for the RP2350 ⇄ ESP32 slave-radio UART link (C3 or C6).
//!
//! The link multiplexes three things over one serial connection:
//!
//! 1. **RPC** request/response pairs (JSON) used to drive the radio: scan, join,
//!    status, credential sync, and the LED heartbeat.
//! 2. **Stream** frames — each frame carries a chunk of one byte stream. Streams
//!    are opened by the slave radio whenever a TCP client (LAN or the AP portal
//!    network) connects to it; the stream is piped to the Pico where it is
//!    presented to `picoserve` as a socket. This lets the Pico serve its whole
//!    web app (HTTP + WebSockets + OTA uploads) with no IP stack of its own.
//! 3. **Events** — status updates pushed from the C3 (connected / AP started / etc).
//!
//! ## Flow control
//!
//! Stream payload is windowed, TCP-style: each side accumulates a per-stream
//! count of *frames* handed to the consumer and echoes it back in a
//! `STREAM_ACK` frame carrying the *cumulative* count. A sender may keep at
//! most [`STREAM_WINDOW_FRAMES`] frames un-acked per stream — exactly the
//! capacity of the receiver's per-stream queue. Since every data frame's body
//! is at most `MAX_PAYLOAD - SEQ_SIZE`, gating on frame count (rather than
//! bytes) is what makes the invariant hold for *any* frame sizes: a byte
//! window would allow more than `queue_slots` small frames in flight and let
//! the queue's `try_send` fail. Buffer overflow is structurally unreachable —
//! a slow consumer pauses its producer instead of corrupting it.
//!
//! Acknowledgements are cumulative, never incremental: a lost ACK is healed
//! by the next one, so there is no credit accumulator that can drift or leak.
//! Stream teardown resets all window state on both ends, so no accounting
//! error can outlive the stream it belongs to.
//!
//! The transport itself is lossless by construction: senders block on bounded
//! queues (which always drain because the UART always drains). A frame that
//! fails its CRC is reported to the caller with its stream id so that stream
//! is *closed* — the one request fails and the client retries, instead of a
//! silently corrupted byte stream being handed to TCP.
//!
//! ## Frame layout
//!
//! ```text
//! [magic: u16 LE = 0xBEEF][type: u8][stream: u16 LE][len: u16 LE][crc: u16 LE][payload: len bytes]
//! ```
//!
//! The CRC16 (CCITT-FALSE, poly 0x1021, init 0xFFFF) covers `type, stream, len`
//! (LE bytes) followed by the payload. The decoder resynchronises on the next
//! magic value after any corruption.
#![cfg_attr(not(test), no_std)]

extern crate alloc;

use alloc::vec::Vec;
use core::fmt;
use serde::{Deserialize, Serialize};

pub const MAGIC: u16 = 0xBEEF;
pub const MAX_PAYLOAD: usize = 1024;
pub const HEADER_SIZE: usize = 9;
pub const MAX_FRAME: usize = HEADER_SIZE + MAX_PAYLOAD;

/// Protocol revision, carried in the HELLO handshake. Version 2 added
/// `STREAM_ACK` cumulative consume-acks; version 3 changed their payload from
/// cumulative *bytes* to cumulative *frames* (see the Flow control section).
pub const PROTOCOL_VERSION: u8 = 3;

/// Maximum un-acknowledged `STREAM_DATA` frames per stream, per direction.
///
/// Both link implementations size their per-stream queues to exactly this many
/// slots, and senders gate on frame count rather than bytes: a byte window
/// (the old `8 × MAX_PAYLOAD`) looks equivalent for full-size frames but lets
/// small frames — HTTP header writes of 15 B or 123 B — pipeline more frames
/// than there are slots, so the receiver's `try_send` fails. Frame gating is
/// the invariant that actually holds for every frame size; queue bytes are
/// then bounded by `slots × (MAX_PAYLOAD - SEQ_SIZE)` automatically.
///
/// Sized for the C3/C6 heap: every queued frame is a heap Vec there, and the
/// worst case (8 slots × 8 streams × 1 KB) plus the frame-TX backlog once
/// OOM-panicked the radio when a WiFi disconnect coincided with AP fallback.
/// 4 frames per stream is ~90 ms of buffering at 460800 — far more than the
/// ack round-trip, so throughput stays line-rate bound.
pub const STREAM_WINDOW_FRAMES: usize = 4;

/// Bytes of a `STREAM_DATA` payload occupied by the leading frame sequence
/// number (u16 LE). The remaining `MAX_PAYLOAD - SEQ_SIZE` bytes are body.
///
/// The sequence makes silent byte-loss detectable: wire noise that eats
/// bytes *between* frames can drop whole frames without ever producing a
/// CRC failure (the decoder just skips the orphaned tail as garbage). A
/// gap in the sequence is proof of loss, and the stream is closed — one
/// retriable request instead of a silently truncated response.
pub const SEQ_SIZE: usize = 2;

/// UART baud rate for the Pico ⇄ slave-radio link.
///
/// Both firmwares read this constant, so the two ends cannot drift apart:
/// change it here and rebuild **both** sides.
///
/// 460800 is the rate that passes the stress suite with zero CRC/seq trips
/// (15/15 byte-exact loads, 2026-11 bench). Earlier 115200-era failures were
/// driver limitations (ESP32 FIFO under WiFi IRQ load — fixed by UHCI DMA,
/// and a receiver-window bug — fixed by frame-count gating v3), not wiring.
/// **921600 tested 2026-11: ~1 mid-frame CRC trip per 10–15 page loads on
/// this bench's jumpers** (Pico→C6 STREAM_DATA; detected, stream closed, one
/// retriable request — but above the zero-trip bar). Re-test 921600 after any
/// wiring rework (shorter loops, second GND closer to TX, or series R at the
/// driver pin); 1.5–2 Mbaud is otherwise plausible with the DMA setup.
pub const BAUD: u32 = 460_800;

/// Stream id reserved for RPC/event traffic (carries JSON payloads).
pub const STREAM_RPC: u16 = 0;

/// Frame type octets.
pub mod frame_type {
    pub const HELLO: u8 = 1;
    pub const HELLO_ACK: u8 = 2;
    pub const RPC_REQ: u8 = 3;
    pub const RPC_RESP: u8 = 4;
    pub const EVENT: u8 = 5;
    pub const STREAM_OPEN: u8 = 8;
    pub const STREAM_DATA: u8 = 9;
    pub const STREAM_CLOSE: u8 = 10;
    pub const KEEPALIVE: u8 = 11;
    /// Cumulative consume-ack for a stream (payload: u32 LE count of frames
    /// consumed). Protocol v3: frames, not bytes.
    pub const STREAM_ACK: u8 = 12;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameError {
    PayloadTooLarge,
    Serialize,
}

/// A decoded frame with an owned payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub ftype: u8,
    pub stream: u16,
    pub payload: Vec<u8>,
}

impl Frame {
    pub fn new(ftype: u8, stream: u16, payload: Vec<u8>) -> Result<Self, FrameError> {
        if payload.len() > MAX_PAYLOAD {
            return Err(FrameError::PayloadTooLarge);
        }
        Ok(Self { ftype, stream, payload })
    }

    /// Convenience constructor for control frames carrying a JSON payload.
    pub fn json<T: Serialize>(ftype: u8, value: &T) -> Result<Self, FrameError> {
        let payload = serde_json::to_vec(value).map_err(|_| FrameError::Serialize)?;
        Self::new(ftype, STREAM_RPC, payload)
    }

    pub fn empty(ftype: u8) -> Self {
        Self {
            ftype,
            stream: STREAM_RPC,
            payload: Vec::new(),
        }
    }

    fn checksum(&self) -> u16 {
        let len = self.payload.len() as u16;
        let crc = crc16(&[
            self.ftype,
            (self.stream & 0xFF) as u8,
            (self.stream >> 8) as u8,
            (len & 0xFF) as u8,
            (len >> 8) as u8,
        ]);
        crc16_update(crc, &self.payload)
    }

    /// Serialize the frame into `out`, returning the number of bytes written.
    pub fn encode_into(&self, out: &mut [u8]) -> Result<usize, FrameError> {
        if out.len() < HEADER_SIZE + self.payload.len() {
            return Err(FrameError::PayloadTooLarge);
        }

        let len = self.payload.len() as u16;
        let crc = self.checksum();

        out[0..2].copy_from_slice(&MAGIC.to_le_bytes());
        out[2] = self.ftype;
        out[3..5].copy_from_slice(&self.stream.to_le_bytes());
        out[5..7].copy_from_slice(&len.to_le_bytes());
        out[7..9].copy_from_slice(&crc.to_le_bytes());
        out[HEADER_SIZE..HEADER_SIZE + self.payload.len()].copy_from_slice(&self.payload);

        Ok(HEADER_SIZE + self.payload.len())
    }

    /// Allocate and encode.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = vec![0u8; HEADER_SIZE + self.payload.len()];
        // Safe: buffer is exactly sized
        let n = self.encode_into(&mut out).unwrap();
        out.truncate(n);
        out
    }

    /// Decode the JSON payload of a control frame.
    pub fn payload_json<T: serde::de::DeserializeOwned>(&self) -> Result<T, serde_json::Error> {
        serde_json::from_slice(&self.payload)
    }

    /// Build a cumulative consume-ack for `stream`.
    pub fn ack(stream: u16, consumed: u32) -> Self {
        Self {
            ftype: frame_type::STREAM_ACK,
            stream,
            payload: consumed.to_le_bytes().to_vec(),
        }
    }

    /// Read the cumulative consumed-*frame* count from a [`frame_type::STREAM_ACK`].
    pub fn consumed(&self) -> Option<u32> {
        if self.payload.len() != 4 {
            return None;
        }
        Some(u32::from_le_bytes([
            self.payload[0],
            self.payload[1],
            self.payload[2],
            self.payload[3],
        ]))
    }

    /// Build a `STREAM_DATA` frame carrying `body` under sequence number
    /// `seq` (u16 LE, starts at 1 and increments per stream per direction).
    pub fn data(stream: u16, seq: u16, body: &[u8]) -> Result<Self, FrameError> {
        if body.len() > MAX_PAYLOAD - SEQ_SIZE {
            return Err(FrameError::PayloadTooLarge);
        }
        let mut payload = Vec::with_capacity(SEQ_SIZE + body.len());
        payload.extend_from_slice(&seq.to_le_bytes());
        payload.extend_from_slice(body);
        Ok(Self {
            ftype: frame_type::STREAM_DATA,
            stream,
            payload,
        })
    }

    /// Split a `STREAM_DATA` payload into `(seq, body)`.
    pub fn data_parts(&self) -> Option<(u16, &[u8])> {
        split_data(&self.payload)
    }
}

/// Split a raw `STREAM_DATA` payload into `(seq, body)`.
pub fn split_data(payload: &[u8]) -> Option<(u16, &[u8])> {
    if payload.len() < SEQ_SIZE {
        return None;
    }
    let seq = u16::from_le_bytes([payload[0], payload[1]]);
    Some((seq, &payload[SEQ_SIZE..]))
}

// ---------------------------------------------------------------------------
// Per-stream flow control (shared by both link implementations)
// ---------------------------------------------------------------------------

/// A frame whose header decoded plausibly but whose CRC did not match. It has
/// already been dropped by the [`Decoder`]. The reported `stream` identifies
/// the affected byte stream so the caller can tear it down: the stream is
/// missing bytes and must not be trusted further.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CorruptFrame {
    pub ftype: u8,
    pub stream: u16,
}

/// Sender-side flow-control window for one stream, counted in *frames*
/// (see the module Flow control section for why not bytes).
///
/// Pure arithmetic — no async, no allocation — so it is unit-tested on the
/// host. The invariant the link relies on: while `outstanding() <= slots`,
/// a receiver queue with `slots` slots can never overflow, for any frame
/// sizes.
#[derive(Debug, Clone, Copy)]
pub struct StreamWindow {
    slots: u32,
    sent: u32,
    acked: u32,
}

impl StreamWindow {
    pub const fn new(slots: u32) -> Self {
        Self {
            slots,
            sent: 0,
            acked: 0,
        }
    }

    /// Construct at an absolute offset (stream recycling / wrap tests).
    pub fn at(slots: u32, sent: u32, acked: u32) -> Self {
        Self { slots, sent, acked }
    }

    /// Frames sent but not yet acknowledged.
    pub fn outstanding(&self) -> u32 {
        self.sent.wrapping_sub(self.acked)
    }

    pub fn available(&self) -> u32 {
        self.slots - self.outstanding()
    }

    pub fn can_send_frame(&self) -> bool {
        self.outstanding() < self.slots
    }

    pub fn note_sent_frame(&mut self) {
        self.sent = self.sent.wrapping_add(1);
    }

    /// Apply a cumulative frame-consume-ack. Stale acks are ignored. A forward
    /// jump larger than the window would mean the receiver consumed more
    /// frames than the sender ever sent — only possible with mismatched
    /// state — so it is refused. Returns true if applied.
    pub fn note_ack(&mut self, consumed_frames: u32) -> bool {
        let delta = consumed_frames.wrapping_sub(self.acked);
        if delta <= self.slots {
            self.acked = consumed_frames;
            true
        } else {
            false
        }
    }
}

/// CRC16-CCITT-FALSE.
pub fn crc16(data: &[u8]) -> u16 {
    crc16_update(0xFFFF, data)
}

pub fn crc16_update(mut crc: u16, data: &[u8]) -> u16 {
    for &byte in data {
        crc ^= (byte as u16) << 8;
        for _ in 0..8 {
            if crc & 0x8000 != 0 {
                crc = (crc << 1) ^ 0x1021;
            } else {
                crc <<= 1;
            }
        }
    }
    crc
}

// ---------------------------------------------------------------------------
// RPC payloads (JSON-serialised, shared by both sides)
// ---------------------------------------------------------------------------

use alloc::{string::String, vec};

/// Wi-Fi mode semantics shared with the Pico's `config.json` (`WifiMode`) plus the
/// combined provisioning mode the C3 uses while the portal is up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RadioMode {
    /// Station only — normal operation.
    Station,
    /// Access point only — fallback / provisioning network.
    AccessPoint,
    /// Access point + station simultaneously (used while the portal re-provisions).
    ApStation,
}

/// One saved Wi-Fi network — mirrors the Pico's `KnownWifiNetwork`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Network {
    pub ssid: String,
    pub pass: String,
}

/// One scan result — field names match the Pico web UI (`WifiResultSchema`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanEntry {
    pub ssid: String,
    pub signal_strength: i8,
    pub password_required: bool,
}

/// Radio status snapshot, also pushed as an event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RadioStatus {
    pub mode: RadioMode,
    /// SSID of the station connection (when up).
    pub ssid: Option<String>,
    /// Station IPv4 address (when up), octet order.
    pub ip: Option<[u8; 4]>,
    pub rssi: Option<i8>,
    /// SSID of the access point (when up).
    pub ap_ssid: Option<String>,
    /// Access point IPv4 address (when up).
    pub ap_ip: Option<[u8; 4]>,
    pub firmware: String,
    pub mac: [u8; 6],
}

/// Requests sent by the Pico (C3 always answers with [`Reply`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Rpc {
    /// Ask the C3 to scan; results are cached on the C3 and returned.
    Scan,
    /// Remember this network (persist on C3) and connect to it now.
    Join { ssid: String, pass: String },
    /// Drop a saved network.
    Forget { ssid: String },
    /// The Pico pushes its authoritative stored networks + AP SSID + mode after boot
    /// or after its config file was edited.
    SetNetworks {
        networks: Vec<Network>,
        ap_ssid: String,
        mode: RadioMode,
    },
    /// The Pico pulls the C3's stored networks (used when the Pico's config is empty).
    GetNetworks,
    /// Current status snapshot.
    Status,
    /// Fire-and-forget LED heartbeat (blink the C3 status LED).
    Led(bool),
    /// Ask the C3 to switch back to station-only mode (used by the portal "finish").
    UseStation,
}

/// Replies to [`Rpc`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Reply {
    Ok,
    Err(String),
    ScanResults(Vec<ScanEntry>),
    Networks(Vec<Network>, String, RadioMode),
    Status(RadioStatus),
}

/// Unsolicited events pushed by the C3.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RadioEvent {
    Status(RadioStatus),
    Joined { ssid: String, ip: [u8; 4] },
    Interrupted,
    ApStarted { ssid: String, ip: [u8; 4] },
    ApStopped,
    PortalJoined,
}

/// Envelope for a JSON RPC request (Pico → C3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RpcRequest {
    pub id: u8,
    #[serde(flatten)]
    pub rpc: Rpc,
}

/// Envelope for a JSON RPC reply (C3 → Pico).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RpcResponse {
    pub id: u8,
    pub reply: Reply,
}

/// Info carried by the HELLO / HELLO_ACK handshake.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HelloInfo {
    pub board: String,
    pub firmware: String,
    pub protocol: u8,
}

/// Payload of STREAM_OPEN frames: the TCP port the C3 accepted on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamOpenInfo {
    pub port: u16,
}

/// Streaming frame decoder. Feed bytes as they arrive; pop frames as they complete.
/// Invalid/corrupt data is skipped one byte at a time while looking for the magic.
pub struct Decoder {
    buf: Vec<u8>,
}

impl Default for Decoder {
    fn default() -> Self {
        Self::new()
    }
}

impl Decoder {
    pub fn new() -> Self {
        Self {
            buf: Vec::with_capacity(4096),
        }
    }

    pub fn push(&mut self, data: &[u8]) {
        self.buf.extend_from_slice(data);
    }

    /// Try to extract the next complete frame.
    ///
    /// `Ok(None)`: need more bytes. `Err(CorruptFrame)`: a frame with a
    /// plausible header failed its CRC and has been discarded — the caller
    /// must tear down the affected stream (if its id is non-zero), because
    /// that byte stream just lost bytes and is no longer reliable.
    pub fn next_frame(&mut self) -> Result<Option<Frame>, CorruptFrame> {
        loop {
            // Resync: find magic.
            let start = match self.find_magic() {
                Some(s) => s,
                None => {
                    // No magic anywhere: this is all garbage. Keep the final
                    // byte (a magic may split across pushes) so the buffer
                    // cannot grow without bound.
                    let keep = core::cmp::min(self.buf.len(), 1);
                    let cut = self.buf.len() - keep;
                    self.buf.drain(0..cut);
                    return Ok(None);
                }
            };
            if start > 0 {
                self.buf.drain(0..start);
            }

            if self.buf.len() < HEADER_SIZE {
                return Ok(None);
            }

            let ftype = self.buf[2];
            let stream = u16::from_le_bytes([self.buf[3], self.buf[4]]);
            let len = u16::from_le_bytes([self.buf[5], self.buf[6]]) as usize;
            let crc = u16::from_le_bytes([self.buf[7], self.buf[8]]);

            if len > MAX_PAYLOAD {
                // Probably a false magic — skip forward.
                self.buf.drain(0..2);
                continue;
            }

            let total = HEADER_SIZE + len;
            if self.buf.len() < total {
                return Ok(None);
            }

            let check = crc16_update(
                crc16(&[
                    ftype,
                    (stream & 0xFF) as u8,
                    (stream >> 8) as u8,
                    (len & 0xFF) as u8,
                    (len >> 8) as u8,
                ]),
                &self.buf[HEADER_SIZE..total],
            );

            if check != crc {
                // The header was plausible enough to read a length that fits
                // the payload limit — treat this as a real frame with a
                // corrupt body, drop it whole, and report it so the affected
                // stream gets torn down (recoverable) instead of silently
                // skipping bytes inside a reliable stream.
                self.buf.drain(0..total);
                return Err(CorruptFrame { ftype, stream });
            }

            let payload = self.buf[HEADER_SIZE..total].to_vec();
            self.buf.drain(0..total);

            return Ok(Some(Frame { ftype, stream, payload }));
        }
    }

    fn find_magic(&self) -> Option<usize> {
        self.buf.windows(2).position(|w| w == MAGIC.to_le_bytes())
    }

    /// Drop everything buffered (e.g. after the link went quiet).
    pub fn reset(&mut self) {
        self.buf.clear();
    }

    /// Bytes currently buffered (for diagnostics).
    pub fn buffered(&self) -> usize {
        self.buf.len()
    }
}

impl fmt::Display for Frame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Frame(type={}, stream={}, len={})",
            self.ftype,
            self.stream,
            self.payload.len()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_frame() {
        let f = Frame::new(frame_type::STREAM_DATA, 7, vec![1, 2, 3, 4]).unwrap();
        let mut out = [0u8; 64];
        let n = f.encode_into(&mut out).unwrap();
        let mut d = Decoder::new();
        d.push(&out[..n]);
        assert_eq!(d.next_frame().unwrap().unwrap(), f);
        assert!(d.next_frame().unwrap().is_none());
    }

    #[test]
    fn roundtrip_large_frame() {
        let payload = vec![0xAB; MAX_PAYLOAD];
        let f = Frame::new(frame_type::STREAM_DATA, 1, payload).unwrap();
        let enc = f.encode();
        let mut d = Decoder::new();
        d.push(&enc);
        assert_eq!(d.next_frame().unwrap().unwrap(), f);
    }

    #[test]
    fn frames_back_to_back() {
        let a = Frame::empty(frame_type::KEEPALIVE);
        let b = Frame::new(frame_type::STREAM_DATA, 2, vec![42]).unwrap();
        let mut d = Decoder::new();
        d.push(&a.encode());
        d.push(&b.encode());
        assert_eq!(d.next_frame().unwrap().unwrap(), a);
        assert_eq!(d.next_frame().unwrap().unwrap(), b);
    }

    #[test]
    fn rpc_roundtrip() {
        let req = RpcRequest {
            id: 3,
            rpc: Rpc::Join {
                ssid: "home".into(),
                pass: "hunter2".into(),
            },
        };
        let f = Frame::json(frame_type::RPC_REQ, &req).unwrap();
        let mut out = [0u8; 2048];
        let n = f.encode_into(&mut out).unwrap();
        let mut d = Decoder::new();
        d.push(&out[..n]);
        let f2 = d.next_frame().unwrap().unwrap();
        let req2: RpcRequest = f2.payload_json().unwrap();
        assert_eq!(req2.id, 3);
        assert_eq!(
            req2.rpc,
            Rpc::Join {
                ssid: "home".into(),
                pass: "hunter2".into()
            }
        );
    }

    #[test]
    fn resyncs_after_garbage() {
        let f = Frame::empty(frame_type::KEEPALIVE);
        let mut d = Decoder::new();
        d.push(&[0xBE, 0x00, 0x55, 9, 9]);
        d.push(&f.encode());
        assert_eq!(d.next_frame().unwrap().unwrap().ftype, frame_type::KEEPALIVE);
    }

    #[test]
    fn bounded_buffer_on_pure_garbage() {
        let mut d = Decoder::new();
        for _ in 0..2000 {
            d.push(&[0x01, 0x02, 0x03, 0x04]);
            assert!(d.next_frame().unwrap().is_none());
        }
        assert!(d.buffered() <= 1, "garbage must not accumulate (was {})", d.buffered());
    }

    #[test]
    fn detects_corrupt_payload() {
        let f = Frame::new(frame_type::STREAM_DATA, 1, vec![1, 2, 3, 4, 5]).unwrap();
        let mut enc = f.encode();
        let last = enc.len() - 1;
        enc[last] ^= 0xFF; // corrupt payload
        let mut d = Decoder::new();
        d.push(&enc);
        // The corrupt frame must be reported with its stream id, not emitted.
        assert_eq!(
            d.next_frame().unwrap_err(),
            CorruptFrame {
                ftype: frame_type::STREAM_DATA,
                stream: 1,
            }
        );
        assert!(d.next_frame().unwrap().is_none());
    }

    #[test]
    fn corrupt_frame_does_not_eat_the_next_one() {
        // Frames share one buffer: dropping the corrupt one whole must let a
        // well-formed later frame decode (other streams on the same link).
        let a = Frame::new(frame_type::STREAM_DATA, 3, vec![7; 64]).unwrap();
        let mut bad = Frame::new(frame_type::STREAM_DATA, 4, vec![9; 64]).unwrap();
        let b = Frame::new(frame_type::STREAM_DATA, 5, vec![11; 64]).unwrap();
        let mut wire = a.encode();
        let mut enc_bad = bad.encode();
        let last = enc_bad.len() - 1;
        enc_bad[last] ^= 0xFF;
        wire.extend_from_slice(&enc_bad);
        wire.extend_from_slice(&b.encode());

        let mut d = Decoder::new();
        d.push(&wire);
        assert_eq!(d.next_frame().unwrap().unwrap(), a);
        assert_eq!(
            d.next_frame().unwrap_err(),
            CorruptFrame {
                ftype: frame_type::STREAM_DATA,
                stream: 4,
            }
        );
        assert_eq!(d.next_frame().unwrap().unwrap(), b);
    }

    #[test]
    fn ack_roundtrip() {
        let f = Frame::ack(9, 0xDEAD_BEEF);
        assert_eq!(f.ftype, frame_type::STREAM_ACK);
        assert_eq!(f.stream, 9);
        let mut d = Decoder::new();
        d.push(&f.encode());
        let f2 = d.next_frame().unwrap().unwrap();
        assert_eq!(f2.consumed(), Some(0xDEAD_BEEF));
    }

    #[test]
    fn data_frame_seq_roundtrip() {
        let f = Frame::data(3, 47, b"hello world").unwrap();
        let mut d = Decoder::new();
        d.push(&f.encode());
        let f2 = d.next_frame().unwrap().unwrap();
        let (seq, body) = f2.data_parts().unwrap();
        assert_eq!(seq, 47);
        assert_eq!(body, b"hello world");
        // Max-size body still fits the frame.
        assert!(Frame::data(1, 1, &[0u8; MAX_PAYLOAD - SEQ_SIZE]).is_ok());
        assert!(Frame::data(1, 1, &[0u8; MAX_PAYLOAD - SEQ_SIZE + 1]).is_err());
    }

    // -----------------------------------------------------------------------
    // Flow-control window arithmetic
    // -----------------------------------------------------------------------

    #[test]
    fn window_basic_accounting() {
        let mut w = StreamWindow::new(4);
        assert!(w.can_send_frame());

        w.note_sent_frame();
        assert_eq!(w.outstanding(), 1);
        assert_eq!(w.available(), 3);

        // Cumulative frame ack releases exactly what was consumed.
        assert!(w.note_ack(1));
        assert_eq!(w.available(), 4);

        // Stale/duplicate ack is a no-op (heals lost-ack without drift).
        assert!(w.note_ack(1));
        assert_eq!(w.outstanding(), 0);
    }

    #[test]
    fn window_small_frames_cannot_exceed_slots() {
        // Regression (v2 byte window): two small frames (HTTP header writes)
        // plus seven full-size frames fit in an 8 KiB byte window but are 9
        // frames — more than the receiver queue's 8 slots. Frame-count gating
        // blocks the 9th regardless of sizes.
        let mut w = StreamWindow::new(STREAM_WINDOW_FRAMES as u32);
        for i in 0..STREAM_WINDOW_FRAMES {
            assert!(w.can_send_frame(), "frame {i} should be allowed");
            w.note_sent_frame();
        }
        assert!(!w.can_send_frame());
        assert!(w.note_ack(1));
        assert!(w.can_send_frame());
    }

    #[test]
    fn window_rejects_impossible_ack() {
        let mut w = StreamWindow::new(8);
        w.note_sent_frame();
        w.note_sent_frame();
        w.note_sent_frame();
        // Receiver claiming 5000 frames consumed — more than were ever sent.
        assert!(!w.note_ack(5000));
        assert_eq!(w.outstanding(), 3);
    }

    #[test]
    fn window_wraps_at_u32_max() {
        // Streams are long-lived byte pipes; sent/acked must keep working
        // when the counters roll over.
        let mut w = StreamWindow::at(6, u32::MAX - 3, u32::MAX - 3);
        for _ in 0..5 {
            w.note_sent_frame(); // sent wraps around
        }
        assert_eq!(w.outstanding(), 5);
        assert!(w.note_ack(u32::MAX - 1)); // ack 2 more (crosses the wrap)
        assert_eq!(w.outstanding(), 3);
        assert!(w.can_send_frame());
    }

    #[test]
    fn window_never_exceeds_capacity_under_sustained_flow() {
        // Simulate the invariant: the sender fills its frame window with no
        // room left, the receiver then consumes a variable number of frames
        // and acks cumulatively, and so on for 100 000 rounds. Frames in
        // flight must never exceed the window (= receiver queue slots).
        let slots = STREAM_WINDOW_FRAMES as u32;
        let mut w = StreamWindow::new(slots);
        let mut sent_total: u64 = 0;
        let mut consumed_total: u64 = 0;
        for i in 0..100_000usize {
            while w.can_send_frame() {
                w.note_sent_frame();
                sent_total += 1;
                assert!(w.outstanding() <= slots, "invariant broken at {i}");
            }
            assert_eq!(w.outstanding(), slots); // window really was full

            // Receiver drains a variable number of what was delivered.
            let drain = 1 + (i as u64 * 104_729) % slots as u64;
            consumed_total = core::cmp::min(consumed_total + drain, sent_total);
            assert!(w.note_ack(consumed_total as u32), "legit ack rejected at {i}");
        }
        assert!(sent_total >= consumed_total);
    }
}
