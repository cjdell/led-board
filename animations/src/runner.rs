use crate::common::{Animation, BufferTarget, BufferTargetRgb888};
use crate::fader::AnimationFader;
use crate::playlist::Playlist;
use alloc::boxed::Box;
use embedded_3dgfx::animation;
use embedded_graphics::pixelcolor::Rgb888;

type Buffer = BufferTargetRgb888;

pub struct AnimationRunner {
    playlist: Playlist,
    fader: AnimationFader,

    current_animation: (Box<dyn Animation>, u32, u32),
    override_animation: Option<(Box<dyn Animation>, u32, u32)>,
}

impl AnimationRunner {
    pub fn new() -> Self {
        let mut playlist = Playlist::new();
        let animation = playlist.get_next_animation();
        let fader = AnimationFader::new(animation.clone());

        Self {
            playlist,
            fader,
            current_animation: (animation, 0, 2_000),
            override_animation: None,
        }
    }

    pub fn next(&mut self) {
        if self.override_animation.is_some() {
            return;
        }

        self.current_animation = (self.playlist.get_next_animation(), 0, 10_000);
        self.fader.switch_to(self.current_animation.0.clone(), 0, 1.0);
    }

    pub fn set_override_animation(&mut self, animation: Box<dyn Animation>, run_time: u32) {
        self.override_animation = Some((animation.clone(), 0, run_time));
        self.fader.switch_to(animation, 0, 0.95);
    }

    fn restore_animation(&mut self) {
        self.override_animation = None;
        self.fader
            .switch_to(self.current_animation.0.clone(), self.current_animation.1, 1.0);
    }

    pub fn update(&mut self, delta_ms: u32, buffer_1: &mut Buffer, buffer_2: &mut Buffer) {
        if let Some(ref mut override_animation) = self.override_animation {
            override_animation.1 += delta_ms;

            if override_animation.1 > override_animation.2 {
                self.current_animation.1 += override_animation.2;
                self.restore_animation();
            }
        } else {
            self.current_animation.1 += delta_ms;

            if self.current_animation.1 > self.current_animation.2 {
                self.next();
            }
        }

        buffer_1.buffer.fill(Rgb888::default());
        buffer_2.buffer.fill(Rgb888::default());

        self.fader.update(delta_ms, buffer_1, buffer_2);
    }
}
