use crate::{
    config::WifiMode,
    make_static,
    tasks::{WifiControl, dhcp_task, mdns_runner, mdns_task},
    types::{EthernetSignal, EthernetSignalMessage, LedBoardConfigFile, SharedWatchdog},
};
use alloc::{
    format,
    string::{String, ToString as _},
};
use core::net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4};
use cyw43::{Control, JoinOptions, PowerManagementMode, aligned_bytes};
use cyw43_pio::PioSpi;
use defmt::*;
use edge_mdns::{
    HostAnswersMdnsHandler,
    buf::VecBufAccess,
    domain::base::Ttl,
    host::Host,
    io::{DEFAULT_SOCKET, MdnsIoError},
};
use edge_nal::{UdpBind as _, UdpSplit as _};
use edge_nal_embassy::UdpError;
use embassy_executor::Spawner;
use embassy_futures::{join::join, yield_now};
use embassy_net::{Config, DhcpConfig, Stack, StackResources};
use embassy_rp::{
    Peri,
    gpio::{Level, Output},
    peripherals::{self, PIN_23, PIO0, TRNG, USB},
    trng::Trng,
    usb::Driver,
};
use embassy_sync::{
    blocking_mutex::raw::{CriticalSectionRawMutex, NoopRawMutex},
    channel::{Channel, Receiver, Sender},
    rwlock::RwLock,
    signal::Signal,
};
use embassy_time::{Duration, Timer};
use embassy_usb::{Builder, UsbDevice, class::cdc_ncm};
use static_cell::StaticCell;

const RAW_FLASH_BASE: u32 = 0x1c000000; // See XIP_NOCACHE_NOALLOC_NOTRANSLATE_BASE
const WIFI_FIRMWARE_FLASH_OFFSET: u32 = 0x00200000;
const WIFI_FIRMWARE_LENGTH: usize = 231077; // Size of `firmware/blobs/43439A0.bin`

type MyDriver = Driver<'static, peripherals::USB>;

const MTU: usize = 1514;

pub async fn init_usb_ethernet(
    spawner: Spawner,
    spi: PioSpi<'static, PIO0, 0>,
    pwr: Peri<'static, PIN_23>,
    driver: embassy_rp::usb::Driver<'static, USB>,
    trng: Trng<'static, TRNG>,
    ethernet_signal: &'static EthernetSignal,
    config_file: LedBoardConfigFile,
    watchdog: SharedWatchdog,
) -> (Stack<'static>, WifiControl) {
    let mut rng = embassy_rp::clocks::RoscRng;
    let seed = rng.next_u64();

    // ======== WiFi stuff (just for LED)

    let firmware_address = RAW_FLASH_BASE + WIFI_FIRMWARE_FLASH_OFFSET;

    info!("Loading WiFi firmware from: 0x{:x}", firmware_address);

    let fw = unsafe { core::slice::from_raw_parts(firmware_address as *const u8, WIFI_FIRMWARE_LENGTH) };
    let fw = unsafe { &*(fw as *const [u8] as *const cyw43::Aligned<cyw43::A4, [u8]>) };
    let nvram = aligned_bytes!("../../blobs/nvram_rp2040.bin");
    let clm = aligned_bytes!("../../blobs/43439A0_clm.bin");

    let pwr_pin = Output::new(pwr, Level::Low);

    Timer::after(Duration::from_millis(1_000)).await;

    info!("SPI configured");

    static STATE: StaticCell<cyw43::State> = StaticCell::new();
    let state = STATE.init(cyw43::State::new());

    let (_wifi_device, mut control, cyw43_runner) = cyw43::new(state, pwr_pin, spi, fw, nvram).await;

    spawner.spawn(cyw43_task(cyw43_runner).unwrap());

    info!("WiFi driver task started");

    control.init(clm).await;

    control.set_power_management(PowerManagementMode::None).await;

    let led_channel = make_static!(Channel<NoopRawMutex, bool, 1>, Channel::<NoopRawMutex, bool, 1>::new());
    let public_control = WifiControl::new(led_channel.sender());

    // ======== USB stuff

    // Create embassy-usb Config
    let mut config = embassy_usb::Config::new(0xc0de, 0xcafe);
    config.manufacturer = Some("Embassy");
    config.product = Some("LED Board");
    config.serial_number = Some("12345678");
    config.max_power = 100;
    config.max_packet_size_0 = 64;

    // Create embassy-usb DeviceBuilder using the driver and config.
    static CONFIG_DESC: StaticCell<[u8; 256]> = StaticCell::new();
    static BOS_DESC: StaticCell<[u8; 256]> = StaticCell::new();
    static CONTROL_BUF: StaticCell<[u8; 128]> = StaticCell::new();
    let mut builder = Builder::new(
        driver,
        config,
        &mut CONFIG_DESC.init([0; 256])[..],
        &mut BOS_DESC.init([0; 256])[..],
        &mut [], // no msos descriptors
        &mut CONTROL_BUF.init([0; 128])[..],
    );

    // Our MAC addr.
    let our_mac_addr = [0xCC, 0xCC, 0xCC, 0xCC, 0xCC, 0xCC];
    // Host's MAC addr. This is the MAC the host "thinks" its USB-to-ethernet adapter has.
    let host_mac_addr = [0x88, 0x88, 0x88, 0x88, 0x88, 0x88];

    // Create classes on the builder.
    static ETHER_STATE: StaticCell<cdc_ncm::State> = StaticCell::new();
    let class = cdc_ncm::CdcNcmClass::new(&mut builder, ETHER_STATE.init(cdc_ncm::State::new()), host_mac_addr, 64);

    // Build the builder.
    let usb = builder.build();

    spawner.spawn(unwrap!(usb_task(usb)));

    // ========

    static NET_STATE: StaticCell<cdc_ncm::embassy_net::State<MTU, 4, 4>> = StaticCell::new();
    let (runner, usb_ethernet_device) =
        class.into_embassy_net_device::<MTU, 4, 4>(NET_STATE.init(cdc_ncm::embassy_net::State::new()), our_mac_addr);

    spawner.spawn(unwrap!(usb_ncm_task(runner)));

    let config = embassy_net::Config::default();

    // Init network stack
    static RESOURCES: StaticCell<StackResources<10>> = StaticCell::new();
    let (stack, runner) = embassy_net::new(usb_ethernet_device, config, RESOURCES.init(StackResources::new()), seed);

    spawner.spawn(unwrap!(net_task(runner)));

    spawner.spawn(
        usb_ethernet_task(
            spawner,
            ethernet_signal,
            stack,
            control,
            led_channel.receiver(),
            config_file,
            watchdog,
        )
        .unwrap(),
    );

    info!("USB Ethernet task started");

    let mut receiver = ethernet_signal.receiver().unwrap();

    if let EthernetSignalMessage::Connected(ip) = receiver.get().await {
        info!("WIFI GOT SIGNAL");
        Timer::after(Duration::from_millis(1_000)).await;

        let task = mdns_task(stack, trng, "led-board".to_string(), ip);

        match task {
            Ok(task) => {
                spawner.spawn(task);
            }
            Err(err) => {
                error!("Could not spawn task: {:?}", err);
            }
        }
    }

    (stack, public_control)
}

#[embassy_executor::task]
async fn usb_task(mut device: UsbDevice<'static, MyDriver>) -> ! {
    device.run().await
}

#[embassy_executor::task]
async fn usb_ncm_task(class: embassy_usb::class::cdc_ncm::embassy_net::Runner<'static, MyDriver, MTU>) -> ! {
    class.run().await
}

#[embassy_executor::task(pool_size = 4)]
async fn cyw43_task(
    runner: cyw43::Runner<'static, cyw43::SpiBus<Output<'static>, PioSpi<'static, PIO0, 0>>, cyw43::Cyw43439>,
) -> ! {
    runner.run().await
}

#[embassy_executor::task]
async fn net_task(
    mut runner: embassy_net::Runner<'static, embassy_usb::class::cdc_ncm::embassy_net::Device<'static, MTU>>,
) -> ! {
    runner.run().await
}

#[embassy_executor::task]
pub async fn usb_ethernet_task(
    spawner: Spawner,
    signal: &'static EthernetSignal,
    stack: Stack<'static>,
    control: Control<'static>,
    receiver: Receiver<'static, NoopRawMutex, bool, 1>,
    config_file: LedBoardConfigFile,
    watchdog: SharedWatchdog,
) {
    let join_timeout = Duration::from_secs(10);
    let dhcp_timeout = Duration::from_secs(10);

    let control: RwLock<CriticalSectionRawMutex, cyw43::Control<'_>> = RwLock::new(control);

    let sender = signal.sender();

    let config = config_file.get_data().await;

    join(
        async {
            if let WifiMode::Station = config.wifi_mode {
                info!("Waiting for DHCP lease...");

                match wait_for_config_timeout(stack, dhcp_timeout).await {
                    Some(cfg) => {
                        let ip = cfg.address.address();
                        info!("Connected! IP Address: {}", ip);
                        sender.send(EthernetSignalMessage::Connected(ip));

                        return; // TODO: We're connected now but we should check that we stay connected
                    }
                    None => {
                        error!("DHCP timeout. No IP address assigned.");
                    }
                };
            }

            watchdog.write().await.feed(Duration::from_secs(15));

            let ip_address = embassy_net::Ipv4Address::new(192, 168, 1, 1);

            let config = Config::ipv4_static(embassy_net::StaticConfigV4 {
                address: embassy_net::Ipv4Cidr::new(ip_address, 16),
                dns_servers: heapless::Vec::new(),
                gateway: None,
            });

            stack.set_config_v4(config.ipv4);

            spawner.spawn(dhcp_task(stack, ip_address).unwrap());

            sender.send(EthernetSignalMessage::Connected(ip_address));
        },
        async {
            loop {
                let state = receiver.receive().await;
                let mut control = control.write().await;
                control.gpio_set(0, state).await;
            }
        },
    )
    .await;
}

async fn wait_for_config_timeout(stack: Stack<'static>, timeout: Duration) -> Option<embassy_net::StaticConfigV4> {
    let start = embassy_time::Instant::now();

    loop {
        if let Some(config) = stack.config_v4() {
            return Some(config.clone());
        }

        if start.elapsed() > timeout {
            return None;
        }

        yield_now().await;
    }
}
