#![no_std]
#![no_main]
#![recursion_limit = "256"]
#![feature(vec_into_chunks)]

extern crate alloc;

use alloc::{boxed::Box, string::ToString as _, sync::Arc, vec::Vec};
use animations::{AnimationRunner, BufferTarget, ScrollingMessage, apply_power_limit};
use cyw43_pio::{PioSpi, RM2_CLOCK_DIVIDER};
use defmt::*;
use embassy_executor::{Executor, Spawner};
use embassy_futures::join::join;
use embassy_net::Stack;
use embassy_rp::{
    bind_interrupts,
    block::*,
    bootsel::is_bootsel_pressed,
    clocks::{ClockConfig, CoreVoltage},
    dma,
    flash::{self, Flash},
    gpio::{Level, Output},
    multicore::spawn_core1,
    peripherals::*,
    pio::{InterruptHandler, Pio},
    trng::{self, Trng},
    watchdog::Watchdog,
};
use embassy_sync::rwlock::RwLock;
use embassy_time::{Duration, Timer};
use embedded_alloc::LlffHeap as Heap;
use embedded_graphics::pixelcolor::Rgb888;
use firmware::{
    config::LedBoardConfig,
    flash::{FlashStorage, LittleFsFlashStorage},
    led_screen::LedScreen,
    make_static,
    tasks::{
        http::{self, AppProps},
        wifi::{WifiControl, init_wifi},
    },
    types::*,
    ws2812p::{PioWs2812ParallelDriver, PioWs2812ParallelProgram},
};
use static_cell::StaticCell;
use utils::{
    config::{ConfigFile, storage::LocalFsConfigFileStorage},
    local_fs::LocalFs,
};

#[cfg(feature = "defmt_tcp")]
use defmt_tcp;

#[cfg(feature = "defmt_rtt")]
use defmt_rtt as _;

#[cfg(feature = "defmt_tcp")]
use panic_reset as _;

#[cfg(feature = "defmt_rtt")]
use panic_probe as _;

#[unsafe(link_section = ".start_block")]
#[used]
pub static IMAGE_DEF: Block<3> = Block::new([
    {
        let value =
            IMAGE_TYPE_EXE | IMAGE_TYPE_EXE_CHIP_RP2350 | IMAGE_TYPE_EXE_CPU_ARM | IMAGE_TYPE_EXE_TYPE_SECURITY_S;

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
    TRNG_IRQ => trng::InterruptHandler<TRNG>;
});

static mut CORE1_STACK: embassy_rp::multicore::Stack<32768> = embassy_rp::multicore::Stack::new();
static EXECUTOR1: StaticCell<Executor> = StaticCell::new();

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    // Give probe-rs time to attach and find RTT
    for _ in 0..5000000 {
        cortex_m::asm::nop();
    }

    // Initialize the allocator BEFORE you use it
    unsafe {
        embedded_alloc::init!(HEAP, 160 * 1024);
    }

    let mut p = embassy_rp::init(Default::default());

    // // Set up for clock frequency of 200 MHz, setting all necessary defaults.
    // let mut config = embassy_rp::config::Config::new(ClockConfig::system_freq(200_000_000).unwrap());

    // // since for the rp235x there is no official support for higher clock frequencies, `system_freq()` will not set a voltage for us.
    // // We need to guess the core voltage, that is needed for the higher clock frequency. Going with a small increase from the default 1.1V here, based on
    // // what we know about the RP2040. This is not guaranteed to be correct.
    // config.clocks.core_voltage = CoreVoltage::V1_15;

    // let mut p = embassy_rp::init(config);

    let mut watchdog = Watchdog::new(p.WATCHDOG);

    info!("Reset Reason: {:?}", defmt::Debug2Format(&watchdog.reset_reason()));

    watchdog.start(Duration::from_secs(15));

    let watchdog: SharedWatchdog = Arc::new(RwLock::new(watchdog));

    Timer::after(Duration::from_millis(1_000)).await;

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

    let display_worker_channel = make_static!(DisplayWorkerChannel, DisplayWorkerChannel::new());

    let display_worker_sender = display_worker_channel.sender();
    let display_worker_receiver = display_worker_channel.receiver();

    spawn_core1(
        p.CORE1,
        unsafe { &mut *core::ptr::addr_of_mut!(CORE1_STACK) },
        move || {
            let executor1 = EXECUTOR1.init(Executor::new());
            executor1.run(|spawner| spawner.spawn(unwrap!(core1_task(ws2812, display_worker_receiver))));
        },
    );

    info!("Init");

    let cs = Output::new(p.PIN_25, Level::High);
    let mut pio = Pio::new(p.PIO0, Irqs);
    let spi = PioSpi::new(
        &mut pio.common,
        pio.sm0,
        // SPI communication won't work if the speed is too high, so we use a divider larger than `DEFAULT_CLOCK_DIVIDER`.
        // See: https://github.com/embassy-rs/embassy/issues/3960.
        RM2_CLOCK_DIVIDER,
        pio.irq0,
        cs,
        p.PIN_24,
        p.PIN_29,
        dma::Channel::new(p.DMA_CH0, Irqs),
        dma::Channel::new(p.DMA_CH1, Irqs),
    );

    let trng = Trng::new(p.TRNG, Irqs, trng::Config::default());

    let ethernet_signal = make_static!(EthernetSignal, EthernetSignal::new());

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

    info!("Starting WiFi...");

    let (stack, wifi_control) = init_wifi(
        spawner,
        spi,
        p.PIN_23,
        trng,
        ethernet_signal,
        config_file.clone(),
        watchdog.clone(),
    )
    .await;

    info!("Waiting for WiFi...");

    let mut receiver = ethernet_signal.receiver().unwrap();

    loop {
        if let EthernetSignalMessage::Connected(_) = receiver.get().await {
            info!("MAIN GOT SIGNAL");
            Timer::after(Duration::from_millis(1_000)).await;
            break;
        }
    }

    spawner.spawn(defmt_runner(stack).unwrap());

    Timer::after(Duration::from_millis(1_000)).await;

    let activity_watch = make_static!(ActivityWatch, ActivityWatch::new());
    let activity_watch_sender = activity_watch.sender();
    let activity_watch_receiver = make_static!(ActivityWatchReceiver, activity_watch.receiver().unwrap());

    spawner.spawn(watchdog_runner(watchdog.clone(), wifi_control, activity_watch_receiver).unwrap());

    // let files = local_fs.list_files().await.expect("list");
    // info!("Files: {}", defmt::Debug2Format(&files));

    // local_fs
    //     .write_binary_chunk("file.bin", 0, &[0x55, 0xAA], true)
    //     .await
    //     .expect("write");

    // let bytes = local_fs.read_binary_chunk("file.bin", 0, 2).await.expect("read");
    // info!("Data: {}", defmt::Debug2Format(&bytes));

    // let files = local_fs.list_files().await.expect("list");
    // info!("Files: {}", defmt::Debug2Format(&files));

    // use_local_fs(local_fs.clone()).await;

    let flash_storage = FlashStorage::new(flash);

    let web_socket_incoming_channel = make_static!(WebSocketIncomingChannel, WebSocketIncomingChannel::new());

    http::start_http(
        spawner,
        stack,
        AppProps::new(
            web_socket_incoming_channel.sender(),
            flash_storage,
            local_fs.clone(),
            config_file.clone(),
            watchdog.clone(),
        ),
    );

    // let led_screen = Arc::new(RwLock::<CriticalSectionRawMutex, _>::new(led_screen));

    // let mut current_animation_index: usize = 0;
    // let anim: RwLock<CriticalSectionRawMutex, Box<dyn Animation>> = RwLock::new(Box::new(Columns {}));
    // let last_write: RwLock<CriticalSectionRawMutex, _> = RwLock::new(embassy_time::Instant::now());

    let ws_fut = async {
        loop {
            match web_socket_incoming_channel.receive().await {
                WebSocketIncomingMessage::Ping => {
                    activity_watch_sender.send(embassy_time::Instant::now().as_millis());
                }
                WebSocketIncomingMessage::FrameBuffer(items) => {
                    // *last_write.write().await = embassy_time::Instant::now();

                    // let mut full_buffer: Vec<RGB8> = Vec::new();

                    // full_buffer.resize(TOTAL_PIXELS, RGB8::default());

                    // let pixels = items
                    //     .into_chunks::<3>()
                    //     .into_iter()
                    //     .map(|chunk| RGB8::new(chunk[0], chunk[1], chunk[2]))
                    //     .collect::<Vec<RGB8>>();

                    // let mut i = 0;

                    // for pixel in pixels {
                    //     full_buffer[i] = pixel;
                    //     i += 1;
                    // }

                    // let mut led_screen = led_screen.write().await;
                    // led_screen.copy_buffer(full_buffer);
                    // led_screen.flush().await;
                }
                WebSocketIncomingMessage::NoteOn(_) => {
                    // let mut anim = anim.write().await;
                    // *anim = Box::new(Circles {});
                }
                _ => (),
            }
        }
    };

    let animation_fut = async {
        loop {
            if is_bootsel_pressed(p.BOOTSEL.reborrow()) {
                display_worker_sender.send(DisplayWorkerMessage::Next).await;

                Timer::after(Duration::from_secs(1)).await;
            }

            Timer::after(Duration::from_millis(100)).await;
        }

        //     let mut target = BufferTarget::new();

        //     let start_time = embassy_time::Instant::now();
        //     let mut last_time = embassy_time::Instant::now();

        //     loop {
        //         if embassy_time::Instant::now() - *last_write.read().await < embassy_time::Duration::from_secs(5) {
        //             // Don't animate if the WebSocket is active...
        //             Timer::after(Duration::from_secs(1)).await;
        //             continue;
        //         }

        //         let now = embassy_time::Instant::now();

        //         let time_since_start = now.duration_since(start_time);
        //         let time_since_last_frame = now.duration_since(last_time);

        //         last_time = now;

        //         // target.clear(Rgb888::BLACK).unwrap();
        //         target.buffer.fill(Rgb888::default());

        //         {
        //             let mut anim = anim.write().await;
        //             anim.draw(time_since_start.as_millis() as u32, &mut target);
        //         }

        //         {
        //             let mut led_screen = led_screen.write().await;
        //             led_screen.copy_eg_buffer(&target.buffer);
        //             led_screen.flush().await;
        //         }

        //         // yield_now().await;
        //         Timer::after(Duration::from_millis(1)).await;

        //         if is_bootsel_pressed(p.BOOTSEL.reborrow()) {
        //             // warn!("Rebooting...");
        //             // Timer::after(Duration::from_secs(1)).await;
        //             // reset_to_usb_boot(0, 0);

        //             info!("Next animation...");

        //             current_animation_index = (current_animation_index + 1) % NUM_ANIMATIONS;

        //             {
        //                 let mut anim = anim.write().await;
        //                 *anim = get_animation(current_animation_index);
        //             }

        //             Timer::after(Duration::from_millis(100)).await;
        //         }
        //     }
    };

    join(ws_fut, animation_fut).await;
}

#[embassy_executor::task]
async fn core1_task(ws2812: PioWs2812ParallelDriver<'static, PIO1, 0, 300>, receiver: DisplayWorkerReceiver) {
    info!("core1_task");

    let mut led_screen = LedScreen::new(ws2812);

    led_screen.copy_eg_buffer(&[Rgb888::new(0x10, 0x10, 0x0); 2400]);
    led_screen.flush().await;

    let mut buffer_1 = BufferTarget::new();
    let mut buffer_2 = BufferTarget::new();

    let mut runner = AnimationRunner::new();
    let mut last_time = embassy_time::Instant::now();

    loop {
        let now = embassy_time::Instant::now();
        let time_since_last_frame = now.duration_since(last_time);
        let delta_ms = time_since_last_frame.as_millis() as u32;

        last_time = now;

        if let Ok(msg) = receiver.try_receive() {
            match msg {
                DisplayWorkerMessage::Next => {
                    // runner.next();

                    runner.set_override_animation(
                        Box::new(ScrollingMessage {
                            msg: "The quick brown fox jumped over the lazy dog.".to_string(),
                        }),
                        6_000,
                    );
                }
            }
        }

        runner.update(delta_ms, &mut buffer_1, &mut buffer_2);

        apply_power_limit(&mut buffer_1, 0.1);

        led_screen.copy_eg_buffer(&buffer_1.buffer);
        led_screen.flush().await;
    }
}

#[embassy_executor::task]
async fn defmt_runner(stack: Stack<'static>) {
    #[cfg(feature = "defmt_tcp")]
    defmt_tcp::TCP_SENDER.runner(stack).await;
}

#[embassy_executor::task]
async fn watchdog_runner(
    watchdog: SharedWatchdog,
    wifi_control: WifiControl,
    activity_watch: &'static mut ActivityWatchReceiver,
) {
    let delay = Duration::from_millis(5_000);
    let blink = Duration::from_millis(100);

    rp235x_ota::mark_firmware_good();

    loop {
        Timer::after(delay).await;

        let time_since_last_activity = embassy_time::Instant::now().as_millis() - activity_watch.get().await;

        info!("time_since_last_activity: {}ms", time_since_last_activity);

        if time_since_last_activity < 60_000 {
            if let Ok(mut watchdog) = watchdog.try_write() {
                watchdog.feed(Duration::from_millis(15_000));
            }
        }

        wifi_control.set_led(true).await;
        Timer::after(blink).await;
        wifi_control.set_led(false).await;
    }
}
