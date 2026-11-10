use crate::common::{Animation, BufferTarget, BufferTargetRgb888};
use crate::fader::AnimationFader;
use crate::playlist::{Playlist, PlaylistItem};
use crate::AnimationParams;
use alloc::boxed::Box;
use embedded_graphics::pixelcolor::Rgb888;
use ringbuf::HeapRb;
use ringbuf::traits::{Consumer as _, Observer as _, Producer as _};

type Buffer = BufferTargetRgb888;

/// Everything the status API needs about the playlist slot the runner is on.
#[derive(Clone, Debug)]
pub struct RunnerStatus {
    pub index: usize,
    pub len: usize,
    pub item: Option<PlaylistItem>,
    pub elapsed_ms: u32,
    pub duration_ms: u32,
}

pub struct AnimationRunner {
    playlist: Playlist,
    fader: AnimationFader,

    current_animation: (u32, u32),
    override_animation: Option<(u32, u32)>,
    suspend: u32,
    paused: bool,

    params_buffer: HeapRb<AnimationParams>,
    dropped: i32,
    rem: u32,
}

impl AnimationRunner {
    pub fn new(mut playlist: Playlist) -> Self {
        let animation = playlist.advance().expect("empty playlist");
        let fader = AnimationFader::new(animation.0.clone());
        let params_buffer = HeapRb::<AnimationParams>::new(200);

        Self {
            playlist,
            fader,
            current_animation: (0, animation.1.duration_ms),
            override_animation: None,
            suspend: 0,
            paused: false,
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
        // A user-initiated change ends any running preview override so the
        // new item is actually visible (the override fades out).
        self.clear_override_animation();

        let Some(next_animation) = self.playlist.advance() else {
            return;
        };

        #[cfg(feature = "defmt")]
        defmt::info!("Next Animation");

        self.current_animation = (0, next_animation.1.duration_ms);
        self.fader.switch_to(next_animation.0);
    }

    /// Jump straight to a playlist slot (the UI's "play now"). Out-of-range
    /// indices are ignored; returns whether the jump happened.
    pub fn jump_to(&mut self, index: usize) -> bool {
        if !self.playlist.set_current(index) {
            return false;
        }

        let Some(item) = self.playlist.restart_current() else {
            return false;
        };

        self.clear_override_animation();
        self.current_animation = (0, item.1.duration_ms);
        self.fader.switch_to(item.0);

        true
    }

    pub fn set_override_animation(&mut self, animation: Box<dyn Animation>, run_time: u32) {
        self.override_animation = Some((0, run_time));
        self.fader.set_override_animation(animation);
    }

    pub fn clear_override_animation(&mut self) {
        if self.override_animation.take().is_some() {
            self.fader.clear_override_animation();
        }
    }

    pub fn override_active(&self) -> bool {
        self.override_animation.is_some()
    }

    /// Elapsed/total of a running override animation (preview), if any.
    pub fn override_progress(&self) -> Option<(u32, u32)> {
        self.override_animation
    }

    pub fn update_playlist(&mut self, playlist: Playlist) {
        self.clear_override_animation();
        self.playlist = playlist;
        self.next();
    }

    pub fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
    }

    pub fn is_paused(&self) -> bool {
        self.paused
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

    pub fn clear_suspend(&mut self) {
        self.suspend = 0;
    }

    /// Playlist length / cursor / current item for the status feed.
    pub fn status(&self) -> RunnerStatus {
        let index = self.playlist.current_index();

        RunnerStatus {
            index,
            len: self.playlist.len(),
            item: self.playlist.item(index).cloned(),
            elapsed_ms: self.current_animation.0,
            duration_ms: self.current_animation.1,
        }
    }

    pub fn playlist(&self) -> &Playlist {
        &self.playlist
    }

    pub fn update(&mut self, delta_ms: u32, buffer_1: &mut Buffer, buffer_2: &mut Buffer) {
        // Paused (user) or suspended (a frame stream owns the panel): hold
        // the last rendered frame — nothing advances, nothing is drawn.
        if self.paused || self.suspend > 0 {
            if self.suspend > 0 {
                self.suspend = self.suspend.saturating_sub(delta_ms);

                #[cfg(feature = "std")]
                std::println!("Suspending: {}", self.suspend);
            }

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
