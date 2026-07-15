use crate::common::{Animation, AnimationParams, BufferTargetRgb888};
use alloc::boxed::Box;
use embedded_graphics::pixelcolor::{Rgb888, RgbColor as _};
use tupl::NonEmptyTuple as _;

// New struct to hold the fading state
pub struct AnimationFader {
    current_animation: (Box<dyn Animation>, u32),

    previous_animation: Option<(Box<dyn Animation>, u32)>,
    fade_progress: f32,    // 0.0 = old, 1.0 = new
    fade_duration_ms: u64, // Duration of the fade transition

    override_animation: Option<(Box<dyn Animation>, u32)>,
    override_fade_progress: f32,
    override_active: bool,
}

impl AnimationFader {
    pub fn new(animation: Box<dyn Animation>) -> Self {
        Self {
            current_animation: (animation, 0),

            previous_animation: None,
            fade_progress: 1.0,
            fade_duration_ms: 1_000,

            override_animation: None,
            override_fade_progress: 0.0,
            override_active: false,
        }
    }

    pub fn switch_to(&mut self, animation: Box<dyn Animation>) {
        self.previous_animation = Some(self.current_animation.clone());
        self.current_animation = (animation, 0);
        self.fade_progress = 0.0;
    }

    pub fn set_override_animation(&mut self, animation: Box<dyn Animation>) {
        self.override_animation = Some((animation, 0));
        self.override_fade_progress = 0.0;
        self.override_active = true;
    }

    pub fn clear_override_animation(&mut self) {
        self.override_active = false;
    }

    pub fn update(
        &mut self,
        delta_ms: u32,
        params: &AnimationParams,
        buffer_1: &mut BufferTargetRgb888,
        buffer_2: &mut BufferTargetRgb888,
    ) {
        if let Some(ref mut previous_animation) = self.previous_animation {
            previous_animation.1 += delta_ms;

            self.fade_progress += delta_ms as f32 / self.fade_duration_ms as f32;

            if self.fade_progress >= 1.0 {
                self.fade_progress = 1.0;
                self.previous_animation = None;
            }
        }

        if let Some(ref mut override_animation) = self.override_animation {
            override_animation.1 += delta_ms;

            if self.override_active {
                self.override_fade_progress += delta_ms as f32 / self.fade_duration_ms as f32;

                if self.override_fade_progress >= 1.0 {
                    self.override_fade_progress = 1.0;
                }
            } else {
                self.override_fade_progress -= delta_ms as f32 / self.fade_duration_ms as f32;

                if self.override_fade_progress <= 0.0 {
                    self.override_fade_progress = 0.0;

                    self.override_animation = None;
                }
            }
        }

        self.current_animation.1 += delta_ms;

        // Draw current animation into old_buffer
        self.current_animation
            .0
            .draw(self.current_animation.1, &params, buffer_1);

        // Draw previous animation into target (for fading)
        if let Some(ref mut prev) = self.previous_animation {
            prev.0.draw(prev.1, &params, buffer_2);

            apply_fade(buffer_1, self.fade_progress, buffer_2, 1.0 - self.fade_progress);
        }

        if let Some(ref mut override_animation) = self.override_animation {
            override_animation.0.draw(override_animation.1, &params, buffer_2);

            apply_fade(
                buffer_1,
                1.0 - self.override_fade_progress,
                buffer_2,
                self.override_fade_progress,
            );
        }
    }
}

fn apply_fade(buffer_1: &mut BufferTargetRgb888, mix_1: f32, buffer_2: &BufferTargetRgb888, mix_2: f32) {
    // Blend pixels based on fade_progress
    for (new_pixel, old_pixel) in buffer_1.buffer.iter_mut().zip(buffer_2.buffer.iter()) {
        *new_pixel = Rgb888::new(
            (old_pixel.r() as f32 * mix_2 + new_pixel.r() as f32 * mix_1) as u8,
            (old_pixel.g() as f32 * mix_2 + new_pixel.g() as f32 * mix_1) as u8,
            (old_pixel.b() as f32 * mix_2 + new_pixel.b() as f32 * mix_1) as u8,
        );
    }
}
