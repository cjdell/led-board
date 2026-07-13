use crate::{
    config::WifiMode,
    make_static,
    tasks::common::WifiControl,
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
    static RESOURCES: StaticCell<StackResources<8>> = StaticCell::new();
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

        let task = mdns_task(stack, trng, "mypico2w".to_string(), ip);

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

#[embassy_executor::task]
pub async fn mdns_task(stack: Stack<'static>, trng: Trng<'static, TRNG>, our_name: String, our_ip: Ipv4Addr) {
    info!("mdns_task");

    Timer::after(Duration::from_millis(1_000)).await;

    mdns_runner(stack, trng, &our_name, our_ip)
        .await
        .expect("mdns_runner failed");
}

async fn mdns_runner(
    stack: Stack<'static>,
    trng: Trng<'static, TRNG>,
    our_name: &str,
    our_ip: Ipv4Addr,
) -> Result<(), MdnsIoError<UdpError>> {
    // info!("About to run an mDNS responder for our PC. It will be addressable using {our_name}.local, so try to `ping {our_name}.local`.");

    info!("mdns_runner: Creating stack buffers...");
    let udp_buffers: edge_nal_embassy::UdpBuffers<5, 1024, 1024, 5> = edge_nal_embassy::UdpBuffers::new();

    info!("mdns_runner: Creating UDP stack...");
    let udp = edge_nal_embassy::Udp::new(stack, &udp_buffers);

    info!("mdns_runner: Creating buffers...");
    let (recv_buf, send_buf) = (
        VecBufAccess::<NoopRawMutex, 1500>::new(),
        VecBufAccess::<NoopRawMutex, 1500>::new(),
    );

    info!("mdns_runner: Creating socket...");
    let mut socket = edge_mdns::io::bind(&udp, DEFAULT_SOCKET, Some(Ipv4Addr::UNSPECIFIED), Some(0)).await?;

    let (recv, send) = socket.split();

    info!("mdns_runner: Creating host...");
    let host = Host {
        hostname: our_name,
        ipv4: our_ip, //Ipv4Addr::new(192, 168, 49, 39),
        ipv6: Ipv6Addr::UNSPECIFIED,
        ttl: Ttl::from_secs(60),
    };

    // A way to notify the mDNS responder that the data in `Host` had changed
    // We don't use it in this example, because the data is hard-coded
    let signal = Signal::<NoopRawMutex, _>::new();

    info!("mdns_runner: Creating Mdns...");
    let mdns = edge_mdns::io::Mdns::new(
        Some(Ipv4Addr::UNSPECIFIED),
        Some(0),
        recv,
        send,
        recv_buf,
        send_buf,
        trng,
        &signal,
    );

    info!("mdns_runner: Running Mdns...");
    mdns.run(HostAnswersMdnsHandler::new(&host)).await?;

    info!("mdns_runner: Finished");
    Ok(())
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

#[embassy_executor::task]
pub async fn dhcp_task(stack: Stack<'static>, ap_ip_address: Ipv4Addr) {
    info!("DHCP: Task started");

    let mut buf = [0u8; 1500];

    let mut gw_buf = [Ipv4Addr::UNSPECIFIED];
    let dns = [ap_ip_address];

    let buffers = edge_nal_embassy::UdpBuffers::<3, 1024, 1024, 10>::new();
    let unbound_socket = edge_nal_embassy::Udp::new(stack, &buffers);
    let mut bound_socket = unbound_socket
        .bind(SocketAddr::V4(SocketAddrV4::new(
            Ipv4Addr::UNSPECIFIED,
            edge_dhcp::io::DEFAULT_SERVER_PORT,
        )))
        .await
        .unwrap();

    loop {
        let captive_url = format!("http://{}/", ap_ip_address);

        let mut options = edge_dhcp::server::ServerOptions::new(ap_ip_address, Some(&mut gw_buf));
        options.dns = &dns;
        options.captive_url = Some(&captive_url);

        if let Err(err) = edge_dhcp::io::server::run(
            &mut edge_dhcp::server::Server::<_, 64>::new_with_et(ap_ip_address),
            &options,
            &mut bound_socket,
            &mut buf,
        )
        .await
        {
            warn!("DHCP: Server error: {:?}", defmt::Debug2Format(&err));
        }

        Timer::after(Duration::from_millis(500)).await;
        info!("DHCP: Offered IP address");
    }
}
