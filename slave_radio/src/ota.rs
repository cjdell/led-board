//! Over-the-air firmware update for the slave radio itself.
//!
//! If the radio dies, the board's *network* dies with it, so updates use the
//! ESP A/B partition model: the image streams into the **inactive** OTA slot,
//! is sanity-checked (ESP image magic; the second-stage bootloader verifies the
//! header checksum on every load), `otadata` is flipped, and the chip resets.
//! A bad image therefore fails bootloader verification and it falls back to
//! the other slot on its own; the manual escape hatch for an image that boots
//! but is unwanted is holding GPIO9 (BOOT, pulled up) low across a reset,
//! which toggles the active slot — [`boot_guard`].
//!
//! While an upload runs, the config-store mutex is held: it owns the single
//! `FlashStorage` handle, so it doubles as the gate that serialises every
//! flash writer (journal sectors, otadata, OTA slots) against each other.
//! Journal writes pause for the duration of an upload by design.

use alloc::{format, string::String, string::ToString as _, vec::Vec};

use embassy_net::tcp::TcpSocket;
use embassy_time::Timer;
use embedded_io_async::Write as _;
use embedded_storage::{ReadStorage, Storage};
use esp_bootloader_esp_idf::ota::OtaImageState;
use esp_bootloader_esp_idf::ota_updater::OtaUpdater;
use esp_bootloader_esp_idf::partitions::{AppPartitionSubType, PARTITION_TABLE_MAX_LEN};
use esp_storage::FlashStorage;
use serde_json::json;

use crate::httpd;
use crate::httpd::JSON;
use crate::portal::RequestHead;

/// Largest accepted image: the OTA slot size (`partitions.csv`).
const MAX_IMAGE: usize = 0x1F0000;
/// First byte of every ESP image header; the bootloader validates the rest
/// (segment table + header checksum) at load time.
const ESP_IMAGE_MAGIC: u8 = 0xE9;
/// Read buffer while pumping the upload into flash. 4096 = one flash sector:
/// `FlashStorage::write` does sector read-modify-erase-program internally, so
/// aligned full-sector writes erase exactly once per sector.
const PAGE: usize = 4096;

/// True for the radio's own OTA endpoint — `POST /api/ota-radio` (upload)
/// and `GET /api/ota-radio` (status). Both acceptors route these to [`serve`]
/// instead of the portal or the Pico relay.
pub fn is_own(head: &RequestHead) -> bool {
    head.path.split('?').next() == Some("/api/ota-radio")
        && (head.method == "POST" || head.method == "GET")
}

fn slot_name(part: esp_bootloader_esp_idf::partitions::AppPartitionSubType) -> String {
    match part {
        AppPartitionSubType::Ota0 => String::from("ota_0"),
        AppPartitionSubType::Ota1 => String::from("ota_1"),
        _ => String::from("?"),
    }
}

fn dbg_str(err: impl core::fmt::Debug) -> String {
    format!("{err:?}")
}

/// Runs once from `main`, before the scheduler starts: if BOOT (GPIO9) is held
/// low, flip `otadata` to the other app slot and reset into it; otherwise
/// confirm a freshly activated image (`New`/`PendingVerify` → `Valid`) so the
/// bootloader stops monitoring it for auto-rollback.
///
/// Best-effort: any flash/partition error must never block boot — log and
/// carry on with whatever the bootloader picked.
pub fn boot_guard(flash: &mut FlashStorage<'_>, boot_low: bool) {
    let mut buf = alloc::boxed::Box::new([0u8; PARTITION_TABLE_MAX_LEN]);
    let Ok(mut updater) = OtaUpdater::new(flash, &mut *buf) else {
        log::warn!("ota: boot guard skipped (no otadata / <2 app slots?)");
        return;
    };

    if boot_low {
        if let Ok((_region, next)) = updater.next_partition() {
            log::warn!("ota: BOOT held at reset — switching slot to {}", slot_name(next));
            let flipped = updater
                .ota_data()
                .and_then(|mut ota| ota.set_current_app_partition(next));
            if flipped.is_ok() {
                esp_hal::system::software_reset();
            }
            log::error!("ota: slot switch failed; booting current image");
        }
        return;
    }

    if let Ok(state) = updater.current_ota_state() {
        if matches!(state, OtaImageState::New | OtaImageState::PendingVerify) {
            if updater.set_current_ota_state(OtaImageState::Valid).is_ok() {
                log::info!("ota: confirmed first boot of new image (was {:?})", state);
            }
        }
    }
}

/// `GET /api/ota-radio` (portal-claimed): what is running, and where an
/// update would go.
pub async fn status_json() -> Vec<u8> {
    let store = httpd::store();
    let mut store = store.lock().await;
    // Transient 3 KB partition-table scratch: a store-resident buffer once
    // cost a permanent 3 KB off the 112 KB heap and pulled the 4-way burst
    // heap floor below the stream-queue cliff (window timeouts). Allocating
    // per call keeps it short-lived; the store mutex still gates flash use.
    let mut tbl = alloc::boxed::Box::new([0u8; PARTITION_TABLE_MAX_LEN]);
    let (running, target) = match OtaUpdater::new(store.flash_mut(), &mut *tbl) {
        Ok(mut updater) => {
            let running = updater
                .selected_partition()
                .map(slot_name)
                .unwrap_or_else(|_| String::from("?"));
            let target = updater
                .next_partition()
                .map(|(_r, part)| slot_name(part))
                .unwrap_or_else(|_| String::from("?"));
            (running, target)
        }
        Err(_) => (String::from("?"), String::from("?")),
    };
    let body = json!({
        "version": env!("CARGO_PKG_VERSION"),
        "slot": running,
        "update_writes_to": target,
        "max_image": MAX_IMAGE,
    })
    .to_string();
    httpd::response("200 OK", JSON, body.as_bytes())
}

/// Serve the radio's own `/api/ota-radio`: `GET` reports slot/version,
/// `POST` streams the body into the inactive slot, activates it, and
/// reboots. `head.body` already carries the prefix the acceptor buffered
/// (part of the image) — it is written first.
pub async fn serve(socket: &mut TcpSocket<'_>, head: &RequestHead) {
    if head.method == "GET" {
        let resp = status_json().await;
        let _ = socket.write_all(&resp).await;
        return;
    }
    let total = head.content_length;
    if total == 0 || total > MAX_IMAGE {
        let msg = format!("expected a firmware image, 1..{MAX_IMAGE} bytes, got {total}");
        log::warn!("ota: rejected upload: {msg}");
        let _ = socket
            .write_all(&httpd::response("400 Bad Request", JSON, msg.as_bytes()))
            .await;
        return;
    }

    log::info!("ota: update started: {total} bytes");
    match update(socket, head, total).await {
        Ok(slot) => {
            let msg = format!(r#"{{"ok":true,"slot":"{slot}","rebooting":true}}"#);
            let _ = socket.write_all(&httpd::response("200 OK", JSON, msg.as_bytes())).await;
            let _ = socket.flush().await;
            log::info!("ota: image staged in {slot} — rebooting");
            Timer::after_millis(500).await;
            esp_hal::system::software_reset();
        }
        Err(err) => {
            log::error!("ota: update failed: {err}");
            let msg = json!({ "error": err }).to_string();
            let _ = socket.write_all(&httpd::response("500 Internal Server Error", JSON, msg.as_bytes())).await;
            // The inactive slot may hold partial garbage; that is harmless —
            // it stays unselected and the next upload overwrites it.
        }
    }
}

async fn update(socket: &mut TcpSocket<'_>, head: &RequestHead, total: usize) -> Result<String, String> {
    let store = httpd::store();
    let mut store = store.lock().await;
    let mut tbl = alloc::boxed::Box::new([0u8; PARTITION_TABLE_MAX_LEN]);
    let mut updater = OtaUpdater::new(store.flash_mut(), &mut *tbl).map_err(dbg_str)?;

    let slot;
    {
        let (mut region, part) = updater.next_partition().map_err(dbg_str)?;
        slot = slot_name(part);
        if total > region.capacity() {
            return Err(format!("image larger than slot {slot} ({} bytes)", region.capacity()));
        }

        let mut written: u32 = 0;
        // Seed from the acceptor's head/body buffer — those bytes belong to
        // the image (the acceptor consumed them off the socket).
        if !head.body.is_empty() {
            let want = core::cmp::min(head.body.len(), total);
            region.write(written, &head.body[..want]).map_err(dbg_str)?;
            written += want as u32;
        }

        let mut page = [0u8; PAGE];
        let mut since_yield = 0usize;
        while (written as usize) < total {
            let n = socket.read(&mut page).await.map_err(|_| String::from("socket read error"))?;
            if n == 0 {
                return Err(format!("connection closed at {written}/{total} bytes"));
            }
            let want = core::cmp::min(n, total - written as usize);
            region.write(written, &page[..want]).map_err(dbg_str)?;
            written += want as u32;

            since_yield += want;
            if since_yield >= 32 * 1024 {
                // Sector erase/program blocks with interrupts disabled for
                // short windows; yield so link/ACK traffic interleaves.
                since_yield = 0;
                Timer::after_millis(0).await;
            }
        }

        let mut magic = [0u8; 1];
        region.read(0, &mut magic).map_err(dbg_str)?;
        if magic[0] != ESP_IMAGE_MAGIC {
            return Err(String::from("not an ESP image (magic mismatch — wrong file?)"));
        }
        // Drop the region borrow so `updater` is writable again.
    }

    updater.activate_next_partition().map_err(dbg_str)?;
    // New (not Valid): if bootloader auto-rollback is enabled it gets the
    // first-boot check; `boot_guard` confirms it to Valid on a healthy boot.
    updater.set_current_ota_state(OtaImageState::New).map_err(dbg_str)?;
    Ok(slot)
}
