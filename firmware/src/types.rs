//! Pico-flavoured type aliases over the platform-neutral core types: the
//! concrete littlefs-on-flash store, its config file, and the rp235x
//! watchdog the OTA flow must pause.

use alloc::sync::Arc;
use embassy_rp::watchdog::Watchdog;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, rwlock::RwLock};
use rp235x_embedded_tools::flash::LittleFsFlashStorage;

pub use led_core::types::*;

pub type LocalFs = embedded_tools::local_fs::LocalFs<LittleFsFlashStorage>;

pub type LedBoardConfigFile = led_core::types::LedBoardConfigFile<LocalFs>;

pub type SharedWatchdog = Arc<RwLock<CriticalSectionRawMutex, Watchdog>>;
