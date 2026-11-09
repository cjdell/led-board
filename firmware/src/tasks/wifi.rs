use crate::{
    config::WifiMode,
    make_static,
    tasks::{common::WifiControl, dhcp_task, mdns_task},
    types::{
        DisplayWorkerMessage, DisplayWorkerSender, EthernetSignal, EthernetSignalMessage, LedBoardConfigFile,
        SharedWatchdog,
    },
};
use animations::AnimationEnum;
use alloc::{
    format,
    string::{String, ToString as _},
    vec::Vec,
};
use core::net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4};
use core::sync::atomic::{AtomicU32, Ordering};
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
    mutex::Mutex,
    rwlock::RwLock,
    signal::Signal,
};
use embassy_time::{Duration, Timer};
use embedded_tools::config::ConfigFileTrait as _;
use static_cell::StaticCell;

const RAW_FLASH_BASE: u32 = 0x1c000000; // See XIP_NOCACHE_NOALLOC_NOTRANSLATE_BASE
const WIFI_FIRMWARE_FLASH_OFFSET: u32 = 0x00200000;
const WIFI_FIRMWARE_LENGTH: usize = 231077; // Size of `firmware/blobs/43439A0.bin`

/// `Control` is `!Sync` (Cell/RefCell internals), but the only access path is
/// through the inner `RwLock`'s guard, and `CriticalSectionRawMutex` is a real
/// cross-core critical section on the RP2351 — simultaneous access is
/// impossible, which is exactly what `Sync` promises. (Same discipline
/// `StaticCell` applies to `cyw43::State` in `init_wifi`.)
struct Shared<T>(T);
// SAFETY: see above — all access is serialized by the inner lock.
unsafe impl<T> Sync for Shared<T> {}

/// Shared cyw43 handle so the provisioning endpoints (`/api/wifi` scan) can use
/// the radio without a C3 relay. `wifi_task` parks it here while idle and
/// borrows it for each join attempt, so contention is a scan racing a join —
/// which the provisioning flow (AP fallback, nobody joining) never creates.
pub static WIFI_CONTROL: Shared<RwLock<CriticalSectionRawMutex, Option<Control<'static>>>> =
    Shared(RwLock::new(None));

/// Native scan for the wifi build's `/api/wifi`: dedupe by SSID (strongest
/// signal wins), sorted by RSSI, capped to keep the JSON inside the web-task
/// TX buffer. `None` if the radio never initialized.
pub async fn wifi_scan() -> Option<Vec<(String, i8, bool)>> {
    let mut guard = WIFI_CONTROL.0.write().await;
    let control = guard.as_mut()?;

    let mut rows: Vec<(String, i8, bool)> = Vec::new();
    let mut scanner = control.scan(cyw43::ScanOptions::default()).await;
    while let Some(bss) = scanner.next().await {
        let len = (bss.ssid_len as usize).min(bss.ssid.len());
        let ssid = String::from_utf8_lossy(&bss.ssid[..len]).into_owned();
        if ssid.is_empty() {
            continue;
        }
        let rssi = bss.rssi.clamp(i16::from(i8::MIN), i16::from(i8::MAX)) as i8;
        let password_required = bss.capability & 0x0010 != 0;
        match rows.iter_mut().find(|r| r.0 == ssid) {
            Some(existing) => existing.1 = existing.1.max(rssi),
            None => rows.push((ssid, rssi, password_required)),
        }
    }
    rows.sort_by(|a, b| b.1.cmp(&a.1));
    rows.truncate(12);
    Some(rows)
}

/// What the last join attempt did, exposed via `GET /api/join-status`.
/// Provisioning from a phone is a blind window — there is no host-side log
/// until the board is on the LAN — so the board must report its own failure.
#[derive(Default)]
struct JoinStatus {
    last_error: Option<String>,
    joined_ssid: Option<String>,
    ip: Option<String>,
    ap_fallback: bool,
}

static JOIN_ATTEMPTS: AtomicU32 = AtomicU32::new(0);
static JOIN_STATUS: Mutex<CriticalSectionRawMutex, JoinStatus> = Mutex::new(JoinStatus {
    last_error: None,
    joined_ssid: None,
    ip: None,
    ap_fallback: false,
});

pub async fn join_status_json() -> String {
    let status = JOIN_STATUS.lock().await;
    serde_json::json!({
        "attempts": JOIN_ATTEMPTS.load(Ordering::Relaxed),
        "last_error": status.last_error.clone(),
        "joined_ssid": status.joined_ssid.clone(),
        "ip": status.ip.clone(),
        "ap_fallback": status.ap_fallback,
    })
    .to_string()
}

pub async fn init_wifi(
    spawner: Spawner,
    spi: PioSpi<'static, PIO0, 0>,
    pwr: Peri<'static, PIN_23>,
    trng: Trng<'static, TRNG>,
    ethernet_signal: &'static EthernetSignal,
    config_file: LedBoardConfigFile,
    watchdog: SharedWatchdog,
    display: DisplayWorkerSender,
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

    info!("SPI configured (gSPI PIO)");

    crate::tasks::usb_dbg::stage(16, "wifi: loading cyw43 firmware");

    static STATE: StaticCell<cyw43::State> = StaticCell::new();
    let state = STATE.init(cyw43::State::new());

    let (net_device, mut control, runner) = cyw43::new(state, pwr_pin, spi, fw, nvram).await;

    spawner.spawn(cyw43_task(runner).unwrap());

    info!("WiFi driver task started");
    crate::tasks::usb_dbg::stage(17, "wifi: cyw43 driver up, init clm");

    control.init(clm).await;
    info!("WiFi: clm init done");

    control.set_power_management(PowerManagementMode::None).await;
    info!("WiFi: power management off (max throughput)");

    let mut dhcp_config = DhcpConfig::default();
    dhcp_config.hostname = Some(heapless::String::<32>::try_from("led-board").unwrap());

    let config = Config::dhcpv4(dhcp_config);

    // Socket budget: 5 web listeners + mdns + dhcp + defmt-TCP + mDNS
    // responder sockets + a browser's parallel connections (6 in practice).
    // 10 was too tight: the 4-way burst test had connections refused because
    // every slot was taken by a listener.
    static RESOURCES: StaticCell<StackResources<16>> = StaticCell::new();
    let (stack, runner) = embassy_net::new(net_device, config, RESOURCES.init(StackResources::new()), seed);

    spawner.spawn(net_task(runner).unwrap());
    info!("Network stack task started (DHCP)");
    crate::tasks::usb_dbg::stage(18, "wifi: net stack running");

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
            display,
        )
        .unwrap(),
    );

    info!("WiFi task started (join loop on its own task)");

    // Do NOT park main here: the web app, playlist and heartbeat must come up
    // whatever the network is doing (venue joins can wander for minutes).
    // mDNS spawns itself the moment the first lease/fallback IP lands.
    match network_ready_task(spawner, stack, trng, ethernet_signal) {
        Ok(task) => {
            spawner.spawn(task);
        }
        Err(_) => {
            error!("could not spawn network_ready_task (pool full?)");
        }
    }

    (stack, public_control)
}

#[embassy_executor::task]
async fn network_ready_task(
    spawner: Spawner,
    stack: Stack<'static>,
    trng: Trng<'static, TRNG>,
    ethernet_signal: &'static EthernetSignal,
) {
    let mut receiver = ethernet_signal.receiver().unwrap();

    if let EthernetSignalMessage::Connected(ip) = receiver.get().await {
        info!("WIFI GOT SIGNAL — spawning mDNS");
        crate::tasks::usb_dbg::stage(19, "net: first IPv4 lease");
        crate::tasks::usb_dbg::log(format_args!("net: first signal — mdns up ({ip})"));
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
    display: DisplayWorkerSender,
) {
    let join_timeout = Duration::from_secs(10);
    let dhcp_timeout = Duration::from_secs(15);

    // Park the handle in the shared static; every use below borrows it from
    // there so the http provisioning handlers can reach it too.
    *WIFI_CONTROL.0.write().await = Some(control);

    let sender = signal.sender();

    let config = config_file.get_data().await;

    join(
        async {
            if let WifiMode::Station = config.wifi_mode {
                let mut has_connected_once = false;

                'station: loop {
                    for network in &config.known_wifi_networks {
                        info!("Starting WiFi connection sequence to: {}", network.ssid.as_str());
                        // Console breadcrumb: visible over USB even when the
                        // network (and thus defmt-TCP) is dead — this is the
                        // venue join-forensics channel.
                        crate::tasks::usb_dbg::log(format_args!(
                            "wifi: joining \"{}\"...",
                            network.ssid.as_str()
                        ));

                        let start = embassy_time::Instant::now();

                        let mut joined = false;
                        let mut attempts = 0u32;

                        let short_ssid: String = network.ssid.chars().take(10).collect();
                        display
                            .try_send(DisplayWorkerMessage::Status(AnimationEnum::CenteredTitle(
                                "wifi".to_string(),
                                format!("{short_ssid}?"),
                            )))
                            .ok();

                        loop {
                            attempts += 1;

                            watchdog.write().await.feed(Duration::from_secs(15));

                            let mut control_guard = WIFI_CONTROL.0.write().await;
                            let control = control_guard.as_mut().unwrap();

                            match control
                                .join(&network.ssid, JoinOptions::new(network.pass.as_bytes()))
                                .await
                            {
                                Ok(()) => {
                                    info!("Joined WiFi network: {}", network.ssid.as_str());
                                    crate::tasks::usb_dbg::log(format_args!(
                                        "wifi: joined \"{}\" after {} attempt(s)",
                                        network.ssid.as_str(),
                                        attempts
                                    ));
                                    display
                                        .try_send(DisplayWorkerMessage::Status(AnimationEnum::CenteredTitle(
                                            "wifi".to_string(),
                                            "leased?".to_string(),
                                        )))
                                        .ok();
                                    let mut status = JOIN_STATUS.lock().await;
                                    status.joined_ssid = Some(network.ssid.clone());
                                    status.last_error = None;
                                    drop(status);
                                    joined = true;
                                    break;
                                }
                                Err(e) => {
                                    JOIN_STATUS.lock().await.last_error = Some(format!("{e:?}"));
                                    if start.elapsed() > join_timeout {
                                        JOIN_ATTEMPTS.fetch_add(1, Ordering::Relaxed);
                                        error!("Failed to join WiFi after {} seconds: {:?}", join_timeout.as_secs(), e);
                                        crate::tasks::usb_dbg::log(format_args!(
                                            "wifi: GIVE UP \"{}\" after {} attempt(s): {:?}",
                                            network.ssid.as_str(),
                                            attempts,
                                            e
                                        ));
                                        break;
                                    }
                                    info!("Join attempt failed: {}, retrying...", e);
                                    crate::tasks::usb_dbg::log(format_args!(
                                        "wifi: attempt {attempts} failed: {e:?}"
                                    ));
                                    Timer::after_millis(500).await;
                                }
                            }
                        }

                        if !joined {
                            error!("Failed to connect to WiFi network: {}", network.ssid.as_str());
                            continue;
                        }

                        info!("Waiting for DHCP lease...");

                        // Crowded-venue APs (Leighhack) associate fine but can
                        // take >10 s to hand out a lease. The old single 10 s
                        // attempt dropped a healthy association and churned to
                        // the next network — keep the association and give
                        // DHCP three rounds of 15 s before walking away.
                        let mut lease = None;
                        for round in 1..=3u32 {
                            match wait_for_config_timeout(stack, dhcp_timeout).await {
                                Some(cfg) => {
                                    lease = Some(cfg);
                                    break;
                                }
                                None => {
                                    error!("DHCP timeout (round {}/3), staying associated...", round);
                                    crate::tasks::usb_dbg::log(format_args!(
                                        "wifi: DHCP timeout, round {round}/3 (staying on)"
                                    ));
                                    display
                                        .try_send(DisplayWorkerMessage::Status(AnimationEnum::StatusCard(
                                            "wifi".to_string(),
                                            format!("no lease {round}/3"),
                                        )))
                                        .ok();
                                    JOIN_STATUS.lock().await.last_error =
                                        Some(format!("DHCP timeout (round {round})"));
                                }
                            }
                        }

                        let cfg = match lease {
                            Some(cfg) => cfg,
                            None => {
                                error!("DHCP timeout. No IP address assigned.");
                                crate::tasks::usb_dbg::log(format_args!(
                                    "wifi: DHCP dead after 3 rounds, leaving \"{}\"",
                                    network.ssid.as_str()
                                ));
                                JOIN_STATUS.lock().await.last_error = Some("DHCP timeout".to_string());
                                continue;
                            }
                        };

                        let ip = cfg.address.address();
                        info!("Connected! IP Address: {}", ip);
                        crate::tasks::usb_dbg::log(format_args!("wifi: connected, ip {ip}"));
                        display.try_send(DisplayWorkerMessage::StatusClear).ok();
                        JOIN_STATUS.lock().await.ip = Some(format!("{}", ip));
                        sender.send(EthernetSignalMessage::Connected(ip));

                        has_connected_once = true;

                        // Supervise the link. cyw43 reports LinkState::Down on deauth (AP-initiated
                        // kicks on congested venue networks), which takes the stack link/config down.
                        // If that happens, rejoin instead of sitting dead forever.
                        loop {
                            Timer::after(Duration::from_secs(2)).await;

                            watchdog.write().await.feed(Duration::from_secs(15));

                            if stack.is_link_up() && stack.is_config_up() {
                                continue;
                            }

                            warn!("WiFi: link lost, starting reconnection sequence...");
                            crate::tasks::usb_dbg::log(format_args!("wifi: link lost — reconnecting"));
                            display
                                .try_send(DisplayWorkerMessage::Status(AnimationEnum::StatusCard(
                                    "wifi".to_string(),
                                    "link lost".to_string(),
                                )))
                                .ok();
                            break;
                        }

                        continue 'station;
                    }

                    if has_connected_once {
                        // Dropped mid-session and nothing joined this pass (e.g. AP rebooting):
                        // keep retrying station mode forever instead of falling back to AP.
                        Timer::after(Duration::from_secs(5)).await;
                        continue 'station;
                    }

                    break 'station;
                }
            }

            watchdog.write().await.feed(Duration::from_secs(15));

            crate::tasks::usb_dbg::log(format_args!("wifi: starting AP fallback (cy43 start_ap_open)..."));
            display
                .try_send(DisplayWorkerMessage::Status(AnimationEnum::StatusCard(
                    "wifi".to_string(),
                    "AP mode...".to_string(),
                )))
                .ok();

            {
                let mut control_guard = WIFI_CONTROL.0.write().await;
                let control = control_guard.as_mut().unwrap();

                control.start_ap_open("LED Board", 5).await;
            }

            crate::tasks::usb_dbg::log(format_args!("wifi: AP fallback \"LED Board\" up (192.168.1.1)"));
            display
                .try_send(DisplayWorkerMessage::Status(AnimationEnum::StatusCard(
                    "AP: LED Board".to_string(),
                    "192.168.1.1".to_string(),
                )))
                .ok();

            JOIN_STATUS.lock().await.ap_fallback = true;

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
                let mut control_guard = WIFI_CONTROL.0.write().await;
                let control = control_guard.as_mut().unwrap();
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
