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
            AnimationEnum::ScrollingMessage("Message".to_string()),
            AnimationEnum::TwoLineMessage("Line 1".to_string(), "Line 2".to_string()),
            AnimationEnum::Circles,
            AnimationEnum::Intensity,
            AnimationEnum::EchoOfTheSoul,
            AnimationEnum::HarmonicSpiral,
            AnimationEnum::OctopusOfSound,
            AnimationEnum::SilentDancer,
            AnimationEnum::MagneticField,
            AnimationEnum::Fireworks,
            AnimationEnum::SynthWave,
            AnimationEnum::BarChart,
        ]
    }

    pub fn new() -> Self {
        let length_ms = 10_000;

        Self {
            current_index: -1,
            list: vec![
                (
                    Box::new(AnimationEnum::ScrollingMessage(
                        "Welcome to Leigh Hackspace!".to_string(),
                    )),
                    length_ms,
                ),
                (Box::new(AnimationEnum::EchoOfTheSoul), length_ms),
                (Box::new(AnimationEnum::HarmonicSpiral), length_ms),
                (Box::new(AnimationEnum::OctopusOfSound), length_ms),
                (Box::new(AnimationEnum::SilentDancer), length_ms),
                (Box::new(AnimationEnum::MagneticField), length_ms),
                (Box::new(AnimationEnum::Fireworks), length_ms),
                (Box::new(AnimationEnum::SynthWave), length_ms),
                (Box::new(AnimationEnum::BarChart), length_ms),
                (Box::new(AnimationEnum::Intensity), length_ms),
                (Box::new(AnimationEnum::Ship), length_ms),
                (Box::new(AnimationEnum::Circles), length_ms),
                (
                    Box::new(AnimationEnum::TwoLineMessage("Leigh".to_string(), "Hack".to_string())),
                    length_ms,
                ),
                (Box::new(AnimationEnum::Rainbows), length_ms),
                (Box::new(AnimationEnum::SpinningCube), length_ms),
                (Box::new(AnimationEnum::RainbowWheel), length_ms),
            ],
        }
    }

    pub fn update(&mut self, playlist_data: Vec<(AnimationEnum, u32)>) {
        self.current_index = -1;

        self.list.clear();

        for item in playlist_data {
            self.list.push((Box::new(item.0), item.1));
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
