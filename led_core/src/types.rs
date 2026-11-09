use crate::config::LedBoardConfig;
use alloc::{vec::Vec};
use animations::{AnimationEnum, AnimationParams};
use core::net::Ipv4Addr;
use embedded_graphics::pixelcolor::Rgb888;
use embedded_tools::config::{ConfigFile, storage::LocalFsConfigFileStorage};
use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex,
    channel::{self, Channel},
    watch::{self, Watch},
};
use serde::{Deserialize, Serialize};

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
    Reset,
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
    PowerLimit(f32),
    ParamsBuffer(Vec<AnimationParams>),
    FrameBuffer(Vec<Rgb888>),
    /// Show `animation` over the playlist until StatusClear — the board
    /// reads its own state (join progress, AP fallback, link lost).
    Status(AnimationEnum),
    /// Leave status-screen mode; the playlist resumes.
    StatusClear,
}

// Capacity 4: the status screens + playlist swaps must never block a
// producer task (main / pump) — worst case a stale frame command drops.
pub type DisplayWorkerChannel = Channel<CriticalSectionRawMutex, DisplayWorkerMessage, 4>;
pub type DisplayWorkerSender = channel::Sender<'static, CriticalSectionRawMutex, DisplayWorkerMessage, 4>;
pub type DisplayWorkerReceiver = channel::Receiver<'static, CriticalSectionRawMutex, DisplayWorkerMessage, 4>;

pub type ActivityWatch = Watch<CriticalSectionRawMutex, u64, 1>;
pub type ActivityWatchSender = watch::Sender<'static, CriticalSectionRawMutex, u64, 1>;
pub type ActivityWatchReceiver = watch::Receiver<'static, CriticalSectionRawMutex, u64, 1>;

/// Board config file for a platform's file store. `LedBoardConfigurator`
/// (add/remove networks) is implemented for this.
pub type LedBoardConfigFile<FS> = ConfigFile<LedBoardConfig, LocalFsConfigFileStorage<FS>>;
