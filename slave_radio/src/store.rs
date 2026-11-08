//! Persistent config store: the C3 keeps its own copy of the WiFi networks,
//! AP SSID, and desired mode in the dedicated `cfg` flash partition so it can
//! come up and connect before (or without) the Pico.
//!
//! Two 4 KiB sectors are used as a tiny journal (magic + sequence + CRC16 +
//! JSON); the newest valid entry wins, surviving power loss mid-write.
//!
//! Partition table (`partitions.csv`):
//! ```text
//! cfg,  data, 0x40, 0x10000, 0x4000
//! ```

use alloc::{string::String, vec, vec::Vec};
use core::fmt;

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::mutex::Mutex;
use embedded_storage::nor_flash::{NorFlash, ReadNorFlash};
use esp_storage::FlashStorage;
use radio_link::{crc16, Network, RadioMode};
use serde::{Deserialize, Serialize};

const SECTOR_A: u32 = 0x0001_0000;
const SECTOR_B: u32 = 0x0001_1000;
const SECTOR_SIZE: usize = 4096;
const MAGIC: u32 = 0x4333_4647; // "GF3C" — C3 radio config

/// Everything the C3 needs to run WiFi autonomously.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct StoredConfig {
    pub networks: Vec<Network>,
    pub ap_ssid: String,
    pub mode: RadioMode,
}

impl Default for StoredConfig {
    fn default() -> Self {
        // Factory: fallback AP only; saved networks arrive via the portal or
        // the Pico's boot-sync push.
        Self {
            networks: Vec::new(),
            ap_ssid: String::from("LED Board"),
            mode: RadioMode::AccessPoint,
        }
    }
}

#[derive(Debug)]
pub enum StoreError {
    Flash,
    Serde,
}

pub struct Store {
    flash: FlashStorage<'static>,
}

impl fmt::Debug for Store {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Store")
    }
}

pub type SharedStore = Mutex<CriticalSectionRawMutex, Store>;

fn encode(cfg: &StoredConfig) -> Result<Vec<u8>, StoreError> {
    serde_json::to_vec(cfg).map_err(|_| StoreError::Serde)
}

fn read_sector(flash: &mut FlashStorage, base: u32) -> Option<(u32, StoredConfig)> {
    let mut header = [0u8; 16];
    flash.read(base, &mut header).ok()?;

    let magic = u32::from_le_bytes(header[0..4].try_into().ok()?);
    let seq = u32::from_le_bytes(header[4..8].try_into().ok()?);
    let len = u32::from_le_bytes(header[8..12].try_into().ok()?);
    let crc = u32::from_le_bytes(header[12..16].try_into().ok()?);

    if magic != MAGIC || len as usize > SECTOR_SIZE - 16 {
        return None;
    }

    let mut payload = vec![0u8; len as usize];
    flash.read(base + 16, &mut payload).ok()?;

    if crc16(&payload) as u32 != crc {
        return None;
    }

    let cfg: StoredConfig = serde_json::from_slice(&payload).ok()?;
    Some((seq, cfg))
}

impl Store {
    pub fn new(flash: FlashStorage<'static>) -> Self {
        Self { flash }
    }

    /// The raw flash handle. The store mutex is the single gate for every
    /// flash writer (journal, OTA slots, otadata) — see `crate::ota`.
    pub fn flash_mut(&mut self) -> &mut FlashStorage<'static> {
        &mut self.flash
    }

    /// Load the newest valid record, or defaults when never written/corrupt.
    pub fn load(&mut self) -> StoredConfig {
        let a = read_sector(&mut self.flash, SECTOR_A);
        let b = read_sector(&mut self.flash, SECTOR_B);

        match (a, b) {
            (Some((sa, ca)), Some((sb, cb))) => {
                if sa.wrapping_sub(sb) < (1 << 31) || sa >= sb {
                    if sa >= sb {
                        ca
                    } else {
                        cb
                    }
                } else {
                    cb
                }
            }
            (Some((_, cfg)), None) | (None, Some((_, cfg))) => cfg,
            (None, None) => StoredConfig::default(),
        }
    }

    /// Write the record to the sector that is currently *not* newest.
    pub fn save(&mut self, cfg: &StoredConfig) -> Result<(), StoreError> {
        let payload = encode(cfg)?;
        let crc = crc16(&payload) as u32;

        let a = read_sector(&mut self.flash, SECTOR_A).map(|(seq, _)| seq);
        let b = read_sector(&mut self.flash, SECTOR_B).map(|(seq, _)| seq);

        let (target, seq) = match (a, b) {
            (None, None) => (SECTOR_A, 1),
            (Some(_), None) => (SECTOR_B, a.unwrap().wrapping_add(1)),
            (None, Some(_)) => (SECTOR_A, b.unwrap().wrapping_add(1)),
            (Some(sa), Some(sb)) => {
                if sa >= sb {
                    (SECTOR_B, sa.wrapping_add(1))
                } else {
                    (SECTOR_A, sb.wrapping_add(1))
                }
            }
        };

        let mut record: Vec<u8> = Vec::with_capacity(16 + payload.len());
        record.extend_from_slice(&MAGIC.to_le_bytes());
        record.extend_from_slice(&seq.to_le_bytes());
        record.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        record.extend_from_slice(&crc.to_le_bytes());
        record.extend_from_slice(&payload);

        // Pad to the flash's 4-byte write alignment.
        while record.len() % 4 != 0 {
            record.push(0xFF);
        }

        self.flash
            .erase(target, target + SECTOR_SIZE as u32)
            .map_err(|_| StoreError::Flash)?;
        self.flash.write(target, &record).map_err(|_| StoreError::Flash)?;

        log::info!("store: saved {} bytes (seq {})", record.len(), seq);
        Ok(())
    }

    /// Replace the network list wholesale (used by `Rpc::SetNetworks`).
    pub fn set(
        &mut self,
        networks: Vec<Network>,
        ap_ssid: String,
        mode: RadioMode,
    ) -> Result<StoredConfig, StoreError> {
        let mut cfg = self.load();
        cfg.networks = networks;
        cfg.ap_ssid = ap_ssid;
        cfg.mode = mode;
        self.save(&cfg)?;
        Ok(cfg)
    }

    /// Add or update one network (used by joins, from Pico or portal).
    pub fn upsert_network(&mut self, ssid: &str, pass: &str) -> Result<StoredConfig, StoreError> {
        let mut cfg = self.load();

        if let Some(existing) = cfg.networks.iter_mut().find(|n| n.ssid == ssid) {
            existing.pass = String::from(pass);
        } else {
            cfg.networks.push(Network {
                ssid: String::from(ssid),
                pass: String::from(pass),
            });
        }

        self.save(&cfg)?;
        Ok(cfg)
    }

    pub fn forget(&mut self, ssid: &str) -> Result<StoredConfig, StoreError> {
        let mut cfg = self.load();
        cfg.networks.retain(|n| n.ssid != ssid);
        self.save(&cfg)?;
        Ok(cfg)
    }

    pub fn set_mode(&mut self, mode: RadioMode) -> Result<StoredConfig, StoreError> {
        let mut cfg = self.load();
        cfg.mode = mode;
        self.save(&cfg)?;
        Ok(cfg)
    }
}
