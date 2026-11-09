//! The platform boundary of the LED board system.
//!
//! Everything the web app and the display worker need from the *machine* —
//! reboot, watchdog, status LED, WiFi provisioning, an OTA sink, and the
//! file store — is reached through [`Board`] and [`OtaBackend`]. The Pico
//! implementation talks to rp235x flash / cyw43 / the ESP32 slave radio;
//! the simulator implementation answers with a file, a directory, and
//! canned radio behaviour.
//!
//! Storage flows through [`LocalFsTrait`]: littlefs over flash on the
//! board, a plain directory under `tmp/host_board/` on the Mac — same
//! file names (`config.json`, the default playlist), same handler code.

use core::future::Future;

use alloc::string::String;
use alloc::vec::Vec;

use embedded_tools::local_fs::LocalFsTrait;

/// A scan result row for `GET /api/wifi` (platform-neutral mirror of the
/// radio/cyw43 scan entry).
#[derive(Clone)]
pub struct WifiScanEntry {
    pub ssid: String,
    pub signal_strength: i8,
    pub password_required: bool,
}

/// What a platform does after a network has been persisted + requested via
/// `POST /api/wifi`.
pub enum JoinOutcome {
    /// The radio joined (or refused) right now — no reboot needed. The
    /// fields echo into the UI's status card.
    Joined { ssid: String, joined: bool, mode: String },
    /// The platform applies saved networks at boot — the API responds and
    /// the core triggers [`Board::reboot`] so the join loop picks it up.
    RebootScheduled { ssid: String },
    /// No radio reachable right now; the string is the 503 body message.
    Unavailable(String),
}

/// One in-progress firmware image. The `/api/ota` pump in
/// [`crate::tasks::http::ota`] drives this; the RP2350 writes flash
/// partition slots, the simulator writes a file and verifies the hash.
pub trait OtaWriter {
    type Error: core::fmt::Debug;

    fn write_chunk(&mut self, chunk: &[u8]) -> Result<(), Self::Error>;

    /// Verify (where the platform can) and commit the image.
    fn finalise(&mut self) -> Result<(), Self::Error>;
}

/// Factory for [`OtaWriter`]s, bound to wherever the platform keeps
/// firmware images.
pub trait OtaBackend: Clone {
    type Writer: OtaWriter;
    type Error: core::fmt::Debug;

    fn begin(&self, expected_sha256: String) -> Result<Self::Writer, Self::Error>;
}

/// Everything the core system logic needs from the machine it runs on.
///
/// Implementors must be cheap to clone (internals are `Arc`/static
/// handles) because each request handler and each task holds one.
#[allow(async_fn_in_trait)] // Single-executor by design; see module docs.
pub trait Board: Clone + 'static {
    /// The platform's file store (config.json, playlists).
    type Fs: LocalFsTrait + Clone;

    /// Where `/api/ota` images go on this machine.
    type Ota: OtaBackend;

    /// Hard restart the machine (sim: exit the process).
    fn reboot(&self) -> impl Future<Output = ()>;

    /// Stop the watchdog so a long flash operation cannot reset the chip
    /// mid-write (sim: no-op).
    fn stop_watchdog(&self);

    /// Blink the status LED — the heartbeat task's indicator (sim: the
    /// window title).
    fn blink(&self, on: bool) -> impl Future<Output = ()>;

    /// `GET /api/wifi` — scan for networks, or `Err(message)` when no
    /// radio is reachable (surfaced as 503).
    fn scan(&self) -> impl Future<Output = Result<Vec<WifiScanEntry>, String>>;

    /// `POST /api/wifi` — make the platform apply `ssid`/`pass` now. The
    /// core has already persisted the credentials before calling this.
    fn join(&self, ssid: String, pass: String) -> impl Future<Output = JoinOutcome>;

    /// `GET /api/join-status` — the join loop's self-report (JSON).
    fn join_status_json(&self) -> impl Future<Output = String>;

    /// Called after the core saves the board config, so radio-backed
    /// platforms can mirror it to the slave radio. Default: no-op.
    fn config_saved(&self) -> impl Future<Output = ()> {
        core::future::ready(())
    }
}
