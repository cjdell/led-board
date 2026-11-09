use crate::AnimationEnum::ScrollingMessage;
use crate::{animations::*, common::Animation};
use alloc::vec;
use alloc::{boxed::Box, string::ToString, vec::Vec};

pub struct Playlist {
    current_index: i32,
    list: Vec<(Box<dyn Animation>, u32)>,
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

    /// Leigh Lights parade loop (Nov 2026): four lantern sections, each led by
    /// a gold title card, framed by the "Light Up Leigh" scroll. One loop ≈ 4
    /// minutes. Hardcoded so the board boots straight into it with no playlist
    /// file on flash (delete `playlist_default.json` / "Reset" to reach it).
    pub fn get_default_playlist_data() -> Vec<(AnimationEnum, u32)> {
        vec![
            // ── Banner ────────────────────────────────────────────────────
            (
                AnimationEnum::ScrollingMessage("Light Up Leigh with Leigh Hackspace".to_string()),
                14_000,
            ),
            // ── Section 1: Natures Magic (robins, leaves, owls, deer) ─────
            (
                AnimationEnum::CenteredTitle("Natures".to_string(), "Magic".to_string()),
                5_000,
            ),
            (AnimationEnum::FallingLeaves, 15_000),
            (AnimationEnum::Fireflies, 16_000),
            (AnimationEnum::EchoOfTheSoul, 12_000),
            // ── Section 2: Awe and Wonder (stars, snow, frost fairies) ────
            (
                AnimationEnum::CenteredTitle("Awe and".to_string(), "Wonder".to_string()),
                5_000,
            ),
            (AnimationEnum::Starfield, 14_000),
            (AnimationEnum::Snowfall, 16_000),
            (AnimationEnum::Fireworks, 12_000),
            (AnimationEnum::Rainbows, 12_000),
            // ── Section 3: Northern Lights (aurora, music, running) ───────
            (
                AnimationEnum::CenteredTitle("Northern".to_string(), "Lights".to_string()),
                5_000,
            ),
            (AnimationEnum::Aurora, 20_000),
            (AnimationEnum::VinylSpin, 12_000),
            (AnimationEnum::SynthWave, 12_000),
            (AnimationEnum::SilentDancer, 12_000),
            // ── Section 4: Heritage in Motion (canal, circus, mills) ──────
            (
                AnimationEnum::CenteredTitle("Heritage".to_string(), "in Motion".to_string()),
                5_000,
            ),
            (AnimationEnum::CanalBoats, 16_000),
            (AnimationEnum::SpinningCube, 12_000),
            (AnimationEnum::Circles, 12_000),
            // ── Close ─────────────────────────────────────────────────────
            (
                AnimationEnum::CenteredTitle("Leigh".to_string(), "Hackspace".to_string()),
                6_000,
            ),
            (
                AnimationEnum::ScrollingMessage("Light Up Leigh with Leigh Hackspace".to_string()),
                14_000,
            ),
        ]
    }

    pub fn new(playlist_data: Vec<(AnimationEnum, u32)>) -> Self {
        let mut list = Vec::<(Box<dyn Animation>, u32)>::new();

        for item in playlist_data {
            list.push((Box::new(item.0), item.1));
        }

        Self {
            current_index: -1,
            list,
        }
    }

    pub fn get_next_animation(&mut self) -> (Box<dyn Animation>, u32) {
        self.current_index = if self.list.len() as i32 > self.current_index + 1 {
            self.current_index + 1
        } else {
            0
        };

        self.list[self.current_index as usize].clone()
    }
}
