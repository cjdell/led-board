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
        ]
    }

    pub fn get_default_playlist_data() -> Vec<(AnimationEnum, u32)> {
        let length_ms = 10_000;

        vec![
            (
                AnimationEnum::ScrollingMessage("Welcome to Leigh Hackspace!".to_string()),
                length_ms,
            ),
            (AnimationEnum::EchoOfTheSoul, length_ms),
            (AnimationEnum::HarmonicSpiral, length_ms),
            (AnimationEnum::OctopusOfSound, length_ms),
            (AnimationEnum::SilentDancer, length_ms),
            (AnimationEnum::MagneticField, length_ms),
            (AnimationEnum::Fireworks, length_ms),
            (AnimationEnum::SynthWave, length_ms),
            (AnimationEnum::BarChart, length_ms),
            (AnimationEnum::Intensity, length_ms),
            (AnimationEnum::Ship, length_ms),
            (AnimationEnum::Circles, length_ms),
            (
                AnimationEnum::TwoLineMessage("Leigh".to_string(), "Hack".to_string()),
                length_ms,
            ),
            (AnimationEnum::Rainbows, length_ms),
            (AnimationEnum::SpinningCube, length_ms),
            (AnimationEnum::RainbowWheel, length_ms),
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
