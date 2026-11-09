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
    pub fn get_default_playlist_data() -> Vec<(AnimationEnum, u32)> {
        vec![
            // ── Banner ────────────────────────────────────────────────────
            (
                AnimationEnum::ScrollingMessage("Light Up Leigh with Leigh Hackspace".to_string()),
                14_000,
            ),
            // ── Section 1: Natures Magic ─────────────────────────────────
            (
                AnimationEnum::CenteredTitle("Natures".to_string(), "Magic".to_string()),
                5_000,
            ),
            (AnimationEnum::FallingLeaves, 18_000),
            (AnimationEnum::Fireflies, 20_000),
            // ── Section 2: Awe and Wonder ────────────────────────────────
            (
                AnimationEnum::CenteredTitle("Awe and".to_string(), "Wonder".to_string()),
                5_000,
            ),
            (AnimationEnum::Starfield, 18_000),
            (AnimationEnum::Snowfall, 20_000),
            // ── Section 3: Northern Lights ───────────────────────────────
            (
                AnimationEnum::CenteredTitle("Northern".to_string(), "Lights".to_string()),
                5_000,
            ),
            (AnimationEnum::Aurora, 30_000),
            (AnimationEnum::Starfield, 14_000),
            (AnimationEnum::VinylSpin, 16_000),
            // ── Section 4: Heritage in Motion ────────────────────────────
            (
                AnimationEnum::CenteredTitle("Heritage".to_string(), "in Motion".to_string()),
                5_000,
            ),
            (AnimationEnum::CanalBoats, 24_000),
            (AnimationEnum::Fireflies, 16_000),
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
