use crate::common::{Animation, BufferTarget, BufferTargetRgb888};
use crate::fader::AnimationFader;
use crate::playlist::Playlist;
use crate::{AnimationEnum, AnimationParams};
use alloc::boxed::Box;
use alloc::vec::Vec;
use embedded_3dgfx::animation;
use embedded_graphics::pixelcolor::Rgb888;
use ringbuf::HeapRb;
use ringbuf::traits::{Consumer as _, Observer as _, Producer as _};

type Buffer = BufferTargetRgb888;

pub struct AnimationRunner {
    playlist: Playlist,
    fader: AnimationFader,

    current_animation: (u32, u32),
    override_animation: Option<(u32, u32)>,
    suspend: u32,

    params_buffer: HeapRb<AnimationParams>,
    dropped: i32,
    rem: u32,
}

impl AnimationRunner {
    pub fn new() -> Self {
        let mut playlist = Playlist::new();
        let animation = playlist.get_next_animation();
        let fader = AnimationFader::new(animation.0.clone());
        let params_buffer = HeapRb::<AnimationParams>::new(200);

        Self {
            playlist,
            fader,
            current_animation: (0, animation.1),
            override_animation: None,
            suspend: 0,
            params_buffer,
            dropped: 0,
            rem: 0,
        }
    }

    pub fn stats(&self) -> (i32, usize, usize) {
        (
            self.dropped,
            self.params_buffer.read_index(),
            self.params_buffer.write_index(),
        )
    }

    pub fn next(&mut self) {
        let next_animation = self.playlist.get_next_animation();

        #[cfg(feature = "defmt")]
        defmt::info!("Next Animation");

        self.current_animation = (0, next_animation.1);
        self.fader.switch_to(next_animation.0);
    }

    pub fn set_override_animation(&mut self, animation: Box<dyn Animation>, run_time: u32) {
        self.override_animation = Some((0, run_time));
        self.fader.set_override_animation(animation);
    }

    pub fn update_playlist(&mut self, playlist_data: Vec<(AnimationEnum, u32)>) {
        self.playlist.update(playlist_data);
        self.next();
    }

    pub fn push_params(&mut self, params: AnimationParams) {
        if self.params_buffer.is_full() {
            self.params_buffer.skip(1);
            self.dropped += 1;
        }

        self.params_buffer.try_push(params).unwrap();
    }

    pub fn set_suspend(&mut self, ms: u32) {
        self.suspend = ms;
    }

    pub fn update(&mut self, delta_ms: u32, buffer_1: &mut Buffer, buffer_2: &mut Buffer) {
        if self.suspend > 0 {
            self.suspend = self.suspend.saturating_sub(delta_ms);

            #[cfg(feature = "std")]
            std::println!("Suspending: {}", self.suspend);

            return;
        }

        if let Some(ref mut override_animation) = self.override_animation {
            override_animation.0 += delta_ms;

            if override_animation.0 > override_animation.1 {
                self.override_animation = None;
                self.fader.clear_override_animation();
            }
        } else {
            self.current_animation.0 += delta_ms;

            if self.current_animation.0 > self.current_animation.1 {
                self.next();
            }
        }

        buffer_1.buffer.fill(Rgb888::default());
        buffer_2.buffer.fill(Rgb888::default());

        let mut seek = self.rem + delta_ms;
        let mut params = AnimationParams::default();

        seek = ((seek as i32) + self.dropped / 16).max(0) as u32;

        if let Some(p) = self.params_buffer.try_peek() {
            params = p.clone();
        }

        for _ in 0..(seek / 10) {
            if self.params_buffer.skip(1) == 0 {
                self.dropped -= 1;
            }
        }

        // #[cfg(feature = "std")]
        // std::println!("{}", params.beat);

        self.rem = seek % 10;

        self.fader.update(delta_ms, &params, buffer_1, buffer_2);
    }
}
