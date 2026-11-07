//! Shared radio state: the status snapshot served to the Pico and pushed as
//! events, plus the scan cache shown by the portal.

use alloc::{string::String, vec::Vec};

use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex, rwlock::RwLock,
};
use radio_link::{RadioMode, RadioStatus, ScanEntry};

/// The AP network is always 192.168.2.1/24 (matches DHCP + captive portal).
pub const AP_IP: core::net::Ipv4Addr = core::net::Ipv4Addr::new(192, 168, 2, 1);
pub const AP_IP_CIDR: u8 = 24;

/// Static hostname advertised over mDNS on whichever interface is up.
pub const MDNS_NAME: &str = "led-board";

pub type SharedMutex<T> = Mutex<CriticalSectionRawMutex, T>;

/// Latest status snapshot (also the source for `Rpc::Status`).
pub static STATUS: SharedMutex<Option<RadioStatus>> = SharedMutex::new(None);

/// Most recent scan, newest-first (the portal + `Rpc::Scan` read this).
pub static SCAN_CACHE: RwLock<CriticalSectionRawMutex, Vec<ScanEntry>> = RwLock::new(Vec::new());

/// True while the Pico keeps the link warm (HELLO/keepalives within 15s).
pub static PICO_LINK_UP: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

/// Uptime-ms (wrapping u32) of the last HELLO/KEEPALIVE seen from the Pico.
static PICO_LAST_SEEN_MS: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

/// Called by the link layer when any frame arrives from the Pico.
pub fn pico_seen() {
    PICO_LAST_SEEN_MS.store(embassy_time::Instant::now().as_millis() as u32, core::sync::atomic::Ordering::Relaxed);
    PICO_LINK_UP.store(true, core::sync::atomic::Ordering::Relaxed);
}

/// Whether the Pico has spoken on the link recently (wrapping-safe compare).
pub fn pico_online() -> bool {
    let last = PICO_LAST_SEEN_MS.load(core::sync::atomic::Ordering::Relaxed);
    if last == 0 {
        return false;
    }
    let now = embassy_time::Instant::now().as_millis() as u32;
    now.wrapping_sub(last) < 15_000
}

/// Firmware version string used in HELLO_ACK and the portal footer.
pub const FIRMWARE: &str = env!("CARGO_PKG_VERSION");

/// Publish a new status snapshot (and let the link layer diff-and-push).
pub async fn publish(status: RadioStatus) -> RadioStatus {
    let previous = {
        let mut current = STATUS.lock().await;
        let previous = current.clone();
        *current = Some(status.clone());
        previous
    };

    if previous.as_ref() != Some(&status) {
        crate::link::status_changed(&status).await;
    }

    status
}

pub async fn snapshot() -> Option<RadioStatus> {
    STATUS.lock().await.clone()
}

/// Build a status snapshot from live parameters.
pub fn build_status(
    mode: RadioMode,
    ssid: Option<String>,
    ip: Option<[u8; 4]>,
    rssi: Option<i8>,
    ap_ssid: Option<String>,
    ap_ip: Option<[u8; 4]>,
) -> RadioStatus {
    RadioStatus {
        mode,
        ssid,
        ip,
        rssi,
        ap_ssid,
        ap_ip,
        firmware: String::from(FIRMWARE),
        mac: [0; 6],
    }
}

/// Update the scan cache with fresh results.
pub async fn store_scan(entries: Vec<ScanEntry>) {
    *SCAN_CACHE.write().await = entries;
}

/// Current AP IP as octets (for status/events).
pub const fn ap_ip_octets() -> [u8; 4] {
    AP_IP.octets()
}
