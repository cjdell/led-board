use crate::config::LedBoardConfig;
use alloc::{string::String, vec::Vec};
use animations::{AnimationEnum, AnimationParams, PlaylistItem};
use core::net::Ipv4Addr;
use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex,
    channel::{self, Channel},
    watch::{self, Watch},
};
use embedded_graphics::pixelcolor::Rgb888;
use embedded_tools::config::{ConfigFile, storage::LocalFsConfigFileStorage};
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
        playlist: Vec<PlaylistItem>,
        save: bool,
    },
    Reset,
    /// Freeze / resume the playlist clock (screen holds the last frame).
    Pause(bool),
    /// Start playing playlist slot `n` right now ("play now" from the UI).
    JumpTo(u32),
    /// Drop the paint-stream suspend so the playlist resumes immediately.
    ResumeNow,
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
    /// Replace the live playlist. `items` empty = fall back to the built-in
    /// festival default. `source` tags where it came from for the UI.
    Playlist {
        items: Vec<PlaylistItem>,
        source: PlaylistSource,
    },
    PowerLimit(f32),
    ParamsBuffer(Vec<AnimationParams>),
    FrameBuffer(Vec<Rgb888>),
    Pause(bool),
    JumpTo(u32),
    ResumeNow,
    /// Show `animation` over the playlist until StatusClear — the board
    /// reads its own state (join progress, AP fallback, link lost).
    Status(AnimationEnum),
    /// Like [`Self::Status`], but auto-clears after `timeout_ms`. The WiFi
    /// join/AP cards use this so a failed network never leaves the panel
    /// stuck on a status card forever — the festival playlist resumes.
    StatusTimed(AnimationEnum, u32),
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

/// Where the board's live playlist came from — drives the UI's source badge
/// and the "unsaved changes" story.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlaylistSource {
    /// The file saved on flash (what boots).
    Saved,
    /// The built-in festival default.
    Default,
    /// Pushed live from a client, not persisted.
    Live,
}

impl PlaylistSource {
    /// Loggable label (defmt's `{}` wants plain strs, not Debug enums).
    pub fn label(self) -> &'static str {
        match self {
            PlaylistSource::Saved => "saved",
            PlaylistSource::Default => "default",
            PlaylistSource::Live => "live",
        }
    }
}

/// The animation slot the panel is showing right now (status screen,
/// preview override, or playlist item — in that precedence).
#[derive(Clone, Serialize, Deserialize)]
pub struct Playing {
    pub kind: String,
    pub title: String,
    pub animation: AnimationEnum,
    pub elapsed_ms: u32,
    pub duration_ms: u32,
}

/// Everything a client needs to draw the board's live state. Published as
/// JSON on [`StatusWatch`] about once a second and on every change.
#[derive(Clone, Serialize, Deserialize)]
pub struct BoardStatus {
    pub now_playing: Playing,
    pub playlist_index: usize,
    pub playlist_len: usize,
    pub paused: bool,
    /// A frame stream (the Grid painter) currently owns the panel.
    pub streaming: bool,
    /// A status card (join progress, link lost, …) is on top.
    pub status_screen: bool,
    /// Power budget in percent — an integer so the status JSON never drags
    /// the float formatter into the firmware image.
    pub power_limit_pct: u8,
    pub uptime_s: u64,
    /// The commit this image was built from (build-time `git rev-parse`) —
    /// a flashed board must never have to be identified by bundle size.
    pub firmware_sha: String,
    /// True when the image came from an uncommitted working tree.
    pub firmware_dirty: bool,
}

/// `GET /api/version` — identity of the firmware image on flash.
#[derive(Clone, Serialize)]
pub struct FirmwareVersion {
    pub git_sha: &'static str,
    pub dirty: bool,
    pub commit_time: &'static str,
    /// Wall-clock build time (UTC); changes every build, even for a dirty
    /// tree with the same commit. Used to verify OTA slot switching.
    pub build_time: &'static str,
    /// "pico" firmware or the host simulator.
    pub runtime: &'static str,
}

/// The live playlist as the display worker sees it — source included so the
/// UI can tell "what the board plays" apart from "what's on flash".
#[derive(Clone, Serialize, Deserialize)]
pub struct PlaylistInfo {
    pub playlist: Vec<PlaylistItem>,
    pub source: PlaylistSource,
    pub index: usize,
}

// The display worker owns these feeds; the web app pushes them to clients.
// 12 receiver slots ≈ the web task pool + a status poll per page — receiver
// count is per connection/request and freed when it ends. Values are
// pre-serialised JSON so every consumer (WS push, GET) is a cheap clone.
pub type StatusWatch = Watch<CriticalSectionRawMutex, String, 12>;
pub type StatusWatchSender = watch::Sender<'static, CriticalSectionRawMutex, String, 12>;
pub type StatusWatchReceiver = watch::Receiver<'static, CriticalSectionRawMutex, String, 12>;
pub type PlaylistWatch = Watch<CriticalSectionRawMutex, String, 12>;
pub type PlaylistWatchSender = watch::Sender<'static, CriticalSectionRawMutex, String, 12>;
pub type PlaylistWatchReceiver = watch::Receiver<'static, CriticalSectionRawMutex, String, 12>;

/// Board config file for a platform's file store. `LedBoardConfigurator`
/// (add/remove networks) is implemented for this.
pub type LedBoardConfigFile<FS> = ConfigFile<LedBoardConfig, LocalFsConfigFileStorage<FS>>;
