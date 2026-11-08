//! ESP32-C3 slave-radio firmware entry point.
//!
//! Boots the radio + network stacks, wires the `radio_link` UART to the Pico,
//! and starts the supervisor, HTTP, and provisioning services.
//!
//! Wiring: GPIO21 (UART1 TX) → Pico GP13 (UART0 RX), GPIO20 (UART1 RX) ← Pico
//! GP12 (UART0 TX), GND ↔ GND, 5 V/VIN ↔ VSYS.

#![no_std]
#![no_main]

extern crate alloc;

use alloc::boxed::Box;

use embassy_executor::Spawner;
use embassy_net::{Runner, StackResources};
use embassy_sync::mutex::Mutex;
use esp_alloc as _;
use esp_backtrace as _;
use esp_hal::{
    clock::CpuClock,
    dma::DmaRxBuf,
    dma_buffers,
    gpio::{Level, Output},
    interrupt::software::SoftwareInterruptControl,
    ram,
    rng::Rng,
    timer::timg::TimerGroup,
    uart::{Config as UartConfig, Uart, uhci::Uhci},
};
use esp_radio::wifi::{
    ap::AccessPointConfig, sta::StationConfig, AuthenticationMethod, Config, ControllerConfig,
};
use esp_storage::FlashStorage;
use radio_link::RadioMode;

use slave_radio::{
    httpd, link, net, state,
    store::{SharedStore, Store},
    supervisor,
};

esp_bootloader_esp_idf::esp_app_desc!();

macro_rules! mk_static {
    ($t:ty,$val:expr) => {{
        static STATIC_CELL: static_cell::StaticCell<$t> = static_cell::StaticCell::new();
        #[deny(unused_attributes)]
        let x = STATIC_CELL.uninit().write(($val));
        x
    }};
}

#[esp_rtos::main]
async fn main(spawner: Spawner) -> ! {
    esp_println::logger::init_logger_from_env();
    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);

    esp_alloc::heap_allocator!(#[ram(reclaimed)] size: 64 * 1024);
    esp_alloc::heap_allocator!(size: 48 * 1024);

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let sw_int = SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);
    esp_rtos::start(timg0.timer0, sw_int.software_interrupt0);

    // Bring-up diagnostics: wait for a host to attach before printing anything.
    // The USB-JTAG FIFO drops output while no listener is attached, and opening
    // the port resets the chip — so this delay makes each attach replay boot.
    embassy_time::Timer::after_millis(6000).await;
    esp_println::println!(
        "stage: rtos up, reset={:?}",
        esp_hal::system::reset_reason()
    );

    log::info!("slave_radio booting (fw {})", state::FIRMWARE);

    // ---- persistent store ---------------------------------------------------
    esp_println::println!("stage: store init");
    let store: &'static SharedStore =
        Box::leak(Box::new(Mutex::new(Store::new(FlashStorage::new(peripherals.FLASH)))));
    httpd::init_store(store);

    let cfg = {
        let mut store = store.lock().await;
        store.load()
    };
    log::info!(
        "store: mode={:?} networks={} ap={}",
        cfg.mode,
        cfg.networks.len(),
        cfg.ap_ssid
    );

    // ---- UART link to the Pico ----------------------------------------------
    esp_println::println!("stage: uart init");
    // The RX side runs through the UHCI UART-DMA engine. The plain
    // interrupt-driven path could not keep up above 115200 — esp-radio's
    // interrupt load starved the 128-byte FIFO and sustained transfers
    // overflowed (`FifoOverflowed`). With DMA the engine drains the FIFO on its
    // own, so the link runs at `radio_link::BAUD` (921600) with a 4 KB buffer
    // absorbing scheduling latency.
    let uart_config = UartConfig::default().with_baudrate(radio_link::BAUD);
    let uart = Uart::new(peripherals.UART1, uart_config)
        .expect("uart config")
        .with_tx(peripherals.GPIO21)
        .with_rx(peripherals.GPIO20);

    let (rx_buffer, rx_descriptors, _tx_buffer, _tx_descriptors) =
        dma_buffers!(link::RX_DMA_CHUNK as usize);
    let dma_rx = DmaRxBuf::new(rx_descriptors, rx_buffer).expect("dma rx buffer");
    // Second ping-pong buffer: rx_task re-arms this the instant the other
    // transfer completes, so the DMA is never disarmed while frames are
    // decoded (a disarmed window shed bytes mid-frame under load).
    let (rx_buffer2, rx_descriptors2, _, _) = dma_buffers!(link::RX_DMA_CHUNK as usize);
    let dma_rx2 = DmaRxBuf::new(rx_descriptors2, rx_buffer2).expect("dma rx buffer 2");

    let mut uhci = Uhci::new(uart, peripherals.UHCI0, peripherals.DMA_CH0);
    uhci.apply_rx_config(&esp_hal::uart::uhci::RxConfig::default().with_chunk_limit(link::RX_DMA_CHUNK))
        .expect("uhci rx config");
    uhci.apply_tx_config(&esp_hal::uart::uhci::TxConfig::default())
        .expect("uhci tx config");
    uhci.set_uart_config(&uart_config).expect("uart config");

    let (uart_rx, uart_tx) = uhci.into_async().split();
    // TX stays on the plain async UART path; only RX needs DMA.
    let uart_tx = uart_tx.uart_tx;

    let led = Output::new(peripherals.GPIO8, Level::Low, Default::default());

    link::start(&spawner, uart_rx, uart_tx, dma_rx, dma_rx2, led);
    esp_println::println!(
        "stage: link started (UART1 {} baud, RX via UHCI DMA)",
        radio_link::BAUD
    );

    // ---- WiFi controller + network stacks ------------------------------------
    esp_println::println!("stage: wifi init");
    let initial_config = match cfg.mode {
        RadioMode::AccessPoint => Config::AccessPoint(
            AccessPointConfig::default()
                .with_ssid(cfg.ap_ssid.clone())
                .with_auth_method(AuthenticationMethod::Wpa2Personal)
                .with_password("ledboard123".into()),
        ),
        RadioMode::ApStation => Config::AccessPointStation(
            StationConfig::default(),
            AccessPointConfig::default()
                .with_ssid(cfg.ap_ssid.clone())
                .with_auth_method(AuthenticationMethod::Wpa2Personal)
                .with_password("ledboard123".into()),
        ),
        RadioMode::Station => Config::Station(StationConfig::default()),
    };

    let (controller, interfaces) = esp_radio::wifi::new(
        peripherals.WIFI,
        ControllerConfig::default().with_initial_config(initial_config),
    )
    .expect("wifi init");

    esp_println::println!("stage: wifi up");
    log::info!("wifi controller up, initial mode: {:?}", cfg.mode);

    let sta_config = embassy_net::Config::dhcpv4(Default::default());
    let ap_config = embassy_net::Config::ipv4_static(embassy_net::StaticConfigV4 {
        address: embassy_net::Ipv4Cidr::new(state::AP_IP, state::AP_IP_CIDR),
        gateway: Some(state::AP_IP),
        dns_servers: Default::default(),
    });

    let rng = Rng::new();
    let seed = (rng.random() as u64) << 32 | rng.random() as u64;

    let (ap_stack, ap_runner) = embassy_net::new(
        interfaces.access_point,
        ap_config,
        // dhcp + dns + mdns + 2 acceptors + client connections (slots are cheap;
        // per-socket buffers are heap-allocated)
        mk_static!(StackResources<12>, StackResources::<12>::new()),
        seed,
    );
    let (sta_stack, sta_runner) = embassy_net::new(
        interfaces.station,
        sta_config,
        // mdns + 3 relay acceptors + active connections
        mk_static!(StackResources<16>, StackResources::<16>::new()),
        seed,
    );

    spawner.spawn(net_task(ap_runner).unwrap());
    spawner.spawn(net_task(sta_runner).unwrap());

    // ---- services --------------------------------------------------------------
    spawner.spawn(net::dhcp_task(ap_stack, state::AP_IP).unwrap());
    spawner.spawn(net::captive_task(ap_stack, state::AP_IP).unwrap());

    httpd::start(&spawner, ap_stack, sta_stack);
    esp_println::println!("stage: httpd started");

    supervisor::start(
        &spawner,
        supervisor::SupervisorInit {
            controller,
            sta_stack,
            ap_stack,
            store,
            spawner,
        },
    );

    log::info!("slave_radio: services running");
    esp_println::println!("stage: services running");

    loop {
        embassy_time::Timer::after_millis(1_000).await;
    }
}

#[embassy_executor::task(pool_size = 2)]
async fn net_task(mut runner: Runner<'static, esp_radio::wifi::Interface<'static>>) {
    runner.run().await;
}
