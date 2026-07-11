use crate::{animations::*, common::Animation};
use alloc::{boxed::Box, string::ToString};

pub struct Playlist {
    current_index: i32,
}

impl Playlist {
    pub fn new() -> Self {
        Self { current_index: -1 }
    }

    pub fn get_next_animation(&mut self) -> Box<dyn Animation> {
        self.current_index = (self.current_index + 1) % 6;

        match self.current_index {
            0 => Box::new(Blank {}),
            1 => Box::new(Circles {}),
            2 => Box::new(Rainbows {}),
            3 => Box::new(SpinningCube {}),
            4 => Box::new(RainbowWheel {}),
            5 => Box::new(ScrollingMessage {
                msg: "Welcome to Leigh Hackspace!".to_string(),
            }),
            _ => Box::new(Blank {}),
        }
    }
}
