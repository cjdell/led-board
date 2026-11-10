use crate::AnimationEnum::ScrollingMessage;
use crate::{animations::*, common::Animation};
use alloc::vec;
use alloc::{boxed::Box, string::String, string::ToString as _, vec::Vec};
use serde::{Deserialize, Serialize};

/// Bounds every playlist row is clamped into — long enough for a section of
/// aurora, short enough that one typo cannot freeze the board for a day.
pub const MIN_DURATION_MS: u32 = 500;
pub const MAX_DURATION_MS: u32 = 15 * 60 * 1_000;

/// One playlist row: an animation, how long it runs, and an optional label
/// (shown in the UI and the "now playing" title slot instead of the variant
/// name — e.g. "Section 2: Awe and Wonder").
///
/// Deserialisation is backwards compatible: the pre-v2 wire/flash form
/// `[<animation>, <duration_ms>]` still parses, so playlists saved by older
/// firmware and the `ws_client` examples load unchanged. New serialisation
/// is the self-describing object form.
#[derive(Clone, Debug, Serialize)]
pub struct PlaylistItem {
    pub animation: AnimationEnum,
    pub duration_ms: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ItemWire {
    /// Legacy tuple: `["Intensity", 10000]` / `[{"Aurora": null}...]`.
    Legacy((AnimationEnum, u32)),
    Modern {
        animation: AnimationEnum,
        #[serde(alias = "durationMs")]
        duration_ms: u32,
        #[serde(default)]
        label: Option<String>,
    },
}

impl<'de> Deserialize<'de> for PlaylistItem {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Ok(match ItemWire::deserialize(deserializer)? {
            ItemWire::Legacy((animation, duration_ms)) => PlaylistItem::new(animation, duration_ms),
            ItemWire::Modern { animation, duration_ms, label } => PlaylistItem {
                animation,
                duration_ms: duration_ms.clamp(MIN_DURATION_MS, MAX_DURATION_MS),
                label: label.filter(|l| !l.is_empty()),
            },
        })
    }
}

impl PlaylistItem {
    pub fn new(animation: AnimationEnum, duration_ms: u32) -> Self {
        Self {
            animation,
            duration_ms: duration_ms.clamp(MIN_DURATION_MS, MAX_DURATION_MS),
            label: None,
        }
    }

    pub fn with_label(mut self, label: &str) -> Self {
        self.label = Some(String::from(label));
        self
    }

    /// Variant name of the underlying animation.
    pub fn kind(&self) -> &'static str {
        self.animation.kind_name()
    }

    /// Label if set, else the animation's own display title.
    pub fn title(&self) -> String {
        match &self.label {
            Some(label) => label.clone(),
            None => self.animation.display_title(),
        }
    }
}

pub struct Playlist {
    current_index: i32,
    list: Vec<(Box<dyn Animation>, PlaylistItem)>,
}

impl Playlist {
    pub fn get_available() -> Vec<AnimationEnum> {
        vec![
            AnimationEnum::Blank,
            AnimationEnum::TwoLineMessage("Line 1".to_string(), "Line 2".to_string()),
            AnimationEnum::ScrollingMessage("Message".to_string()),
            AnimationEnum::Circles,
            AnimationEnum::Fireworks,
            AnimationEnum::SynthWave,
            AnimationEnum::Rainbows,
            AnimationEnum::TestPattern,
            AnimationEnum::RainbowWheel,
            AnimationEnum::SpinningCube,
            AnimationEnum::Ship,
            AnimationEnum::Columns,
            AnimationEnum::Intensity,
            AnimationEnum::BarChart,
            AnimationEnum::LineChart,
            AnimationEnum::EchoOfTheSoul,
            AnimationEnum::HarmonicSpiral,
            AnimationEnum::OctopusOfSound,
            AnimationEnum::MagneticField,
            AnimationEnum::SilentDancer,
            AnimationEnum::CenteredTitle("Title".to_string(), "Line".to_string()),
            AnimationEnum::StatusCard("Alert".to_string(), "Line".to_string()),
            AnimationEnum::Aurora,
            AnimationEnum::Starfield,
            AnimationEnum::Snowfall,
            AnimationEnum::FallingLeaves,
            AnimationEnum::Fireflies,
            AnimationEnum::VinylSpin,
            AnimationEnum::CanalBoats,
        ]
    }

    /// Leigh Lights parade loop (Nov 2026): festival animations ONLY — the
    /// legacy effects (Fireworks, Rainbows, SynthWave, SilentDancer,
    /// SpinningCube, Circles, EchoOfTheSoul) are deliberately absent from the
    /// startup loop so the board opens on the parade programme. The old
    /// effects remain available in `/api/info/animations` for manual use.
    ///
    /// Four lantern sections, each led by a gold title card, framed by the
    /// "Light Up Leigh" scroll. One loop ≈ 230 s. Hardcoded so the board boots
    /// straight into it with no playlist file on flash (delete
    /// `playlist_default.json` / "Reset" to reach it).
    pub fn get_default_playlist_data() -> Vec<PlaylistItem> {
        vec![
            // ── Banner ────────────────────────────────────────────────────
            PlaylistItem::new(
                AnimationEnum::ScrollingMessage("Light Up Leigh with Leigh Hackspace".to_string()),
                14_000,
            )
            .with_label("Banner"),
            // ── Section 1: Natures Magic ─────────────────────────────────
            PlaylistItem::new(
                AnimationEnum::CenteredTitle("Natures".to_string(), "Magic".to_string()),
                5_000,
            ),
            PlaylistItem::new(AnimationEnum::FallingLeaves, 18_000).with_label("Section 1 · Leaves"),
            PlaylistItem::new(AnimationEnum::Fireflies, 20_000).with_label("Section 1 · Fireflies"),
            // ── Section 2: Awe and Wonder ────────────────────────────────
            PlaylistItem::new(
                AnimationEnum::CenteredTitle("Awe and".to_string(), "Wonder".to_string()),
                5_000,
            ),
            PlaylistItem::new(AnimationEnum::Starfield, 18_000).with_label("Section 2 · Stars"),
            PlaylistItem::new(AnimationEnum::Snowfall, 20_000).with_label("Section 2 · Snow"),
            // ── Section 3: Northern Lights ───────────────────────────────
            PlaylistItem::new(
                AnimationEnum::CenteredTitle("Northern".to_string(), "Lights".to_string()),
                5_000,
            ),
            PlaylistItem::new(AnimationEnum::Aurora, 30_000).with_label("Section 3 · Aurora"),
            PlaylistItem::new(AnimationEnum::Starfield, 14_000),
            PlaylistItem::new(AnimationEnum::VinylSpin, 16_000),
            // ── Section 4: Heritage in Motion ────────────────────────────
            PlaylistItem::new(
                AnimationEnum::CenteredTitle("Heritage".to_string(), "in Motion".to_string()),
                5_000,
            ),
            PlaylistItem::new(AnimationEnum::CanalBoats, 24_000).with_label("Section 4 · Canal"),
            PlaylistItem::new(AnimationEnum::Fireflies, 16_000),
            // ── Close ─────────────────────────────────────────────────────
            PlaylistItem::new(
                AnimationEnum::CenteredTitle("Leigh".to_string(), "Hackspace".to_string()),
                6_000,
            ),
            PlaylistItem::new(
                AnimationEnum::ScrollingMessage("Light Up Leigh with Leigh Hackspace".to_string()),
                14_000,
            )
            .with_label("Banner"),
        ]
    }

    pub fn new(playlist_data: Vec<PlaylistItem>) -> Self {
        let mut list = Vec::<(Box<dyn Animation>, PlaylistItem)>::new();

        for item in playlist_data {
            list.push((Box::new(item.animation.clone()), item));
        }

        Self {
            current_index: -1,
            list,
        }
    }

    pub fn len(&self) -> usize {
        self.list.len()
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    /// 0-based index of the item the playlist is on (before the first
    /// `advance` this reports 0 — the item that is *about to* start).
    pub fn current_index(&self) -> usize {
        self.current_index.max(0) as usize
    }

    pub fn item(&self, index: usize) -> Option<&PlaylistItem> {
        self.list.get(index).map(|(_, item)| item)
    }

    /// The whole list as plain data — what the status API hands the UI.
    pub fn data(&self) -> Vec<PlaylistItem> {
        self.list.iter().map(|(_, item)| item.clone()).collect()
    }

    /// Point the cursor *at* `index` (so the next start plays it). Returns
    /// false when the index is out of range.
    pub fn set_current(&mut self, index: usize) -> bool {
        if index < self.list.len() {
            self.current_index = index as i32;
            true
        } else {
            false
        }
    }

    /// Move to the next item (wrapping) and hand back what to play. Cloning
    /// the boxed animation gives every play-through a fresh renderer, which
    /// is what makes transitions deterministic.
    pub fn advance(&mut self) -> Option<(Box<dyn Animation>, PlaylistItem)> {
        if self.list.is_empty() {
            return None;
        }

        self.current_index = if self.list.len() as i32 > self.current_index + 1 {
            self.current_index + 1
        } else {
            0
        };

        let (animation, item) = &self.list[self.current_index as usize];

        Some((animation.clone(), item.clone()))
    }

    /// Hand back the item the cursor currently sits on (used after
    /// `set_current`).
    pub fn restart_current(&mut self) -> Option<(Box<dyn Animation>, PlaylistItem)> {
        let index = self.current_index.max(0) as usize;

        self.list.get(index).map(|(animation, item)| (animation.clone(), item.clone()))
    }
}
