//! Inbound HTTP:
//!
//! * STA interface :80 → every connection is relayed to the Pico over a
//!   `radio_link` stream (the Pico serves the real web app).
//! * AP interface :80 → captive-portal + WiFi provisioning UI served locally;
//!   anything else is relayed to the Pico when the link is up (so the LED
//!   board app is usable on the fallback network too).

use alloc::{boxed::Box, format, vec, vec::Vec};
use core::sync::atomic::{AtomicUsize, Ordering};

use embassy_executor::Spawner;
use embassy_net::tcp::{State as TcpState, TcpSocket};
use embassy_net::Stack;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::mutex::Mutex;
use embassy_time::{Duration, Instant, Timer};
use embedded_io_async::Write as _;

use crate::link;
use crate::ota;
use crate::portal::{self, RequestHead};
use crate::state;
use crate::store::{SharedStore, Store};

// ---------------------------------------------------------------------------
// Globals
// ---------------------------------------------------------------------------

static STORE_PTR: AtomicUsize = AtomicUsize::new(0);

/// Register the (leaked) config store so every task can reach it.
pub fn init_store(store: &'static Mutex<CriticalSectionRawMutex, Store>) {
    STORE_PTR.store(store as *const Mutex<CriticalSectionRawMutex, Store> as usize, Ordering::Relaxed);
}

/// Access the config store (main must have called [`init_store`]).
pub fn store() -> &'static SharedStore {
    let ptr = STORE_PTR.load(Ordering::Relaxed);
    assert_ne!(ptr, 0, "store not initialised");
    unsafe { &*(ptr as *const SharedStore) }
}

/// Start the HTTP listeners on both interfaces.
pub fn start(spawner: &Spawner, ap_stack: Stack<'static>, sta_stack: Stack<'static>) {
    for _ in 0..2 {
        spawner.spawn(ap_http_task(ap_stack).unwrap());
    }
    // Six STA relay slots: browsers routinely open six parallel connections
    // per host, and the Pico tunnel pool matches at TUNNEL_POOL = 6. The STA
    // stack's socket budget (16) covers 6 acceptor sockets + mDNS + slack.
    for _ in 0..6 {
        spawner.spawn(sta_relay_task(sta_stack).unwrap());
    }
}

// ---------------------------------------------------------------------------
// AP interface: portal locally, everything else relayed
// ---------------------------------------------------------------------------

#[embassy_executor::task(pool_size = 2)]
async fn ap_http_task(stack: Stack<'static>) {
    let rx: &'static mut [u8] = Box::leak(vec![0u8; 2048].into_boxed_slice());
    let tx: &'static mut [u8] = Box::leak(vec![0u8; 2048].into_boxed_slice());
    let mut prefix: Vec<u8> = Vec::with_capacity(4096);

    loop {
        let mut socket = TcpSocket::new(stack, rx, tx);
        socket.set_timeout(Some(Duration::from_secs(120)));

        if socket.accept(80).await.is_err() {
            Timer::after_millis(50).await;
            continue;
        }

        prefix.clear();

        match read_head(&mut socket, &mut prefix).await {
            Some(head) if ota::is_own(&head) => {
                // Radio's own firmware update/status — served locally (works
                // with the Pico offline; it is precisely for that case).
                ota::serve(&mut socket, &head).await;
            }
            Some(head) => {
                if let Some(response) = portal::route(&head).await {
                    let _ = socket.write_all(response).await;
                    let _ = socket.flush().await;
                } else if state::pico_online() {
                    relay_stream(&mut socket, prefix.clone()).await;
                } else {
                    let page = b"HTTP/1.1 503 Service Unavailable\r\n\
                        Content-Type: text/html\r\nConnection: close\r\n\r\n\
                        <p>The main board is offline. Use the <a href=\"/setup\">WiFi setup page</a>.</p>";
                    let _ = socket.write_all(page).await;
                }
            }
            None => {
                // Poisoned/scanner connection — see the STA acceptor: an RST
                // stops the peer's stale retransmissions from re-poisoning.
                socket.abort();
                continue;
            }
        }

        drain_close(&mut socket).await;
    }
}

// ---------------------------------------------------------------------------
// STA interface: pure relay
// ---------------------------------------------------------------------------

#[embassy_executor::task(pool_size = 6)]
async fn sta_relay_task(stack: Stack<'static>) {
    let rx: &'static mut [u8] = Box::leak(vec![0u8; 2048].into_boxed_slice());
    let tx: &'static mut [u8] = Box::leak(vec![0u8; 2048].into_boxed_slice());

    loop {
        let mut socket = TcpSocket::new(stack, rx, tx);
        socket.set_timeout(Some(Duration::from_secs(300)));

        if socket.accept(80).await.is_err() {
            Timer::after_millis(50).await;
            continue;
        }

        // Peek the request head so the radio's own management endpoints can
        // be served without involving the Pico; everything else is relayed as
        // raw bytes (the peeked prefix included), exactly as before.
        let mut prefix: Vec<u8> = Vec::new();
        let head_opt = read_head(&mut socket, &mut prefix).await;
        if let Some(h) = &head_opt {
            log::info!("relay: head m={} p={} cl={} own={} pre={}", h.method, h.path, h.content_length, ota::is_own(h), prefix.len());
        }
        match head_opt {
            Some(head) if ota::is_own(&head) => {
                log::info!("relay: STA ota-radio request");
                ota::serve(&mut socket, &head).await;
            }
            Some(head) if state::pico_online() => {
                log::info!("relay: STA conn, opening stream");
                relay_stream(&mut socket, head.raw).await;
                log::info!("relay: STA relay done");
            }
            Some(_) => {
                log::info!("relay: STA conn, PICO OFFLINE -> 503");
                let body = b"HTTP/1.1 503 Service Unavailable\r\nConnection: close\r\nContent-Length: 0\r\n\r\n";
                let _ = socket.write_all(body).await;
            }
            None => {
                // Garbage connection (poisoned stale bytes, scanner). An RST
                // instead of a graceful FIN is deliberate: the client kernel
                // retransmits the stale bytes on the same 4-tuple for
                // minutes, re-poisoning each new connection. The RST kills
                // the peer's socket immediately, so the retry lands clean.
                socket.abort();
                continue;
            }
        }

        drain_close(&mut socket).await;
    }
}

// ---------------------------------------------------------------------------
// Socket ⇄ stream pump
// ---------------------------------------------------------------------------

/// Close a connection gracefully, waiting until the socket has actually
/// finished draining before it is dropped. `TcpSocket::drop` removes the
/// socket from the stack immediately — any bytes still queued for transmit
/// (the tail of a response, a 503 page) are silently discarded, which
/// presented as truncated/RST'd responses to the client.
async fn drain_close(socket: &mut TcpSocket<'_>) {
    socket.close();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !matches!(socket.state(), TcpState::TimeWait | TcpState::Closed) {
        if Instant::now() >= deadline {
            break;
        }
        Timer::after_millis(25).await;
    }
}

/// Relay one accepted connection to the Pico. `prefix` holds bytes already
/// read from the client (the AP acceptor needs them for routing) and gets
/// forwarded first.
///
/// The pump is lossless in both directions: bytes toward the Pico wait on
/// its flow-control window (`send_stream_data` returns false only when the
/// stream has gone), and frames toward the browser release the Pico's window
/// with a cumulative frame-count `STREAM_ACK` once the TCP socket has taken
/// them. A slow browser therefore pauses picoserve instead of truncating the
/// response.
async fn relay_stream(socket: &mut TcpSocket<'_>, prefix: Vec<u8>) {
    let Some(entry) = link::open_stream(80).await else {
        log::warn!("relay: open_stream FAILED (no slot)");
        return;
    };
    let id = entry.id();
    log::info!("relay: stream {} open, forwarding {} prefix bytes", id, prefix.len());

    let (mut reader, mut writer) = socket.split();
    let mut buf = [0u8; 1024];

    let mut alive = true;
    if !prefix.is_empty() {
        alive = link::send_stream_data(entry, &prefix).await;
    }
    // Release the head-peek buffer (up to 4 KB) before the long pump — it is
    // heap that every concurrent stream would otherwise pin for its lifetime.
    drop(prefix);

    while alive {
        let either = embassy_futures::select::select(reader.read(&mut buf), entry.queue.receive()).await;

        match either {
            embassy_futures::select::Either::First(result) => match result {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    alive = link::send_stream_data(entry, &buf[..n]).await;
                }
            },
            embassy_futures::select::Either::Second(payload) => {
                if payload.is_empty() {
                    // The Pico closed the stream.
                    log::info!("relay: stream {} closed by pico", id);
                    break;
                }
                if writer.write_all(&payload).await.is_err() {
                    break;
                }
                // The TCP socket has taken this frame: the Pico's window
                // opens by one frame (cumulative ack — self-healing if an
                // ack frame is ever lost).
                link::ack_stream(id).await;
            }
        }
    }

    let _ = writer.flush().await;
    log::info!("relay: stream {} done", id);
    link::close_stream(id).await;
    link::release_stream(id);
}

// ---------------------------------------------------------------------------
// Request head parsing (shared by the AP acceptor and the portal)
// ---------------------------------------------------------------------------

/// Every RFC 9110 method token, prefix-matched against the first bytes of a
/// connection. A connection that does not start with one is garbage — stale
/// bytes from an aborted upload landing on a same-tuple reconnection (real
/// bench failure: a POST body's tail poisoned the next connection's head)
/// or a scanner — closed immediately so the client's retry lands clean.
const HTTP_METHODS: [&[u8]; 9] = [
    b"GET ",
    b"POST",
    b"HEAD ",
    b"PUT ",
    b"PATCH",
    b"DELETE",
    b"OPTION",
    b"CONNET",
    b"TRACE ",
];

/// Read the HTTP request head (and any body) into `prefix`, returning the
/// parsed request. `prefix` accumulates every byte consumed so it can be
/// replayed toward the Pico on the relay path.
pub async fn read_head(socket: &mut TcpSocket<'_>, prefix: &mut Vec<u8>) -> Option<RequestHead> {
    let mut tmp = [0u8; 512];

    // Head, capped.
    loop {
        if prefix.windows(4).any(|w| w == b"\r\n\r\n") || prefix.len() > 4096 {
            break;
        }
        match socket.read(&mut tmp).await {
            Ok(0) | Err(_) => break,
            Ok(n) => prefix.extend_from_slice(&tmp[..n]),
        }
    }

    if prefix.is_empty() {
        return None;
    }
    if !HTTP_METHODS.iter().any(|m| prefix.starts_with(m)) {
        log::warn!(
            "httpd: non-HTTP bytes on :80 ({:02x?}…) — dropping",
            &prefix[..prefix.len().min(4)]
        );
        return None;
    }

    let mut head = portal::parse_head(prefix);

    // Pull a declared body — but only when it is small enough to buffer.
    // A large upload must not grow `prefix` at all: on the fragmented 112 KB
    // heap a 7 KB realloc OOM-panicked the chip mid-upload. Big bodies stream
    // straight from the socket instead — `ota::update` seeds from `head.body`
    // and keeps reading, and relayed POSTs (e.g. /api/ota) forward the head
    // and pump the rest like any other bytes.
    if head.content_length > 0 && head.content_length <= 4096 {
        while head.body.len() < head.content_length && head.body.len() < 8192 {
            match socket.read(&mut tmp).await {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let want = core::cmp::min(n, head.content_length - head.body.len());
                    head.body.extend_from_slice(&tmp[..want]);
                    prefix.extend_from_slice(&tmp[..n]);
                }
            }
        }
    }

    // Refresh raw so the relay prefix and parsed body agree.
    head.raw = prefix.clone();
    Some(head)
}

/// Small text response helper (portal + errors).
pub fn response(status: &str, content_type: &str, body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(body.len() + 256);
    out.extend_from_slice(format!("HTTP/1.1 {status}\r\n").as_bytes());
    out.extend_from_slice(format!("Content-Type: {content_type}\r\n").as_bytes());
    out.extend_from_slice(format!("Content-Length: {}\r\n", body.len()).as_bytes());
    out.extend_from_slice(b"Access-Control-Allow-Origin: *\r\n");
    out.extend_from_slice(b"Access-Control-Allow-Methods: GET, POST, OPTIONS\r\n");
    out.extend_from_slice(b"Access-Control-Allow-Headers: Content-Type\r\n");
    out.extend_from_slice(b"Cache-Control: no-store\r\n");
    out.extend_from_slice(b"Connection: close\r\n\r\n");
    out.extend_from_slice(body);
    out
}

pub fn redirect(url: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(256);
    out.extend_from_slice(b"HTTP/1.1 302 Found\r\n");
    out.extend_from_slice(format!("Location: {url}\r\n").as_bytes());
    out.extend_from_slice(b"Content-Length: 0\r\nConnection: close\r\n\r\n");
    out
}

pub const HTML: &str = "text/html";
pub const JSON: &str = "application/json";
