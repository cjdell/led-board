//! The display worker: runs the animation playlist into the screen and
//! publishes the board's live state.
//!
//! On the Pico this is the core1 task; on the simulator it is a task on the
//! host executor thread. The loop body is identical — frames land on the
//! screen through [`Ws2812ParallelDriver`].
//!
//! The worker is also the *source of truth* for what the panel is showing:
//! every second (and on any change) it serialises a [`BoardStatus`] onto the
//! status watch and, when the playlist itself changes, a [`PlaylistInfo`]
//! onto the playlist watch. The web app turns those into WebSocket pushes
//! and the `GET /api/status` / `GET /api/info/playlist` answers.

use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use animations::{
    Animation, AnimationEnum, AnimationParams, AnimationRunner, BufferTarget, Playlist, apply_power_limit,
};
use embedded_graphics::pixelcolor::{Rgb888, RgbColor};

use crate::info;
use crate::led_screen::{LedScreen, PIXELS_PER_STRIP, Ws2812ParallelDriver};
use crate::types::{
    BoardStatus, DisplayWorkerMessage, DisplayWorkerReceiver, PANEL_FRAME, Playing, PlaylistInfo, PlaylistSource,
    PlaylistWatchSender, StatusWatchSender,
};

/// Web-mirror cadence: enough for a 2 fps browser poll to look live, small
/// enough that the copy is noise against the frame loop.
const MIRROR_INTERVAL: embassy_time::Duration = embassy_time::Duration::from_millis(250);

/// Publish the frame about to go to the panel onto [`PANEL_FRAME`] for
/// `GET /api/frame`. Call before `apply_power_limit` (the mirror is the
/// authored sRGB, which the panel's gamma stage reproduces). Skips the
/// publish when a poller holds the lock mid-response — never blocks.
fn mirror_frame(buffer: &BufferTarget<Rgb888>, last: &mut embassy_time::Instant) {
    if last.elapsed() < MIRROR_INTERVAL {
        return;
    }

    if let Ok(mut slot) = PANEL_FRAME.try_lock() {
        slot.seq = slot.seq.wrapping_add(1);

        for (i, px) in buffer.buffer.iter().enumerate() {
            let o = i * 3;

            slot.rgb[o] = px.r();
            slot.rgb[o + 1] = px.g();
            slot.rgb[o + 2] = px.b();
        }

        *last = embassy_time::Instant::now();
    }
}

/// True when a timed status card has outlived its timeout. `timeout_ms == 0`
/// means "no timeout" (the original `Status` message); anything else clears.
pub fn status_due(elapsed_ms: u32, timeout_ms: u32) -> bool {
    timeout_ms > 0 && elapsed_ms >= timeout_ms
}

pub async fn run<D: Ws2812ParallelDriver<PIXELS_PER_STRIP>>(
    receiver: DisplayWorkerReceiver,
    mut led_screen: LedScreen<D>,
    status_tx: StatusWatchSender,
    playlist_tx: PlaylistWatchSender,
) -> ! {
    info!(
        "display worker, firmware {}{}",
        crate::build_info::GIT_SHA,
        if crate::build_info::GIT_DIRTY { "+dirty" } else { "" }
    );

    led_screen.copy_eg_buffer(&[Rgb888::new(0x10, 0x10, 0x0); crate::led_screen::TOTAL_PIXELS]);
    led_screen.flush().await;
    info!("display: first frame flushed (self-test yellow)");

    let mut buffer_1 = BufferTarget::new();
    let mut buffer_2 = BufferTarget::new();

    // Power-on splash: the firmware identity readable across the room with
    // no console and no browser — commit sha (`*` = dirty tree) over the
    // commit date — then the WiFi join cards take the panel as usual. The
    // 6x12 font fits 10 chars, hence sha + `*` and the date's `T…` cut off.
    let sha_label = if crate::build_info::GIT_DIRTY {
        format!("{}*", crate::build_info::GIT_SHA)
    } else {
        String::from(crate::build_info::GIT_SHA)
    };
    let mut splash = AnimationEnum::CenteredTitle(
        sha_label,
        String::from(crate::build_info::GIT_COMMIT_TIME.split('T').next().unwrap_or("?")),
    );

    let splash_start = embassy_time::Instant::now();
    let mut last_mirror = splash_start;
    while splash_start.elapsed() < embassy_time::Duration::from_millis(1_800) {
        buffer_1.buffer.fill(Rgb888::new(0x00, 0x00, 0x10));
        splash.draw(
            splash_start.elapsed().as_millis() as u32,
            &AnimationParams::default(),
            &mut buffer_1,
        );
        mirror_frame(&buffer_1, &mut last_mirror);
        apply_power_limit(&mut buffer_1, 0.1);
        led_screen.copy_eg_buffer(&buffer_1.buffer);
        led_screen.flush().await;
    }

    let mut runner = AnimationRunner::new(Playlist::new(Playlist::get_default_playlist_data()));
    let mut playlist_source = PlaylistSource::Default;
    let mut last_time = embassy_time::Instant::now();
    let started = last_time;

    // Frame ledger: a stall, a dropped frame rate or a dead worker is visible
    // in the log stream instead of having to be inferred from the panel.
    let mut frames: u32 = 0;
    let mut window_start = last_time;
    // Phase timing (µs accumulated): "draw" is the animation render,
    // "panel" is screen scatter + transpose + the wait for the previous
    // frame's wire time. With the pipelined driver, panel ≈ wire − draw
    // means the panel transfer is the clock (~9 ms, ~110 fps); if draw is
    // the bigger share, animation CPU is the clock.
    let mut draw_us: u64 = 0;
    let mut panel_us: u64 = 0;

    let mut power_limit = 0.1f32;
    // Status-screen mode: when set, the animation is drawn over the still
    // advancing playlist until StatusClear or its timeout (see
    // DisplayWorkerMessage::StatusTimed).
    let mut status_screen: Option<(AnimationEnum, u32, u32)> = None;
    // The preview override's enum value (for now-playing metadata — the
    // runner holds the type-erased box, which cannot describe itself).
    let mut override_meta: Option<(AnimationEnum, u32)> = None;
    // The paint stream (Grid page) owns the panel until its lease expires.
    let mut streaming_until: Option<embassy_time::Instant> = None;

    // Status-feed bookkeeping: publish when a second has passed (progress
    // bars need the clock) or when anything the UI reads has flipped.
    let mut last_publish = started;
    let mut dirty = true;
    let mut last_index = usize::MAX;
    let mut last_override = false;
    let mut last_paused = false;
    let mut last_streaming = false;
    let mut last_status_screen = false;
    let mut last_power = f32::NAN;

    publish_playlist(&playlist_tx, &runner, playlist_source);

    loop {
        let now = embassy_time::Instant::now();
        let time_since_last_frame = now.duration_since(last_time);
        let delta_ms = time_since_last_frame.as_millis() as u32;

        last_time = now;

        if let Ok(msg) = receiver.try_receive() {
            match msg {
                DisplayWorkerMessage::Next => {
                    info!("Next");
                    runner.next();
                    dirty = true;
                }
                DisplayWorkerMessage::Animation(animation, length_ms) => {
                    info!("Override Animation");
                    override_meta = Some((animation.clone(), length_ms));
                    runner.set_override_animation(Box::new(animation), length_ms);
                    dirty = true;
                }
                DisplayWorkerMessage::Playlist { items, source } => {
                    if items.is_empty() {
                        info!(
                            "Update playlist: built-in default ({} items)",
                            Playlist::get_default_playlist_data().len()
                        );
                        let fallback = Playlist::get_default_playlist_data();
                        runner.update_playlist(Playlist::new(fallback));
                        playlist_source = PlaylistSource::Default;
                    } else {
                        info!("Update playlist ({} items, {})", items.len(), source.label());
                        runner.update_playlist(Playlist::new(items));
                        playlist_source = source;
                    }

                    publish_playlist(&playlist_tx, &runner, playlist_source);
                    dirty = true;
                }
                DisplayWorkerMessage::PowerLimit(new_power_limit) => {
                    power_limit = new_power_limit;
                    dirty = true;
                }
                DisplayWorkerMessage::ParamsBuffer(params_buffer) => {
                    for params in params_buffer {
                        runner.push_params(params);
                    }
                }
                DisplayWorkerMessage::FrameBuffer(pixels) => {
                    runner.set_suspend(10_000);
                    streaming_until = Some(now + embassy_time::Duration::from_millis(10_000));

                    let mut i = 0;
                    for pixel in pixels {
                        buffer_1.buffer[i] = pixel;
                        i += 1;
                    }
                }
                DisplayWorkerMessage::Pause(paused) => {
                    info!("Playlist {}", if paused { "paused" } else { "resumed" });
                    runner.set_paused(paused);
                    dirty = true;
                }
                DisplayWorkerMessage::JumpTo(index) => {
                    if runner.jump_to(index as usize) {
                        info!("Jump to slot {}", index);
                    }
                    dirty = true;
                }
                DisplayWorkerMessage::ResumeNow => {
                    runner.clear_suspend();
                    streaming_until = None;
                    dirty = true;
                }
                DisplayWorkerMessage::Status(animation) => {
                    info!("Status screen");
                    status_screen = Some((animation, 0, 0));
                    dirty = true;
                }
                DisplayWorkerMessage::StatusTimed(animation, timeout_ms) => {
                    info!("Status screen ({} ms)", timeout_ms);
                    status_screen = Some((animation, 0, timeout_ms));
                    dirty = true;
                }
                DisplayWorkerMessage::StatusClear => {
                    info!("Status cleared");
                    status_screen = None;
                    dirty = true;
                }
            }
        }

        let t_render = embassy_time::Instant::now();

        runner.update(delta_ms, &mut buffer_1, &mut buffer_2);

        if let Some((animation, ms, timeout_ms)) = status_screen.as_mut() {
            *ms += delta_ms;

            if status_due(*ms, *timeout_ms) {
                info!("Status screen timed out after {} ms", timeout_ms);
                status_screen = None;
                dirty = true;
            } else {
                animation.draw(*ms, &AnimationParams::default(), &mut buffer_1);
            }
        }

        mirror_frame(&buffer_1, &mut last_mirror);
        apply_power_limit(&mut buffer_1, power_limit);

        let t_panel = embassy_time::Instant::now();

        led_screen.copy_eg_buffer(&buffer_1.buffer);
        led_screen.flush().await;

        let t_flushed = embassy_time::Instant::now();
        draw_us += t_panel.duration_since(t_render).as_micros() as u64;
        panel_us += t_flushed.duration_since(t_panel).as_micros() as u64;

        // ------------------------------------------------------------------
        // Live-state feed.
        // ------------------------------------------------------------------
        // `status()` clones the playlist item (heap Strings), so the hot
        // dirty-check uses only the cursor; the full snapshot is built just
        // before publishing.
        let index = runner.playlist().current_index();

        // A preview ending is only visible inside the runner — sync the meta.
        if !runner.override_active() {
            override_meta = None;
        }

        let streaming = streaming_until.is_some_and(|until| now < until);
        if last_streaming && !streaming {
            // Paint lease expired: the playlist took the panel back.
            dirty = true;
        }

        let status_screen_on = status_screen.is_some();
        let override_on = runner.override_active();

        dirty |= index != last_index
            || override_on != last_override
            || runner.is_paused() != last_paused
            || status_screen_on != last_status_screen
            || power_limit != last_power;

        if dirty || now.duration_since(last_publish).as_millis() >= 1_000 {
            let status = runner.status();

            publish_status(
                &status_tx,
                &status,
                &status_screen,
                &override_meta,
                runner.is_paused(),
                streaming,
                status_screen_on,
                power_limit,
                started,
            )
            .await;

            if status.index != last_index {
                info!("Now playing slot {}", status.index);
            }

            last_publish = now;
            last_index = status.index;
            last_override = override_on;
            last_paused = runner.is_paused();
            last_streaming = streaming;
            last_status_screen = status_screen_on;
            last_power = power_limit;
            dirty = false;
        }

        frames += 1;
        let window = window_start.elapsed().as_millis();
        if window >= 10_000 {
            info!(
                "display: {} frames in {} ms ({} fps), draw {} us/frame, panel {} us/frame, power limit {}",
                frames,
                window,
                frames * 1000 / window.max(1) as u32,
                draw_us / frames as u64,
                panel_us / frames as u64,
                power_limit
            );
            frames = 0;
            draw_us = 0;
            panel_us = 0;
            window_start = embassy_time::Instant::now();
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn publish_status(
    status_tx: &StatusWatchSender,
    runner_status: &animations::RunnerStatus,
    status_screen: &Option<(AnimationEnum, u32, u32)>,
    override_meta: &Option<(AnimationEnum, u32)>,
    paused: bool,
    streaming: bool,
    status_screen_on: bool,
    power_limit: f32,
    started: embassy_time::Instant,
) {
    let now_playing = match (status_screen, override_meta) {
        (Some((animation, ms, _)), _) => Playing {
            kind: String::from(animation.kind_name()),
            title: String::from("Status"),
            animation: animation.clone(),
            elapsed_ms: *ms,
            duration_ms: 0,
        },
        (None, Some((animation, duration))) => Playing {
            kind: String::from(animation.kind_name()),
            title: animation.display_title(),
            animation: animation.clone(),
            elapsed_ms: runner_status.elapsed_ms.min(*duration),
            duration_ms: *duration,
        },
        (None, None) => match &runner_status.item {
            Some(item) => Playing {
                kind: String::from(item.kind()),
                title: item.title(),
                animation: item.animation.clone(),
                elapsed_ms: runner_status.elapsed_ms,
                duration_ms: runner_status.duration_ms,
            },
            None => Playing {
                kind: String::from("Blank"),
                title: String::from("—"),
                animation: AnimationEnum::Blank,
                elapsed_ms: 0,
                duration_ms: 0,
            },
        },
    };

    let board_status = BoardStatus {
        now_playing,
        playlist_index: runner_status.index,
        playlist_len: runner_status.len,
        paused,
        streaming,
        status_screen: status_screen_on,
        power_limit_pct: (power_limit * 100.0).clamp(0.0, 100.0) as u8,
        uptime_s: started.elapsed().as_secs(),
        firmware_sha: String::from(crate::build_info::GIT_SHA),
        firmware_dirty: crate::build_info::GIT_DIRTY,
    };

    match serde_json::to_string(&board_status) {
        Ok(json) => status_tx.send(json),
        Err(err) => crate::error!("Could not serialise board status: {}", crate::Debug2Format(&err)),
    }
}

fn publish_playlist(playlist_tx: &PlaylistWatchSender, runner: &AnimationRunner, source: PlaylistSource) {
    let info = PlaylistInfo {
        playlist: runner.playlist().data(),
        source,
        index: runner.status().index,
    };
    match serde_json::to_string(&info) {
        Ok(json) => playlist_tx.send(json),
        Err(err) => crate::error!("Could not serialise playlist info: {}", crate::Debug2Format(&err)),
    }
}
