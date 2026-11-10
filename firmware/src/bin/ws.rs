#![no_std]
#![no_main]
#![recursion_limit = "256"]

extern crate alloc;

use alloc::{string::ToString as _, sync::Arc, vec::Vec};
use animations::PlaylistItem;
#[cfg(any(feature = "wifi", feature = "usb_ethernet", feature = "ppp"))]
use cyw43_pio::{DEFAULT_CLOCK_DIVIDER, PioSpi};
use defmt::*;
use embassy_executor::Spawner;
// Used only by the `core1` feature's EXECUTOR1 below.
#[cfg(feature = "core1")]
use embassy_executor::Executor;
use embassy_futures::join::join;
#[cfg(not(feature = "radio"))]
use embassy_net::Stack;
use embassy_rp::rom_data;
use embassy_rp::{
    bind_interrupts,
    block::*,
    bootsel::is_bootsel_pressed,
    dma,
    flash::{self, Flash},
    peripherals::*,
    pio::{InterruptHandler, Pio},
    trng::{self},
    usb,
    watchdog::Watchdog,
};
// The cyw43 PIO-SPI + TRNG bring-up below is only wired into the
// IP-stack network features; the radio build lets the C3 own WiFi.
#[cfg(any(feature = "wifi", feature = "usb_ethernet", feature = "ppp"))]
use embassy_rp::{
    gpio::{Level, Output},
    trng::Trng,
};
// `core1` feature only — the pre-Nov-2026 display arrangement (see the
// cfg blocks below); the default single-core build never references it.
#[cfg(feature = "core1")]
use embassy_rp::multicore::spawn_core1;
use embassy_sync::rwlock::RwLock;
use embassy_time::{Duration, Timer};
use embedded_alloc::LlffHeap as Heap;
use embedded_tools::config::{ConfigFile, ConfigFileTrait, storage::LocalFsConfigFileStorage};
use embedded_tools::local_fs::LocalFsTrait as _;
use firmware::{
    board::ActiveBoard,
    config::{DEFAULT_PLAYLIST_FILENAME, LedBoardConfig},
    led_screen::LedScreen,
    make_static,
    tasks::http::AppProps,
    types::*,
    ws2812p::{PioWs2812ParallelDriver, PioWs2812ParallelProgram},
};
use led_core::tasks::{display, heartbeat, http, pump};
#[cfg(not(feature = "radio"))]
use picoserve::AppRouter;
use rp235x_embedded_tools::flash::{FlashStorage, LittleFsFlashStorage};
#[cfg(feature = "core1")]
use static_cell::StaticCell;

#[cfg(feature = "defmt_tcp")]
use defmt_tcp;

#[cfg(feature = "defmt_rtt")]
use defmt_rtt as _;

#[cfg(all(feature = "defmt_tcp", not(feature = "console")))]
use panic_reset as _;

#[cfg(all(feature = "defmt_rtt", not(feature = "console")))]
use panic_probe as _;

#[unsafe(link_section = ".start_block")]
#[used]
pub static IMAGE_DEF: Block<3> = Block::new([
    {
        let value = IMAGE_TYPE_EXE
            | IMAGE_TYPE_EXE_CHIP_RP2350
            | IMAGE_TYPE_EXE_CPU_ARM
            | IMAGE_TYPE_EXE_TYPE_SECURITY_S
            | IMAGE_TYPE_TBYB;

        item_generic_1bs(value, 1, ITEM_1BS_IMAGE_TYPE)
    },
    item_generic_1bs(0, 2, ITEM_1BS_VERSION),
    {
        let major = 1u16;
        let minor = 3u16;
        (major as u32) << 16 | minor as u32
    },
]);

const FLASH_SIZE: usize = 4 * 1024 * 1024;

#[global_allocator]
static HEAP: Heap = Heap::empty();

bind_interrupts!(struct Irqs {
    PIO0_IRQ_0 => InterruptHandler<PIO0>;
    PIO1_IRQ_0 => InterruptHandler<PIO1>;
    DMA_IRQ_0 => dma::InterruptHandler<DMA_CH0>, dma::InterruptHandler<DMA_CH1>, dma::InterruptHandler<DMA_CH2>, dma::InterruptHandler<DMA_CH3>;
    USBCTRL_IRQ => usb::InterruptHandler<USB>;
    TRNG_IRQ => trng::InterruptHandler<TRNG>;
    UART0_IRQ => embassy_rp::uart::BufferedInterruptHandler<UART0>;
});

#[cfg(feature = "core1")]
static mut CORE1_STACK: embassy_rp::multicore::Stack<32768> = embassy_rp::multicore::Stack::new();
#[cfg(feature = "core1")]
static EXECUTOR1: StaticCell<Executor> = StaticCell::new();

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    // Initialize the allocator BEFORE you use it
    unsafe {
        embedded_alloc::init!(HEAP, 192 * 1024);
    }
    firmware::tasks::usb_dbg::stage(0, "allocator up (192 KiB heap)");

    let mut p = embassy_rp::init(Default::default());
    firmware::tasks::usb_dbg::stage(10, "clocks + peripherals up");

    let pio = p.PIO1;
    let dma = p.DMA_CH2;

    let Pio { mut common, sm0, .. } = Pio::new(pio, Irqs);

    let program = PioWs2812ParallelProgram::new(&mut common);
    let ws2812: PioWs2812ParallelDriver<'_, _, 0, 300> = PioWs2812ParallelDriver::new(
        &mut common,
        sm0,
        dma,
        Irqs,
        p.PIN_2,
        p.PIN_3,
        p.PIN_4,
        p.PIN_5,
        p.PIN_6,
        p.PIN_7,
        p.PIN_8,
        p.PIN_9,
        &program,
    );

    firmware::tasks::usb_dbg::stage(11, "ws2812 PIO driver configured");

    let display_worker_channel = make_static!(DisplayWorkerChannel, DisplayWorkerChannel::new());

    let display_worker_sender = display_worker_channel.sender();
    let display_worker_receiver = display_worker_channel.receiver();

    // Live-state feeds (now playing / live playlist JSON) — the display
    // worker produces them, the web app consumes and pushes them.
    let status_watch = make_static!(StatusWatch, StatusWatch::new());
    let playlist_watch = make_static!(PlaylistWatch, PlaylistWatch::new());

    // Display worker placement. Single-core is the default: the display-only
    // image (ws_art) proved the whole pipeline works on the main executor,
    // while the core1 split kept freezing the panel at its first frame.
    // `core1` restores the old multicore arrangement for comparison.
    #[cfg(feature = "core1")]
    {
        firmware::tasks::usb_dbg::stage(2, "spawning core1 display");
        spawn_core1(
            p.CORE1,
            unsafe { &mut *core::ptr::addr_of_mut!(CORE1_STACK) },
            move || {
                let executor1 = EXECUTOR1.init(Executor::new());
                executor1.run(|spawner| {
                    spawner.spawn(unwrap!(core1_task(
                        ws2812,
                        display_worker_receiver,
                        status_watch.sender(),
                        playlist_watch.sender()
                    )))
                });
            },
        );
    }

    #[cfg(not(feature = "core1"))]
    {
        firmware::tasks::usb_dbg::stage(2, "spawning display worker (single core)");
        spawner.spawn(unwrap!(display_task(
            ws2812,
            display_worker_receiver,
            status_watch.sender(),
            playlist_watch.sender()
        )));
    }

    // Bench debug console (feature "console"): USB-CDC port + breadcrumb ring.
    // Started AFTER spawn_core1 — a console that initializes the USB PHY
    // before core1 comes off reset breaks the core1 handoff (observed on the
    // venue bench: display worker never spawned).
    // (usb_ethernet/ppp builds own p.USB for networking; they get no console.)
    #[cfg(all(feature = "console", not(feature = "usb_ethernet"), not(feature = "ppp")))]
    {
        let dbg_driver = usb::Driver::new(p.USB, Irqs);
        firmware::tasks::usb_dbg::start(spawner, dbg_driver);
        firmware::tasks::usb_dbg::stage(
            1,
            &alloc::format!(
                "BOOT ws v{} [{}]",
                env!("CARGO_PKG_VERSION"),
                if cfg!(feature = "radio") { "radio" } else if cfg!(feature = "wifi") { "wifi" } else { "?" }
            ),
        );
    }

    info!("Init!");

    Timer::after(Duration::from_millis(1_000)).await;

    rp235x_ota::mark_firmware_good();

    let watchdog = Watchdog::new(p.WATCHDOG);
    info!("Reset Reason: {:?}", defmt::Debug2Format(&watchdog.reset_reason()));
    // watchdog.start(Duration::from_secs(15));
    let watchdog: SharedWatchdog = Arc::new(RwLock::new(watchdog));

    // The cyw43 PIO-SPI link and TRNG feed only the network features that
    // run an IP stack (wifi's cyw43, and the ppp / usb_ethernet modems);
    // radio builds leave these peripherals unclaimed (GP23/24/25/29).
    #[cfg(any(feature = "wifi", feature = "usb_ethernet", feature = "ppp"))]
    let (spi, trng) = {
        let cs = Output::new(p.PIN_25, Level::High);
        let mut pio = Pio::new(p.PIO0, Irqs);
        let spi = PioSpi::new(
            &mut pio.common,
            pio.sm0,
            // ÷2 → 75 MHz SM clock → 37.5 MHz GSPI (half the SM clock: the PIO programs
            // spend two SM cycles per SPI bit), inside the CYW43439's 50 MHz spec and an
            // integer divider. At this speed the program-selection logic (embassy PR
            // #4792, in our locked checkout) loads the sample-on-clock-low program, the
            // one that actually moves bulk data above ~25 MHz — the old RM2 workaround
            // for issue #3960 predates both #4792 and the #6001 RP2350 RX DMA fix and
            // only got the 25 MHz / sample-on-high tier. See fletcher.cool/blog/pico-2w-wifi-throughput.
            DEFAULT_CLOCK_DIVIDER,
            pio.irq0,
            cs,
            p.PIN_24,
            p.PIN_29,
            dma::Channel::new(p.DMA_CH0, Irqs),
            dma::Channel::new(p.DMA_CH1, Irqs),
        );
        let trng = Trng::new(p.TRNG, Irqs, trng::Config::default());
        (spi, trng)
    };

    let ethernet_signal = make_static!(EthernetSignal, EthernetSignal::new());

    firmware::tasks::usb_dbg::stage(12, "mounting littlefs");
    let flash = make_static!(Flash<FLASH, flash::Async, FLASH_SIZE>, Flash::new(p.FLASH, p.DMA_CH3, Irqs));

    let flash = Arc::new(RwLock::new(flash));

    let local_fs = match LocalFs::new(LittleFsFlashStorage::new(flash.clone())) {
        Ok(local_fs) => local_fs,
        Err(err) => {
            error!("Bad FS - Reformatting... {}", defmt::Debug2Format(&err));

            let mut io = LittleFsFlashStorage::new(flash.clone());

            match LocalFs::format(&mut io) {
                Ok(()) => LocalFs::new(io).unwrap(),
                Err(err) => {
                    error!("Could not reformat FS! {}", defmt::Debug2Format(&err));
                    Timer::after(Duration::from_millis(5_000)).await;
                    defmt::panic!("Rebooting...");
                }
            }
        }
    };

    let config_file = ConfigFile::new(
        LocalFsConfigFileStorage::new(local_fs.clone(), "config.json".to_string()),
        LedBoardConfig::default(),
    )
    .await;

    {
        let cfg_data = config_file.get_data().await;
        firmware::tasks::usb_dbg::log(format_args!(
            "fs mounted; cfg: mode {:?} networks {}",
            defmt::Debug2Format(&cfg_data.wifi_mode),
            cfg_data.known_wifi_networks.len()
        ));
    }

    firmware::tasks::usb_dbg::stage(13, "filesystem + config ready");
    info!("Starting Network...");
    firmware::tasks::usb_dbg::stage(3, "starting network");

    #[cfg(feature = "wifi")]
    let (stack, wifi_control) = {
        firmware::tasks::wifi::init_wifi(
            spawner,
            spi,
            p.PIN_23,
            trng,
            ethernet_signal,
            config_file.clone(),
            watchdog.clone(),
            display_worker_channel.sender(),
        )
        .await
    };

    #[cfg(feature = "usb_ethernet")]
    let (stack, wifi_control) = {
        let driver = usb::Driver::new(p.USB, Irqs);

        firmware::tasks::usb_ethernet::init_usb_ethernet(
            spawner,
            spi,
            p.PIN_23,
            driver,
            trng,
            ethernet_signal,
            config_file.clone(),
            watchdog.clone(),
        )
        .await
    };

    #[cfg(feature = "ppp")]
    let (stack, wifi_control) = {
        let driver = usb::Driver::new(p.USB, Irqs);

        firmware::tasks::ppp::init_ppp(spawner, spi, p.PIN_23, driver, trng, ethernet_signal, watchdog.clone()).await
    };

    // ESP32-C3 slave radio: the C3 owns WiFi; browser connections are relayed
    // over UART0 (GP12 -> C3 RX, GP13 <- C3 TX). No IP stack runs on the Pico.
    #[cfg(feature = "radio")]
    let _wifi_control = {
        let mut uart_config = embassy_rp::uart::Config::default();
        uart_config.baudrate = firmware::tasks::radio::BAUD;

        // RX ring: structural headroom, not a race. With the flow-control
        // window the radio can never send more than the link protocol
        // tolerates, so even a stalled consumer (flash-sector erase during
        // OTA, ~30–80 ms) only closes the window upstream — nothing drops.
        // 16 KB ≈ 170 ms of full-rate slack; keep it generous.
        let bufs = make_static!(([u8; 16384], [u8; 4096]), ([0u8; 16384], [0u8; 4096]));

        let uart = embassy_rp::uart::BufferedUart::new(
            p.UART0,
            p.PIN_12,
            p.PIN_13,
            Irqs,
            &mut bufs.0,
            &mut bufs.1,
            uart_config,
        );
        let (uart_tx, uart_rx) = uart.split();

        firmware::tasks::radio::init(spawner, uart_tx, uart_rx, ethernet_signal, config_file.clone()).await
    };

    info!("Waiting for Network...");

    #[cfg(not(feature = "radio"))]
    {
        let mut receiver = ethernet_signal.receiver().unwrap();

        loop {
            if let EthernetSignalMessage::Connected(_) = receiver.get().await {
                info!("MAIN GOT SIGNAL");
                Timer::after(Duration::from_millis(1_000)).await;
                break;
            }
        }
    }

    // With the radio link, don't block forever if the C3 is not flashed yet —
    // connections simply arrive once it comes up.
    #[cfg(feature = "radio")]
    {
        let mut receiver = ethernet_signal.receiver().unwrap();

        let wait = async {
            loop {
                if let EthernetSignalMessage::Connected(_) = receiver.get().await {
                    info!("MAIN GOT SIGNAL");
                    break;
                }
            }
        };

        if embassy_time::with_timeout(Duration::from_secs(30), wait).await.is_err() {
            warn!("No network signal yet — continuing; the radio link can come up later");
            firmware::tasks::usb_dbg::stage(4, "net: 30s no signal yet, pressing on");
        } else {
            firmware::tasks::usb_dbg::stage(4, "net: signal up");
        }

        Timer::after(Duration::from_millis(1_000)).await;
    }

    #[cfg(not(feature = "radio"))]
    {
        firmware::tasks::usb_dbg::stage(14, "starting defmt-TCP logger");
        spawner.spawn(defmt_runner(stack).unwrap());
    }

    Timer::after(Duration::from_millis(1_000)).await;

    // The machine this app runs on, as the core sees it: reboot / watchdog /
    // heartbeat / provisioning delegate to the hardware (or the slave radio).
    #[cfg(feature = "radio")]
    let board = ActiveBoard::new(config_file.clone(), watchdog.clone());
    #[cfg(not(feature = "radio"))]
    let board = ActiveBoard::new(watchdog.clone(), wifi_control.clone());

    let activity_watch = make_static!(ActivityWatch, ActivityWatch::new());
    let activity_watch_sender = activity_watch.sender();

    spawner.spawn(watchdog_runner(watchdog.clone()).unwrap());
    spawner.spawn(heartbeat_task(board.clone()).unwrap());
    firmware::tasks::usb_dbg::stage(15, "heartbeat + watchdog tasks running");

    match local_fs.read_text_file(DEFAULT_PLAYLIST_FILENAME).await {
        Ok(json) => {
            match serde_json::from_str::<Vec<PlaylistItem>>(&json) {
                Ok(playlist_items) => {
                    firmware::tasks::usb_dbg::stage(
                        5,
                        &alloc::format!("pl: {} items from saved playlist", playlist_items.len()),
                    );
                    // Non-blocking: a wedged display worker must never stall
                    // boot (this cost us the whole venue evening when the
                    // capacity-1 queue filled and core1 stopped consuming).
                    if display_worker_sender
                        .try_send(DisplayWorkerMessage::Playlist {
                            items: playlist_items,
                            source: PlaylistSource::Saved,
                        })
                        .is_err()
                    {
                        error!("display queue full at boot — playlist not applied (stale worker?)");
                    }
                }
                Err(err) => {
                    error!("Could not read default playlist: {}", defmt::Debug2Format(&err));
                    firmware::tasks::usb_dbg::log(format_args!("pl: saved playlist CORRUPT"));
                }
            };
        }
        Err(_) => {
            firmware::tasks::usb_dbg::log(format_args!("pl: no saved playlist, using built-in default"));
        }
    };

    let flash_storage = FlashStorage::new(flash);

    let web_socket_incoming_channel = make_static!(WebSocketIncomingChannel, WebSocketIncomingChannel::new());

    let app_props = AppProps::new(
        web_socket_incoming_channel.sender(),
        status_watch,
        playlist_watch,
        firmware::board::RpOtaBackend(flash_storage),
        local_fs.clone(),
        config_file.clone(),
        board.clone(),
    );

    #[cfg(not(feature = "radio"))]
    {
        firmware::tasks::usb_dbg::stage(6, "app: building router");
        let app = http::build_app(app_props);

        for id in 0..http::WEB_TASK_POOL_SIZE {
            spawner.spawn(web_task(id, stack, app).unwrap());
        }

        firmware::tasks::usb_dbg::stage(7, &alloc::format!("app: {} web tasks spawned", http::WEB_TASK_POOL_SIZE));
    }

    // Relay-served app: publish the router before any stream is accepted.
    #[cfg(feature = "radio")]
    firmware::tasks::tunnel::prepare(http::build_app(app_props));

    let web_socket_incoming_receiver = web_socket_incoming_channel.receiver();

    let ws_fut = pump::run(
        web_socket_incoming_receiver,
        display_worker_sender,
        local_fs.clone(),
        activity_watch_sender,
    );

    let animation_fut = async {
        loop {
            if is_bootsel_pressed(p.BOOTSEL.reborrow()) {
                Timer::after(Duration::from_secs(1)).await;

                warn!("Reboot button pressed!");

                Timer::after(Duration::from_secs(1)).await;

                const REBOOT_TYPE_BOOTSEL: u32 = 0x0002;
                const REBOOT2_FLAG_NO_RETURN_ON_SUCCESS: u32 = 0x0100;

                rp235x_ota::REBOOTING.store(true, core::sync::atomic::Ordering::SeqCst);
                rom_data::reboot(REBOOT_TYPE_BOOTSEL | REBOOT2_FLAG_NO_RETURN_ON_SUCCESS, 10, 0, 0);
            }

            Timer::after(Duration::from_millis(100)).await;
        }
    };

    firmware::tasks::usb_dbg::stage(8, "boot: complete — pump+animation loop running");

    join(ws_fut, animation_fut).await;
}

#[embassy_executor::task]
async fn display_task(
    ws2812: PioWs2812ParallelDriver<'static, PIO1, 0, 300>,
    receiver: DisplayWorkerReceiver,
    status_tx: StatusWatchSender,
    playlist_tx: PlaylistWatchSender,
) {
    firmware::tasks::usb_dbg::stage(9, "display worker starting (core0)");

    let led_screen = LedScreen::new(ws2812);

    display::run(receiver, led_screen, status_tx, playlist_tx).await;
}

#[embassy_executor::task]
async fn core1_task(
    ws2812: PioWs2812ParallelDriver<'static, PIO1, 0, 300>,
    receiver: DisplayWorkerReceiver,
    status_tx: StatusWatchSender,
    playlist_tx: PlaylistWatchSender,
) {
    firmware::tasks::usb_dbg::stage(9, "core1: display worker starting");

    let led_screen = LedScreen::new(ws2812);

    display::run(receiver, led_screen, status_tx, playlist_tx).await;
}

#[cfg(not(feature = "radio"))]
#[embassy_executor::task(pool_size = http::WEB_TASK_POOL_SIZE)]
async fn web_task(id: usize, stack: Stack<'static>, app: &'static AppRouter<AppProps<ActiveBoard>>) {
    http::web_task_body(id, stack, app).await;
}

// Generic `#[embassy_executor::task]` functions are impossible (the pool is a
// static), so these concrete wrappers feed the shared cores in led-core the
// executor slots they need on this target.

#[embassy_executor::task]
async fn heartbeat_task(board: ActiveBoard) -> ! {
    heartbeat::run(board).await
}

#[cfg(not(feature = "radio"))]
#[embassy_executor::task]
async fn defmt_runner(stack: Stack<'static>) {
    #[cfg(feature = "defmt_tcp")]
    defmt_tcp::TCP_SENDER.runner(stack).await;
}

#[embassy_executor::task]
async fn watchdog_runner(watchdog: SharedWatchdog) {
    let delay = Duration::from_millis(5_000);

    loop {
        Timer::after(delay).await;

        // Never feed while a ROM reboot is pending: `reboot()` arms the
        // watchdog as its reset mechanism, so feeding here silently cancels
        // every OTA/bootsel reboot (the board just keeps running old code).
        if rp235x_ota::REBOOTING.load(core::sync::atomic::Ordering::SeqCst) {
            continue;
        }

        if let Ok(mut watchdog) = watchdog.try_write() {
            watchdog.feed(Duration::from_millis(15_000));
        }
    }
}
