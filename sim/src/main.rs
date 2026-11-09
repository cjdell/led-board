//! Full-system LED board simulator for macOS.
//!
//! Runs the *actual* firmware application (led-core: picoserve web app,
//! WebSocket pump, animation display worker, heartbeat, config) with the
//! simulator `Board`, plus three machine-shaped pieces:
//!
//! * the **main thread** owns the minifb dot-matrix window (macOS demands
//!   AppKit on the main thread) and keyboard input,
//! * a thread running the embassy executor — the same executor type the
//!   Pico's cores run — hosting the display worker, control pump, and
//!   heartbeat tasks,
//! * a tokio thread accepting browser connections into the shared router.
//!
//! Start with `just sim`, open http://localhost:8080/, and drive the board
//! exactly like the real one. `tmp/host_board/` is the littlefs equivalent
//! (config.json + playlist survive restarts); `/api/ota` verifies hashes
//! against `tmp/host_board_firmware.bin`.

#![recursion_limit = "512"]

mod board;
mod display;
mod fs;
mod http;
mod ota;

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration as StdDuration;

use embassy_executor::{Executor, Spawner};
use embedded_tools::config::{ConfigFile, storage::LocalFsConfigFileStorage};
use embedded_tools::local_fs::LocalFsTrait as _;
use led_core::config::{DEFAULT_PLAYLIST_FILENAME, LedBoardConfig};
use led_core::led_screen::{HEIGHT, LedScreen, TOTAL_PIXELS, WIDTH};
use led_core::tasks::{display as led_display, heartbeat, http as core_http, pump};
use led_core::types::*;
use minifb::{Key, KeyRepeat, Scale, Window, WindowOptions};
use picoserve::AppRouter;

use board::SimBoard;
use display::{SharedFrame, WindowDisplay};
use fs::HostFs;
use led_core::tasks::http::AppProps;
use ota::FileOtaBackend;

fn port() -> u16 {
    std::env::var("PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(8080)
}

fn main() {
    let addr: SocketAddr = format!("0.0.0.0:{}", port()).parse().unwrap();

    // The channels are the same embassy-sync types as the Pico's; leaked to
    // 'static exactly like `make_static!` does there.
    let display_channel: &'static DisplayWorkerChannel = Box::leak(Box::new(DisplayWorkerChannel::new()));
    let ws_channel: &'static WebSocketIncomingChannel = Box::leak(Box::new(WebSocketIncomingChannel::new()));
    let activity_watch: &'static ActivityWatch = Box::leak(Box::new(ActivityWatch::new()));

    let mut frame = SharedFrame::new();
    let heartbeat_led = Arc::new(AtomicBool::new(false));

    // Hand the built router to the web thread as a pointer, mirroring the
    // Pico's tunnel static: after this hand-off the router is only touched
    // from the tokio thread (the channel `send` is the synchronisation
    // point), so `Sync` is not required — same single-owner invariant as
    // the device's one-executor design.
    let (router_tx, router_rx) = std::sync::mpsc::channel::<usize>();

    // ---------------------------------------------------------------- web
    let _web_handle = thread::spawn(move || {
        let app = {
            let ptr = router_rx.recv().expect("core init died before publishing the app");
            unsafe { &*(ptr as *const AppRouter<AppProps<SimBoard>>) }
        };

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime");

        let local = tokio::task::LocalSet::new();
        if let Err(err) = local.block_on(&rt, http::accept_loop(app, addr)) {
            eprintln!("(sim) web server stopped: {err}");
            std::process::exit(1);
        }
    });

    // ------------------------------------------------------------- core
    let _core_handle = {
        let frame = frame.clone();
        let heartbeat_led = heartbeat_led.clone();
        let disp_sender = display_channel.sender();
        let ws_sender = ws_channel.sender();
        let activity_sender = activity_watch.sender();

        thread::Builder::new()
            .name("led-core".into())
            .spawn(move || {
                // The std executor demands `&'static mut self` (it never
                // returns) — same lifetime the Pico gets from a static.
                let executor: &'static mut Executor = Box::leak(Box::new(Executor::new()));
                executor.run(|spawner| {
                    spawner
                        .spawn(core_init(
                            spawner,
                            core_init_args::Args {
                                frame,
                                heartbeat_led,
                                disp_sender,
                                disp_receiver: display_channel.receiver(),
                                ws_sender,
                                ws_receiver: ws_channel.receiver(),
                                activity_sender,
                                router_tx,
                            },
                        ).unwrap())
                });
            })
    };

    // ------------------------------------------------------------- window
    let mut window_options = WindowOptions::default();
    window_options.scale = Scale::X8;

    let mut window =
        Window::new("LED Board Sim", WIDTH, HEIGHT, window_options).expect("could not open the dot-matrix window");

    window.set_target_fps(60);

    let ws_key_sender = ws_channel.sender();
    let mut pixel_buffer = vec![0xFF00_0000u32; TOTAL_PIXELS];
    let mut title_dot = false;

    while window.is_open() && !window.is_key_down(Key::Escape) {
        if window.is_key_pressed(Key::Space, KeyRepeat::No) {
            let _ = ws_key_sender.try_send(WebSocketIncomingMessage::Next);
        }

        if frame.take_into(&mut pixel_buffer) {
            let _ = window.update_with_buffer(&pixel_buffer, WIDTH, HEIGHT);
        }

        let dot = heartbeat_led.load(Ordering::Relaxed);
        if dot != title_dot {
            title_dot = dot;
            window.set_title(&format!(
                "LED Board Sim  {}   http://localhost:{}   [Space = next]",
                if dot { "●" } else { "○" },
                port()
            ));
        }

        thread::sleep(StdDuration::from_millis(8));
    }

    eprintln!("(sim) window closed — exiting");
    std::process::exit(0);
}

mod core_init_args {
    use super::*;

    pub struct Args {
        pub frame: SharedFrame,
        pub heartbeat_led: Arc<AtomicBool>,
        pub disp_sender: DisplayWorkerSender,
        pub disp_receiver: DisplayWorkerReceiver,
        pub ws_sender: WebSocketIncomingSender,
        pub ws_receiver: WebSocketIncomingReceiver,
        pub activity_sender: ActivityWatchSender,
        pub router_tx: std::sync::mpsc::Sender<usize>,
    }
}

use core_init_args::Args;

/// Async boot, mirroring `firmware/src/bin/ws.rs`'s `main`: open the store,
/// load config, build the app, load the saved playlist, spawn the tasks.
#[embassy_executor::task]
async fn core_init(spawner: Spawner, args: Args) -> ! {
    let Args {
        frame,
        heartbeat_led,
        disp_sender,
        disp_receiver,
        ws_sender,
        ws_receiver,
        activity_sender,
        router_tx,
    } = args;

    let local_fs = HostFs::new("tmp/host_board");

    let config_file = ConfigFile::new(
        LocalFsConfigFileStorage::new(local_fs.clone(), "config.json".to_string()),
        LedBoardConfig::default(),
    )
    .await;

    let board = SimBoard::new(heartbeat_led);

    let app = core_http::build_app(AppProps::new(
        ws_sender,
        FileOtaBackend::default(),
        local_fs.clone(),
        config_file,
        board.clone(),
    ));

    let _ = router_tx.send(app as *const AppRouter<AppProps<SimBoard>> as usize);

    match local_fs.read_text_file(DEFAULT_PLAYLIST_FILENAME).await {
        Ok(json) => match serde_json::from_str::<Vec<(animations::AnimationEnum, u32)>>(&json) {
            Ok(playlist_data) => {
                disp_sender.send(DisplayWorkerMessage::Playlist(playlist_data)).await;
            }
            Err(err) => eprintln!("(sim) Could not read default playlist: {err:?}"),
        },
        Err(_) => {
            // No saved playlist yet: the display worker falls back to the
            // default one, exactly like the device.
        }
    }

    // The device boots with power_limit 0.1 (strip power budget) — on a
    // window that reads as "nothing happened". Run the sim bright unless
    // the environment asks for the device behaviour.
    let power_limit: f32 = std::env::var("SIM_POWER_LIMIT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1.0);
    disp_sender.send(DisplayWorkerMessage::PowerLimit(power_limit)).await;

    spawner.spawn(display_task(disp_receiver, LedScreen::new(WindowDisplay::new(frame))).unwrap());
    spawner.spawn(pump_task(ws_receiver, disp_sender, local_fs, activity_sender).unwrap());
    spawner.spawn(heartbeat_task(board).unwrap());

    println!("(sim) led-core running — display, control pump, heartbeat.");

    // The executor on this thread runs until the process exits.
    std::future::pending::<()>().await;
    loop {}
}

#[embassy_executor::task]
async fn display_task(receiver: DisplayWorkerReceiver, screen: LedScreen<WindowDisplay>) -> ! {
    led_display::run(receiver, screen).await
}

#[embassy_executor::task]
async fn pump_task(
    receiver: WebSocketIncomingReceiver,
    display: DisplayWorkerSender,
    local_fs: HostFs,
    activity: ActivityWatchSender,
) -> ! {
    pump::run(receiver, display, local_fs, activity).await
}

#[embassy_executor::task]
async fn heartbeat_task(board: SimBoard) -> ! {
    heartbeat::run(board).await
}
