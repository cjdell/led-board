//! Minimal LED-art image: no WiFi, no web server, no USB, no second core.
//!
//! Purpose: prove the panel and the animation engine on hardware, in
//! isolation. Boot runs a fixed colour self-test (red → green → blue → white,
//! 600 ms each) and then hands over to the same `display::run` worker the
//! product uses, which plays the hardcoded festival playlist.
//!
//! The onboard LED (GPIO25) blinks once per second forever, so "no WiFi" can
//! never be confused with "the chip is dead": if the LED blinks, the firmware
//! is alive.
//!
//! Build:
//!   cargo objcopy --release -p firmware --features defmt_tcp --bin ws_art \
//!       --target thumbv8m.main-none-eabihf -- -O binary tmp/ws_art.bin

#![no_std]
#![no_main]

extern crate alloc;

use embassy_executor::Spawner;
use embassy_rp::block::*;
use embassy_rp::gpio::{Level, Output};
use embassy_rp::pio::InterruptHandler;
use embassy_rp::pio::Pio;
use embassy_rp::{
    bind_interrupts, dma,
    peripherals::{DMA_CH2, PIO1},
};
use embassy_time::{Duration, Timer};
use embedded_alloc::LlffHeap as Heap;
use embedded_graphics::pixelcolor::Rgb888;

use firmware::ws2812p::{PioWs2812ParallelDriver, PioWs2812ParallelProgram};
use led_core::led_screen::{LedScreen, TOTAL_PIXELS};
use led_core::tasks::display;
use led_core::types::{DisplayWorkerChannel, PlaylistWatch, StatusWatch};

include!(concat!(env!("OUT_DIR"), "/image_version.rs"));

#[unsafe(link_section = ".start_block")]
#[used]
pub static IMAGE_DEF: Block<3> = Block::new([
    {
        let value =
            IMAGE_TYPE_EXE | IMAGE_TYPE_EXE_CHIP_RP2350 | IMAGE_TYPE_EXE_CPU_ARM | IMAGE_TYPE_EXE_TYPE_SECURITY_S;

        item_generic_1bs(value, 1, ITEM_1BS_IMAGE_TYPE)
    },
    item_generic_1bs(0, 2, ITEM_1BS_VERSION),
    IMAGE_VERSION,
]);

bind_interrupts!(struct Irqs {
    PIO1_IRQ_0 => InterruptHandler<PIO1>;
    DMA_IRQ_0 => dma::InterruptHandler<DMA_CH2>;
});

#[global_allocator]
static HEAP: Heap = Heap::empty();

/// Discard logger: this image has no log transport, but embassy-rp itself
/// emits defmt symbols, so a global logger must exist to link.
#[defmt::global_logger]
struct DiscardLogger;

unsafe impl defmt::Logger for DiscardLogger {
    fn acquire() {}
    unsafe fn flush() {}
    unsafe fn release() {}
    unsafe fn write(_bytes: &[u8]) {}
}

/// Halt on panic: this image has no console, and a reset loop would hide the
/// failure behind a blink that looks healthy.
#[cfg(not(feature = "console"))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {
        cortex_m::asm::wfi();
    }
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    unsafe {
        embedded_alloc::init!(HEAP, 192 * 1024);
    }

    let p = embassy_rp::init(Default::default());

    // Onboard LED: the "firmware is alive" channel, independent of the panel.
    let mut alive = Output::new(p.PIN_25, Level::Low);

    let Pio { mut common, sm0, .. } = Pio::new(p.PIO1, Irqs);

    let program = PioWs2812ParallelProgram::new(&mut common);
    let ws2812: PioWs2812ParallelDriver<'_, _, 0, 300> = PioWs2812ParallelDriver::new(
        &mut common,
        sm0,
        p.DMA_CH2,
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

    let mut screen = LedScreen::new(ws2812);

    // --- Colour self-test -------------------------------------------------
    // Each colour proves a different stage: red/green/blue prove the data path
    // and per-channel wiring; white proves all three channels at once.
    for (_name, color) in [
        ("red", Rgb888::new(255, 0, 0)),
        ("green", Rgb888::new(0, 255, 0)),
        ("blue", Rgb888::new(0, 0, 255)),
        ("white", Rgb888::new(80, 80, 80)),
    ] {
        screen.copy_eg_buffer(&[color; TOTAL_PIXELS]);
        screen.flush().await;
        alive.set_high();
        Timer::after(Duration::from_millis(600)).await;
        alive.set_low();
        Timer::after(Duration::from_millis(200)).await;
    }

    // Hand over to the real worker. The channel is created and immediately
    // forgotten (leaked): the worker only ever reads it, and an empty channel
    // means "use the built-in festival playlist". No web app in this image,
    // but the worker's live-state feeds are part of its signature — leak the
    // watches and nobody subscribes.
    let channel: &'static mut DisplayWorkerChannel =
        alloc::boxed::Box::leak(alloc::boxed::Box::new(DisplayWorkerChannel::new()));

    let status_watch: &'static StatusWatch = alloc::boxed::Box::leak(alloc::boxed::Box::new(StatusWatch::new()));
    let playlist_watch: &'static PlaylistWatch = alloc::boxed::Box::leak(alloc::boxed::Box::new(PlaylistWatch::new()));

    // Liveness blink on the onboard LED, independent of the display loop.
    spawner.spawn(alive_task(alive).unwrap());

    display::run(
        channel.receiver(),
        screen,
        status_watch.sender(),
        playlist_watch.sender(),
    )
    .await
}

#[embassy_executor::task]
async fn alive_task(mut led: Output<'static>) {
    loop {
        led.set_high();
        Timer::after(Duration::from_millis(100)).await;
        led.set_low();
        Timer::after(Duration::from_millis(900)).await;
    }
}
