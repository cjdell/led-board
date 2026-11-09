//! The simulator's [`Board`]: the Mac pretending to be an LED art board.
//!
//! Where the Pico calls ROM reboot, blinks a status LED, and asks cyw43 or
//! the ESP32 slave radio for scans, this answers with a process exit, the
//! window title, and a canned 2.4 GHz survey — the core cannot tell the
//! difference, which is the whole point.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use led_core::board::{Board, JoinOutcome, WifiScanEntry};

use crate::fs::HostFs;
use crate::ota::FileOtaBackend;

#[derive(Clone)]
pub struct SimBoard {
    /// Set by the heartbeat task; the window title shows it as a dot.
    heartbeat: Arc<AtomicBool>,
    /// The network a provisioning session asked to join, for the status card.
    joined: Arc<Mutex<Option<String>>>,
}

impl SimBoard {
    pub fn new(heartbeat: Arc<AtomicBool>) -> Self {
        Self {
            heartbeat,
            joined: Arc::new(Mutex::new(None)),
        }
    }
}

impl Board for SimBoard {
    type Fs = HostFs;
    type Ota = FileOtaBackend;

    async fn reboot(&self) {
        println!("(sim) reboot requested — exiting. `just sim` to bring the board back up.");
        std::process::exit(0);
    }

    fn stop_watchdog(&self) {
        // No watchdog on a Mac.
    }

    async fn blink(&self, on: bool) {
        self.heartbeat.store(on, Ordering::Relaxed);
    }

    async fn scan(&self) -> Result<Vec<WifiScanEntry>, String> {
        // Canned survey, open + secured mix, so the provisioning UI has
        // something to render signal bars for.
        Ok(vec![
            WifiScanEntry {
                ssid: "SimNet 5G".into(),
                signal_strength: -42,
                password_required: true,
            },
            WifiScanEntry {
                ssid: "Leighhack".into(),
                signal_strength: -58,
                password_required: true,
            },
            WifiScanEntry {
                ssid: "Bench AP".into(),
                signal_strength: -66,
                password_required: false,
            },
            WifiScanEntry {
                ssid: "SimNet Guest".into(),
                signal_strength: -78,
                password_required: false,
            },
        ])
    }

    async fn join(&self, ssid: String, _pass: String) -> JoinOutcome {
        // The sim has no radio to join — record the intent as connected.
        *self.joined.lock().unwrap() = Some(ssid.clone());

        println!("(sim) \"joined\" {ssid} (pretend IP 192.168.99.99)");

        JoinOutcome::Joined {
            ssid,
            joined: true,
            mode: "Station".into(),
        }
    }

    async fn join_status_json(&self) -> String {
        let joined = self.joined.lock().unwrap().clone();

        serde_json::json!({
            "mode": "Station",
            "joined": joined.is_some(),
            "ssid": joined.clone().unwrap_or_else(|| "SimNet 5G".into()),
            "ip": if joined.is_some() { "192.168.99.99" } else { "127.0.0.1" },
            "attempts": 0,
            "ap_up": false,
            "sim": true,
        })
        .to_string()
    }
}
