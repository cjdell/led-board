//! The source version baked into this image at build time — see `build.rs`.
//!
//! Surfaced on `BoardStatus` (WS live feed + `GET /api/status`), on
//! `GET /api/version`, in the UI, and in the display worker's boot line,
//! so "what is actually running" never needs bench forensics.

/// Short commit (8 hex chars) the image was compiled from.
pub const GIT_SHA: &str = env!("GIT_SHA");

/// True when the working tree had uncommitted changes at compile time
/// (`build.rs` emits `"true"`/`"false"`; const str comparison isn't stable,
/// so test the first byte).
pub const GIT_DIRTY: bool = env!("GIT_DIRTY").as_bytes()[0] == b't';

/// Commit date (UTC) — orders two builds of the same sha.
pub const GIT_COMMIT_TIME: &str = env!("GIT_COMMIT_TIME");

use crate::types::FirmwareVersion;

/// Snapshot for `GET /api/version`.
impl FirmwareVersion {
    pub fn current() -> Self {
        Self {
            git_sha: GIT_SHA,
            dirty: GIT_DIRTY,
            commit_time: GIT_COMMIT_TIME,
            // led-core's `std` feature is only ever enabled by the simulator.
            runtime: if cfg!(feature = "std") { "sim" } else { "pico" },
        }
    }
}
