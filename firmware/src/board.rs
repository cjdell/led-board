//! The `led_core::board::Board` implementations for the RP2350: what
//! "reboot", "watchdog", "blink", "scan", "join" and "write an OTA image"
//! mean on real hardware.
//!
//! * [`RadioBoard`] (feature `radio`) — networking lives on the ESP32 slave
//!   radio, so provisioning calls go over the UART link RPC and config
//!   saves are mirrored to the C3.
//! * [`DirectBoard`] — the cyw43 (and usb/ppp) builds: a save reboots the
//!   board into the join loop, the scan runs on the Pico's own radio.
//!
//! OTA on both goes to an RP2350 partition slot via `rp235x_ota`.

use alloc::{string::String, vec::Vec};

#[cfg(not(feature = "radio"))]
use alloc::string::ToString as _;

use embassy_rp::rom_data;
use led_core::board::{Board, JoinOutcome, OtaBackend, OtaWriter, WifiScanEntry};
use rp235x_ota::{OtaError, Rp235xOta};

use crate::tasks::LedControl as _;
use crate::types::{LocalFs, SharedWatchdog};

#[cfg(feature = "radio")]
use crate::types::LedBoardConfigFile;

#[cfg(not(feature = "radio"))]
use crate::tasks::WifiControl;

#[cfg(not(feature = "radio"))]
fn wifi_entries(entries: Vec<(String, i8, bool)>) -> Vec<WifiScanEntry> {
    entries
        .into_iter()
        .map(|(ssid, signal_strength, password_required)| WifiScanEntry {
            ssid,
            signal_strength,
            password_required,
        })
        .collect()
}

/// RP2350 flash slots as an OTA sink for the shared `/api/ota` handler.
#[derive(Clone)]
pub struct RpOtaBackend(pub rp235x_embedded_tools::flash::FlashStorage);

impl OtaBackend for RpOtaBackend {
    type Writer = RpOtaWriter;
    type Error = OtaError;

    fn begin(&self, expected_sha256: String) -> Result<Self::Writer, Self::Error> {
        Rp235xOta::new(self.0.clone(), expected_sha256).map(RpOtaWriter)
    }
}

/// Newtype so the core's `OtaWriter` can be implemented for the rp235x OTA
/// writer (both are foreign types otherwise — orphan rules).
pub struct RpOtaWriter(Rp235xOta<rp235x_embedded_tools::flash::FlashStorage>);

impl OtaWriter for RpOtaWriter {
    type Error = OtaError;

    fn write_chunk(&mut self, chunk: &[u8]) -> Result<(), Self::Error> {
        self.0.write_chunk(chunk)
    }

    fn finalise(&mut self) -> Result<(), Self::Error> {
        self.0.finalise()
    }
}

/// The `Board` this binary is built for — one name for task wrappers that
/// the executor macro requires to be concrete.
#[cfg(feature = "radio")]
pub type ActiveBoard = RadioBoard;
#[cfg(not(feature = "radio"))]
pub type ActiveBoard = DirectBoard;

fn stop_watchdog(watchdog: &SharedWatchdog) {
    if let Ok(mut watchdog) = watchdog.try_write() {
        watchdog.stop();
    }
}

// ---------------------------------------------------------------------------
// ESP32 slave radio
// ---------------------------------------------------------------------------

#[cfg(feature = "radio")]
#[derive(Clone)]
pub struct RadioBoard {
    config_file: LedBoardConfigFile,
    watchdog: SharedWatchdog,
}

#[cfg(feature = "radio")]
impl RadioBoard {
    pub fn new(config_file: LedBoardConfigFile, watchdog: SharedWatchdog) -> Self {
        Self { config_file, watchdog }
    }
}

#[cfg(feature = "radio")]
impl Board for RadioBoard {
    type Fs = LocalFs;
    type Ota = RpOtaBackend;

    fn reboot(&self) -> impl core::future::Future<Output = ()> {
        // Stop the watchdog feeder re-arming the ROM's asynchronous reboot.
        rp235x_ota::REBOOTING.store(true, core::sync::atomic::Ordering::SeqCst);
        rom_data::reboot(0, 10, 0, 0);
        core::future::ready(())
    }

    fn stop_watchdog(&self) {
        stop_watchdog(&self.watchdog);
    }

    fn blink(&self, on: bool) -> impl core::future::Future<Output = ()> {
        crate::tasks::radio::RadioLed.blink(on)
    }

    async fn scan(&self) -> Result<Vec<WifiScanEntry>, String> {
        let Some(link) = crate::tasks::radio::link().await else {
            return Err("radio link not up".into());
        };

        Ok(link
            .scan()
            .await
            .into_iter()
            .map(|entry| WifiScanEntry {
                ssid: entry.ssid,
                signal_strength: entry.signal_strength,
                password_required: entry.password_required,
            })
            .collect())
    }

    async fn join(&self, ssid: String, pass: String) -> JoinOutcome {
        let Some(link) = crate::tasks::radio::link().await else {
            return JoinOutcome::Unavailable("radio link not up".into());
        };

        match link.join(ssid.clone(), pass.clone()).await {
            Some(status) => JoinOutcome::Joined {
                ssid: status.ssid.unwrap_or(ssid),
                joined: status.ip.is_some(),
                mode: alloc::format!("{:?}", status.mode),
            },
            None => JoinOutcome::Unavailable("radio did not answer".into()),
        }
    }

    async fn join_status_json(&self) -> String {
        // The Pico has no join loop of its own in radio mode; the C3 owns
        // it. Keep reporting the local wifi module's view (idle) so the UI
        // card renders the same schema.
        crate::tasks::wifi::join_status_json().await
    }

    async fn config_saved(&self) {
        crate::tasks::radio::sync_config(self.config_file.clone()).await;
    }
}

// ---------------------------------------------------------------------------
// Built-in WiFi (cyw43) / usb-ethernet / ppp
// ---------------------------------------------------------------------------

#[cfg(not(feature = "radio"))]
#[derive(Clone)]
pub struct DirectBoard {
    watchdog: SharedWatchdog,
    wifi_control: WifiControl,
}

#[cfg(not(feature = "radio"))]
impl DirectBoard {
    pub fn new(watchdog: SharedWatchdog, wifi_control: WifiControl) -> Self {
        Self { watchdog, wifi_control }
    }
}

#[cfg(not(feature = "radio"))]
impl Board for DirectBoard {
    type Fs = LocalFs;
    type Ota = RpOtaBackend;

    fn reboot(&self) -> impl core::future::Future<Output = ()> {
        // Stop the watchdog feeder re-arming the ROM's asynchronous reboot.
        rp235x_ota::REBOOTING.store(true, core::sync::atomic::Ordering::SeqCst);
        rom_data::reboot(0, 10, 0, 0);
        core::future::ready(())
    }

    fn stop_watchdog(&self) {
        stop_watchdog(&self.watchdog);
    }

    fn blink(&self, on: bool) -> impl core::future::Future<Output = ()> {
        self.wifi_control.blink(on)
    }

    async fn scan(&self) -> Result<Vec<WifiScanEntry>, String> {
        // The cyw43 itself answers (no C3 to relay through).
        crate::tasks::wifi::wifi_scan()
            .await
            .map(wifi_entries)
            .ok_or_else(|| "wifi control not ready".to_string())
    }

    async fn join(&self, ssid: String, _pass: String) -> JoinOutcome {
        // The join loop reads config once at boot — the API responds and the
        // core reboots us into the join loop.
        JoinOutcome::RebootScheduled { ssid }
    }

    async fn join_status_json(&self) -> String {
        crate::tasks::wifi::join_status_json().await
    }
}
