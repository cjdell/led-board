use crate::{config::LedBoardConfig, flash::LittleFsFlashStorage};
use alloc::{sync::Arc, vec::Vec};
use animations::{AnimationEnum, AnimationParams};
use core::net::Ipv4Addr;
use embassy_rp::watchdog::Watchdog;
use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex,
    channel::{self, Channel},
    rwlock::RwLock,
    watch::{self, Watch},
};
use serde::{Deserialize, Serialize};
use utils::config::{ConfigFile, storage::LocalFsConfigFileStorage};

#[derive(Debug, Clone)]
pub enum EthernetSignalMessage {
    Connected(Ipv4Addr),
    Disconnected,
}

pub type EthernetSignal = Watch<CriticalSectionRawMutex, EthernetSignalMessage, 10>;
pub type EthernetSignalSender = watch::Sender<'static, CriticalSectionRawMutex, EthernetSignalMessage, 10>;

#[derive(Clone, Serialize, Deserialize)]
pub enum WebSocketIncomingMessage {
    Ping,
    FrameBuffer(Vec<u8>),
    Next,
    Animation(AnimationEnum, u32),
    Playlist {
        playlist: Vec<(AnimationEnum, u32)>,
        save: bool,
    },
    ParamsBuffer(Vec<u8>),
    PowerLimit(f32),
}

pub type WebSocketIncomingChannel = Channel<CriticalSectionRawMutex, WebSocketIncomingMessage, 1>;
pub type WebSocketIncomingSender = channel::Sender<'static, CriticalSectionRawMutex, WebSocketIncomingMessage, 1>;
pub type WebSocketIncomingReceiver = channel::Receiver<'static, CriticalSectionRawMutex, WebSocketIncomingMessage, 1>;

#[derive(Clone)]
pub enum DisplayWorkerMessage {
    Next,
    Animation(AnimationEnum, u32),
    Playlist(Vec<(AnimationEnum, u32)>),
    ParamsBuffer(Vec<AnimationParams>),
    PowerLimit(f32),
}

pub type DisplayWorkerChannel = Channel<CriticalSectionRawMutex, DisplayWorkerMessage, 1>;
pub type DisplayWorkerSender = channel::Sender<'static, CriticalSectionRawMutex, DisplayWorkerMessage, 1>;
pub type DisplayWorkerReceiver = channel::Receiver<'static, CriticalSectionRawMutex, DisplayWorkerMessage, 1>;

pub type ActivityWatch = watch::Watch<CriticalSectionRawMutex, u64, 1>;
pub type ActivityWatchReceiver = watch::Receiver<'static, CriticalSectionRawMutex, u64, 1>;

pub type SharedWatchdog = Arc<RwLock<CriticalSectionRawMutex, Watchdog>>;

pub type LocalFs = utils::local_fs::LocalFs<LittleFsFlashStorage>;

pub type LedBoardConfigFile = ConfigFile<LedBoardConfig, LocalFsConfigFileStorage<LocalFs>>;
