//! Wire protocol for the RP2350 ⇄ ESP32-C3 slave-radio UART link.
//!
//! The link multiplexes three things over one serial connection:
//!
//! 1. **RPC** request/response pairs (JSON) used to drive the radio: scan, join,
//!    status, credential sync, and the LED heartbeat.
//! 2. **Stream** frames — each frame carries a chunk of one byte stream. Streams
//!    are opened by the C3 whenever a TCP client (LAN or the AP portal network)
//!    connects to it; the stream is piped to the Pico where it is presented to
//!    `picoserve` as a socket. This lets the Pico serve its whole web app
//!    (HTTP + WebSockets + OTA uploads) with no IP stack of its own.
//! 3. **Events** — status updates pushed from the C3 (connected / AP started / etc).
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

/// Protocol revision, carried in the HELLO handshake.
pub const PROTOCOL_VERSION: u8 = 1;

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
    pub fn next_frame(&mut self) -> Option<Frame> {
        loop {
            // Resync: find magic.
            let start = self.find_magic()?;
            if start > 0 {
                self.buf.drain(0..start);
            }

            if self.buf.len() < HEADER_SIZE {
                return None;
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
                return None;
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
                // Bad CRC — resync inside this frame.
                self.buf.drain(0..2);
                continue;
            }

            let payload = self.buf[HEADER_SIZE..total].to_vec();
            self.buf.drain(0..total);

            return Some(Frame { ftype, stream, payload });
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
        assert_eq!(d.next_frame().unwrap(), f);
        assert!(d.next_frame().is_none());
    }

    #[test]
    fn roundtrip_large_frame() {
        let payload = vec![0xAB; MAX_PAYLOAD];
        let f = Frame::new(frame_type::STREAM_DATA, 1, payload).unwrap();
        let enc = f.encode();
        let mut d = Decoder::new();
        d.push(&enc);
        assert_eq!(d.next_frame().unwrap(), f);
    }

    #[test]
    fn frames_back_to_back() {
        let a = Frame::empty(frame_type::KEEPALIVE);
        let b = Frame::new(frame_type::STREAM_DATA, 2, vec![42]).unwrap();
        let mut d = Decoder::new();
        d.push(&a.encode());
        d.push(&b.encode());
        assert_eq!(d.next_frame().unwrap(), a);
        assert_eq!(d.next_frame().unwrap(), b);
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
        let f2 = d.next_frame().unwrap();
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
        assert_eq!(d.next_frame().unwrap().ftype, frame_type::KEEPALIVE);
    }

    #[test]
    fn detects_corrupt_payload() {
        let f = Frame::new(frame_type::STREAM_DATA, 1, vec![1, 2, 3, 4, 5]).unwrap();
        let mut enc = f.encode();
        let last = enc.len() - 1;
        enc[last] ^= 0xFF; // corrupt payload
        let mut d = Decoder::new();
        d.push(&enc);
        // The corrupt frame must not be emitted; resync finds nothing (no second magic).
        assert!(d.next_frame().is_none());
    }
}
