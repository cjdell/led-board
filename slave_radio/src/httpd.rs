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
use embassy_net::tcp::TcpSocket;
use embassy_net::Stack;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::mutex::Mutex;
use embassy_time::{Duration, Timer};
use embedded_io_async::Write as _;

use crate::link;
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
    for _ in 0..3 {
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
            Some(head) => {
                if let Some(response) = portal::route(&head).await {
                    let _ = socket.write_all(response).await;
                    let _ = socket.flush().await;
                } else if state::pico_online() {
                    relay_stream(&mut socket, prefix.clone()).await;
                } else {
                    let page = b"HTTP/1.1 503 Service Unavailable\r\n\
                        Content-Type: text/html\r\nConnection: close\r\n\r\n\
                        <p>The main board is offline. Use the <a href=\"/\">WiFi setup page</a>.</p>";
                    let _ = socket.write_all(page).await;
                }
            }
            None => {}
        }

        socket.close();
    }
}

// ---------------------------------------------------------------------------
// STA interface: pure relay
// ---------------------------------------------------------------------------

#[embassy_executor::task(pool_size = 3)]
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

        if state::pico_online() {
            relay_stream(&mut socket, Vec::new()).await;
        } else {
            let body = b"HTTP/1.1 503 Service Unavailable\r\nConnection: close\r\nContent-Length: 0\r\n\r\n";
            let _ = socket.write_all(body).await;
        }

        socket.close();
    }
}

// ---------------------------------------------------------------------------
// Socket ⇄ stream pump
// ---------------------------------------------------------------------------

/// Relay one accepted connection to the Pico. `prefix` holds bytes already
/// read from the client (the AP acceptor needs them for routing) and gets
/// forwarded first.
async fn relay_stream(socket: &mut TcpSocket<'_>, prefix: Vec<u8>) {
    let Some((id, queue)) = link::open_stream(80).await else {
        return;
    };

    if !prefix.is_empty() {
        link::send_stream_data(id, &prefix).await;
    }

    let (mut reader, mut writer) = socket.split();
    let mut buf = [0u8; 1024];

    loop {
        let either = embassy_futures::select::select(reader.read(&mut buf), queue.receive()).await;

        match either {
            embassy_futures::select::Either::First(result) => match result {
                Ok(0) | Err(_) => break,
                Ok(n) => link::send_stream_data(id, &buf[..n]).await,
            },
            embassy_futures::select::Either::Second(payload) => {
                if payload.is_empty() {
                    // The Pico closed the stream.
                    break;
                }
                if writer.write_all(&payload).await.is_err() {
                    break;
                }
            }
        }
    }

    let _ = writer.flush().await;
    link::close_stream(id).await;
}

// ---------------------------------------------------------------------------
// Request head parsing (shared by the AP acceptor and the portal)
// ---------------------------------------------------------------------------

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

    let mut head = portal::parse_head(prefix);

    // Pull a declared (small) body.
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
