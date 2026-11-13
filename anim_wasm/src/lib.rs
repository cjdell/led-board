//! `anim-wasm` — the animations crate built for `wasm32-unknown-unknown`.
//!
//! The web app used to ship hand-written TypeScript ports of every animation
//! painter. Those ports drifted from the firmware renderer, and every new
//! scene meant writing the art twice. This crate compiles the *real*
//! `animations` crate (the exact code the panel runs) into a WebAssembly
//! preview engine, and `web/embed_wasm.ts` base64-inlines the artifact into
//! the single-file SPA bundle. One implementation per animation, in the
//! browser and on the board.
//!
//! No `wasm-bindgen`: the bundle is one gzipped HTML file served by
//! picoserve, so the glue is a handful of `extern "C"` functions and ~100
//! lines of hand-written TypeScript (`web/src/lib/preview.ts`).
//!
//! ABI (bytes are read/written through the instance's exported memory):
//!
//! - `anim_alloc(len) -> ptr` / `anim_free(ptr, len)` — buffers the JS side
//!   fills with an `AnimationEnum` in its board wire format (the JSON the
//!   `Animation` UI type already produces).
//! - `anim_render(json_ptr, json_len, ms_since_start) -> status` — draw one
//!   frame into the static framebuffer. `0` = ok, `1` = JSON rejected (see
//!   `anim_error_*`). The parsed animation is cached, so a preview loop only
//!   re-parses when the JSON string actually changes.
//! - `anim_frame_ptr() -> ptr` — the last frame as raw row-major RGB888
//!   (60×40×3 = 7200 B), byte-identical layout to `GET /api/frame`.
//! - `anim_error_ptr()` / `anim_error_len()` — last rejection message.
//!
//! A panic in a draw becomes a wasm trap, exactly like a panic on the board
//! would be a watchdog reset — the draw functions are panic-free on both
//! sides by contract. `web/src/lib/preview.ts` re-instantiates once on trap
//! and then stops the preview rather than looping on a dead instance.

use animations::{Animation as _, AnimationEnum, AnimationParams, BufferTargetRgb888, TOTAL_PIXELS};
use embedded_graphics_core::pixelcolor::{Rgb888, RgbColor as _};
use std::sync::Mutex;

/// Raw RGB888 size of one frame, matching `led_core::types::PANEL_FRAME`.
pub const FRAME_BYTES: usize = TOTAL_PIXELS * 3;

/// Render status codes returned by [`anim_render`].
pub const STATUS_OK: u32 = 0;
pub const STATUS_BAD_JSON: u32 = 1;

struct Previewer {
    /// Last successfully parsed wire form + instance. Reused verbatim while
    /// the preview loop keeps asking for the same animation (the scenes are
    /// pure functions of `ms_since_start`, like on the board).
    current: Option<(String, AnimationEnum)>,

    frame: BufferTargetRgb888,

    /// Frame packed into raw RGB888 after each draw — the pointer handed to
    /// JS. Same `.r()/.g()/.b()` serialisation as the panel mirror.
    rgb: [u8; FRAME_BYTES],

    error: String,
}

impl Previewer {
    const fn new() -> Self {
        Self {
            current: None,
            // Const equivalent of `BufferTarget::new()`: `Rgb888::new` is a
            // const fn, so the whole static lands in .bss with no lazy-init.
            frame: BufferTargetRgb888 {
                buffer: [Rgb888::new(0, 0, 0); TOTAL_PIXELS],
            },
            rgb: [0; FRAME_BYTES],
            error: String::new(),
        }
    }
}

// A wasm32-unknown-unknown instance is single-threaded, and the std Mutex on
// that target is an uncontended no-op — it exists so the host test binary
// (cargo tests are threaded) shares these entry points without UB.
static PREVIEWER: Mutex<Previewer> = Mutex::new(Previewer::new());

fn with_previewer<R>(f: impl FnOnce(&mut Previewer) -> R) -> R {
    let mut guard = PREVIEWER.lock().unwrap_or_else(|poisoned| poisoned.into_inner());

    f(&mut guard)
}

/// Allocate a byte buffer for [`anim_free`] / [`anim_render`]. `alloc_zeroed`
/// with the exact length, so `anim_free` can always reconstruct the `Vec`
/// with `capacity == len`.
///
/// # Safety
/// `ptr`/`len` must come from a matching [`anim_alloc`] and stay unallocated
/// over in the meantime.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn anim_alloc(len: usize) -> *mut u8 {
    let mut buf = vec![0u8; len];
    let ptr = buf.as_mut_ptr();
    std::mem::forget(buf);

    ptr
}

/// Free a buffer from [`anim_alloc`]. Pass the same `ptr`/`len` pair.
///
/// # Safety
/// See [`anim_alloc`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn anim_free(ptr: *mut u8, len: usize) {
    if !ptr.is_null() {
        drop(unsafe { Vec::from_raw_parts(ptr, len, len) });
    }
}

/// Draw `ms_since_start` of the animation described by the JSON at
/// `json_ptr..json_ptr + json_len` into the frame buffer.
///
/// # Safety
/// `json_ptr..json_ptr + json_len` must be a valid readable byte slice for
/// the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn anim_render(json_ptr: *const u8, json_len: usize, ms_since_start: u32) -> u32 {
    let json = match std::str::from_utf8(unsafe { std::slice::from_raw_parts(json_ptr, json_len) }) {
        Ok(json) => json,
        Err(_) => {
            return with_previewer(|p| {
                p.error = String::from("animation JSON is not valid UTF-8");
                STATUS_BAD_JSON
            });
        }
    };

    with_previewer(|p| {
        if p.current.as_ref().is_none_or(|(cached, _)| cached != json) {
            match serde_json::from_str::<AnimationEnum>(json) {
                Ok(animation) => p.current = Some((json.to_string(), animation)),
                Err(err) => {
                    p.error = err.to_string();

                    return STATUS_BAD_JSON;
                }
            }
        }

        let (_, animation) = p.current.as_mut().expect("just populated above");

        // Same clean-slate semantics as the firmware runner's per-frame
        // `buffer_1.buffer.fill(Rgb888::default())`.
        p.frame.buffer.fill(Default::default());
        animation.draw(ms_since_start, &AnimationParams::default(), &mut p.frame);

        for (i, px) in p.frame.buffer.iter().enumerate() {
            let o = i * 3;

            p.rgb[o] = px.r();
            p.rgb[o + 1] = px.g();
            p.rgb[o + 2] = px.b();
        }

        STATUS_OK
    })
}

/// Pointer to the raw RGB888 bytes of the last rendered frame
/// ([`FRAME_BYTES`] long). Stays valid until the next `anim_render`.
#[unsafe(no_mangle)]
pub extern "C" fn anim_frame_ptr() -> *const u8 {
    with_previewer(|p| p.rgb.as_ptr())
}

/// Pointer/length of the last [`STATUS_BAD_JSON`] rejection message.
#[unsafe(no_mangle)]
pub extern "C" fn anim_error_ptr() -> *const u8 {
    with_previewer(|p| p.error.as_ptr())
}

/// See [`anim_error_ptr`].
#[unsafe(no_mangle)]
pub extern "C" fn anim_error_len() -> usize {
    with_previewer(|p| p.error.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(json: &str, ms: u32) -> u32 {
        let bytes = json.as_bytes();

        unsafe {
            let ptr = anim_alloc(bytes.len());
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len());

            let status = anim_render(ptr, bytes.len(), ms);
            anim_free(ptr, bytes.len());

            status
        }
    }

    fn frame_luma() -> u32 {
        let ptr = anim_frame_ptr();

        unsafe { std::slice::from_raw_parts(ptr, FRAME_BYTES) }
            .iter()
            .map(|&v| v as u32)
            .sum()
    }

    /// Every `AnimationEnum` variant in its board wire format. Kept in sync
    /// by the exhaustiveness test below.
    const ALL_JSON: &[&str] = &[
        "\"Blank\"",
        "{\"TwoLineMessage\":[\"Line 1\",\"Line 2\"]}",
        "{\"ScrollingMessage\":\"Festival Parade\"}",
        "\"Circles\"",
        "\"Fireworks\"",
        "\"SynthWave\"",
        "\"Rainbows\"",
        "\"TestPattern\"",
        "\"RainbowWheel\"",
        "\"SpinningCube\"",
        "\"Ship\"",
        "\"Columns\"",
        "\"Intensity\"",
        "\"BarChart\"",
        "\"LineChart\"",
        "\"EchoOfTheSoul\"",
        "\"HarmonicSpiral\"",
        "\"OctopusOfSound\"",
        "\"MagneticField\"",
        "\"SilentDancer\"",
        "{\"CenteredTitle\":[\"Title\",\"Line\"]}",
        "{\"StatusCard\":[\"Alert\",\"Line\"]}",
        "\"Aurora\"",
        "\"Starfield\"",
        "\"Snowfall\"",
        "\"FallingLeaves\"",
        "\"Fireflies\"",
        "\"VinylSpin\"",
        "\"CanalBoats\"",
        "\"Robin\"",
        "\"Owl\"",
        "\"Deer\"",
        "\"SnowLeopard\"",
        "\"FatherChristmas\"",
        "\"FrostFairies\"",
        "\"SnowGlobe\"",
        "\"Penguins\"",
        "\"Reindeer\"",
        "\"ElectricGuitar\"",
        "\"TrackChampion\"",
        "\"Parrot\"",
        "\"CircusTent\"",
        "\"Elephants\"",
        "\"SpinningWheel\"",
        "\"MillBuildings\"",
    ];

    #[test]
    fn every_variant_renders_without_trapping() {
        for json in ALL_JSON {
            for ms in [0u32, 333, 12_345, 500_000] {
                assert_eq!(render(json, ms), STATUS_OK, "{json} at {ms} ms");
            }
        }
    }

    #[test]
    fn the_frame_actually_changes() {
        assert_eq!(render("\"Aurora\"", 0), STATUS_OK);
        let first = frame_luma();

        assert_eq!(render("\"Aurora\"", 1_700), STATUS_OK);
        let later = frame_luma();

        assert!(first > 0, "aurora should light something up");
        assert_ne!(first, later, "frames at different times should differ");
    }

    #[test]
    fn blank_is_the_dark_guard_navy() {
        assert_eq!(render("\"Blank\"", 10), STATUS_OK);

        let ptr = anim_frame_ptr();
        let rgb = unsafe { std::slice::from_raw_parts(ptr, 3) };

        assert_eq!(rgb, [0x00, 0x00, 0x10]);
    }

    #[test]
    fn garbage_is_rejected_not_rendered() {
        assert_eq!(render("\"NotAnAnimation\"", 0), STATUS_BAD_JSON);
        assert!(anim_error_len() > 0);

        assert_eq!(render("{\"Aurora\":", 0), STATUS_BAD_JSON);
        assert!(anim_error_len() > 0);
    }
}
