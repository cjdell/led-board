use crate::{
    config::WifiMode,
    make_static,
    tasks::{common::WifiControl, dhcp_task, mdns_task},
    types::{EthernetSignal, EthernetSignalMessage, LedBoardConfigFile, SharedWatchdog},
};
use alloc::{
    format,
    string::{String, ToString as _},
};
use core::net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4};
use cyw43::{Control, JoinOptions, PowerManagementMode, SpiBus, aligned_bytes};
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
    peripherals::{PIN_23, PIO0, TRNG},
    trng::Trng,
};
use embassy_sync::{
    blocking_mutex::raw::{CriticalSectionRawMutex, NoopRawMutex},
    channel::{Channel, Receiver, Sender},
    rwlock::RwLock,
    signal::Signal,
};
use embassy_time::{Duration, Timer};
use static_cell::StaticCell;

const RAW_FLASH_BASE: u32 = 0x1c000000; // See XIP_NOCACHE_NOALLOC_NOTRANSLATE_BASE
const WIFI_FIRMWARE_FLASH_OFFSET: u32 = 0x00200000;
const WIFI_FIRMWARE_LENGTH: usize = 231077; // Size of `firmware/blobs/43439A0.bin`

pub async fn init_wifi(
    spawner: Spawner,
    spi: PioSpi<'static, PIO0, 0>,
    pwr: Peri<'static, PIN_23>,
    trng: Trng<'static, TRNG>,
    ethernet_signal: &'static EthernetSignal,
    config_file: LedBoardConfigFile,
    watchdog: SharedWatchdog,
) -> (Stack<'static>, WifiControl) {
    let mut rng = embassy_rp::clocks::RoscRng;
    let seed = rng.next_u64();

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

    let (net_device, mut control, runner) = cyw43::new(state, pwr_pin, spi, fw, nvram).await;

    spawner.spawn(cyw43_task(runner).unwrap());

    info!("WiFi driver task started");

    control.init(clm).await;

    control.set_power_management(PowerManagementMode::None).await;

    let mut dhcp_config = DhcpConfig::default();
    dhcp_config.hostname = Some(heapless::String::<32>::try_from("led-board").unwrap());

    let config = Config::dhcpv4(dhcp_config);

    static RESOURCES: StaticCell<StackResources<8>> = StaticCell::new();
    let (stack, runner) = embassy_net::new(net_device, config, RESOURCES.init(StackResources::new()), seed);

    spawner.spawn(net_task(runner).unwrap());
    info!("Network stack task started");

    let led_channel = make_static!(Channel<NoopRawMutex, bool, 1>, Channel::<NoopRawMutex, bool, 1>::new());
    let public_control = WifiControl::new(led_channel.sender());

    spawner.spawn(
        wifi_task(
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

    info!("WiFi task started");

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

#[embassy_executor::task(pool_size = 4)]
async fn cyw43_task(
    runner: cyw43::Runner<'static, SpiBus<Output<'static>, PioSpi<'static, PIO0, 0>>, cyw43::Cyw43439>,
) -> ! {
    runner.run().await
}

#[embassy_executor::task]
async fn net_task(mut runner: embassy_net::Runner<'static, cyw43::NetDriver<'static>>) -> ! {
    runner.run().await
}

#[embassy_executor::task]
pub async fn wifi_task(
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
                for network in config.known_wifi_networks {
                    info!("Starting WiFi connection sequence to: {}", network.ssid.as_str());

                    let start = embassy_time::Instant::now();

                    let mut joined = false;

                    loop {
                        watchdog.write().await.feed(Duration::from_secs(15));

                        let mut control = control.write().await;

                        match control
                            .join(&network.ssid, JoinOptions::new(network.pass.as_bytes()))
                            .await
                        {
                            Ok(()) => {
                                info!("Joined WiFi network: {}", network.ssid.as_str());
                                joined = true;
                                break;
                            }
                            Err(e) => {
                                if start.elapsed() > join_timeout {
                                    error!("Failed to join WiFi after {} seconds: {:?}", join_timeout.as_secs(), e);
                                    break;
                                }
                                info!("Join attempt failed: {}, retrying...", e);
                                Timer::after_millis(500).await;
                            }
                        }
                    }

                    if !joined {
                        error!("Failed to connect to WiFi network: {}", network.ssid.as_str());
                        continue;
                    }

                    info!("Waiting for DHCP lease...");

                    let cfg = match wait_for_config_timeout(stack, dhcp_timeout).await {
                        Some(cfg) => cfg,
                        None => {
                            error!("DHCP timeout. No IP address assigned.");
                            continue;
                        }
                    };

                    let ip = cfg.address.address();
                    info!("Connected! IP Address: {}", ip);
                    sender.send(EthernetSignalMessage::Connected(ip));

                    return; // TODO: We're connected now but we should check that we stay connected
                }
            }

            watchdog.write().await.feed(Duration::from_secs(15));

            {
                let mut control = control.write().await;

                control.start_ap_open("LED Board", 5).await;
            }

            let ip_address = embassy_net::Ipv4Address::new(192, 168, 1, 1);

            let config = Config::ipv4_static(embassy_net::StaticConfigV4 {
                address: embassy_net::Ipv4Cidr::new(ip_address, 16),
                dns_servers: heapless::Vec::new(),
                gateway: None,
            });

            stack.set_config_v4(config.ipv4);

            spawner.spawn(dhcp_task(stack, ip_address).unwrap());

            spawner.spawn(captive_task(stack, ip_address).unwrap());

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

#[embassy_executor::task]
pub async fn captive_task(stack: Stack<'static>, ap_ip_address: Ipv4Addr) {
    info!("Captive: Task started");

    loop {
        let udp_buffers: edge_nal_embassy::UdpBuffers<5, 1024, 1024, 5> = edge_nal_embassy::UdpBuffers::new();

        let udp = edge_nal_embassy::Udp::new(stack, &udp_buffers);

        let mut tx_buf = [0; 1500];
        let mut rx_buf = [0; 1500];

        edge_captive::io::run(
            &udp,
            SocketAddr::new(core::net::IpAddr::V4(Ipv4Addr::UNSPECIFIED), 53),
            &mut tx_buf,
            &mut rx_buf,
            ap_ip_address,
            core::time::Duration::from_secs(60),
        )
        .await
        .unwrap();

        info!("Captive: Stopped");
    }
}
