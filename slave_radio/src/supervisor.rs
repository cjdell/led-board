//! WiFi supervisor: owns the radio controller and runs the connection policy.
//!
//! * Station mode: scan, join the strongest of the known networks; when the
//!   join fails or the connection dies, fall back to an open AP + captive
//!   portal so the board stays reachable and re-provisionable.
//! * AccessPoint mode: just run the AP.
//! * ApStation mode (portal provisioning): AP up while attempting a station
//!   join with freshly supplied credentials.
//!
//! Everything the Pico asks for (scan/join/forget/set-networks) flows in
//! through [`crate::link`]'s RPC task and lands here via the command channel,
//! because only this task may touch the controller.

use alloc::{string::String, vec::Vec};

use embassy_executor::Spawner;
use embassy_net::Stack;
use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex,
    channel::Channel,
    signal::Signal,
};
use embassy_time::{Duration, Instant, Timer};
use esp_radio::wifi::{
    ap::AccessPointConfig,
    scan::ScanConfig,
    sta::StationConfig,
    AuthenticationMethod, Config, WifiController,
};

use radio_link::{Network, RadioEvent, RadioMode, Reply, ScanEntry};

use crate::state::{self, MDNS_NAME};
use crate::store::SharedStore;

/// Retry cadence while a station join keeps failing.
const RETRY_INTERVAL: Duration = Duration::from_secs(60);
/// How often a "connected" station is health-checked via DNS.
const CONNECTIVITY_INTERVAL: Duration = Duration::from_secs(30);

/// Commands into the supervisor (all controller work is serialized here).
pub(crate) enum Cmd {
    /// Re-evaluate the connection state immediately.
    Reevaluate,
    /// Remember this network and try it now.
    Join { ssid: String, pass: String },
    /// Run a fresh scan and reply on SCAN_REPLY.
    Scan,
}

static CMDS: Channel<CriticalSectionRawMutex, Cmd, 8> = Channel::new();
static SCAN_REPLY: Signal<CriticalSectionRawMutex, Vec<ScanEntry>> = Signal::new();

/// Enqueue a command for the supervisor.
pub(crate) async fn send_cmd(cmd: Cmd) {
    CMDS.send(cmd).await;
}

// ---------------------------------------------------------------------------
// Public API (used by the RPC server and the portal)
// ---------------------------------------------------------------------------

/// Ask for a fresh scan; falls back to the cache on timeout.
pub async fn request_scan() -> Vec<ScanEntry> {
    SCAN_REPLY.reset();
    send_cmd(Cmd::Scan).await;

    match embassy_time::with_timeout(Duration::from_secs(20), SCAN_REPLY.wait()).await {
        Ok(entries) => entries,
        Err(_) => {
            log::warn!("supervisor: scan request timed out, serving cache");
            state::SCAN_CACHE.read().await.clone()
        }
    }
}

pub async fn join_network(ssid: &str, pass: &str) {
    send_cmd(Cmd::Join {
        ssid: String::from(ssid),
        pass: String::from(pass),
    })
    .await;
}

pub async fn forget_network(ssid: &str) {
    let store = crate::httpd::store();
    let mut store = store.lock().await;
    match store.forget(ssid) {
        Ok(_) => drop(store),
        Err(err) => {
            log::error!("supervisor: forget save failed {:?}", err);
            drop(store);
        }
    }
    send_cmd(Cmd::Reevaluate).await;
}

pub async fn apply_networks(networks: Vec<Network>, ap_ssid: String, mode: RadioMode) {
    let store = crate::httpd::store();
    {
        let mut store = store.lock().await;
        if let Err(err) = store.set(networks, ap_ssid, mode) {
            log::error!("supervisor: set-networks save failed {:?}", err);
        }
    }
    send_cmd(Cmd::Reevaluate).await;
}

pub async fn stored_networks() -> Reply {
    let store = crate::httpd::store();
    let mut store = store.lock().await;
    let cfg = store.load();
    Reply::Networks(cfg.networks, cfg.ap_ssid, cfg.mode)
}

pub async fn use_station() {
    let store = crate::httpd::store();
    {
        let mut store = store.lock().await;
        if let Err(err) = store.set_mode(RadioMode::Station) {
            log::error!("supervisor: mode save failed {:?}", err);
        }
    }
    send_cmd(Cmd::Reevaluate).await;
}

// ---------------------------------------------------------------------------
// The supervisor task
// ---------------------------------------------------------------------------

pub struct SupervisorInit {
    pub controller: WifiController<'static>,
    pub sta_stack: Stack<'static>,
    pub ap_stack: Stack<'static>,
    pub store: &'static SharedStore,
    pub spawner: Spawner,
}

#[embassy_executor::task]
async fn supervisor(init: SupervisorInit) {
    let SupervisorInit {
        mut controller,
        sta_stack,
        ap_stack,
        store,
        spawner,
    } = init;

    log::info!("supervisor: started");

    let mut ap_up = false;
    let mut joined: Option<(String, [u8; 4])> = None;
    let mut last_dns_check = Instant::from_ticks(0);
    let mut retry_at = Instant::from_ticks(0);
    let mut pending_join: Option<(String, String)> = None;
    let mut ap_restart_needed = true;
    let mut hb: u32 = 0;

    loop {
        Timer::after_millis(1000).await;

        // ---- drain commands --------------------------------------------
        while let Ok(cmd) = CMDS.try_receive() {
            match cmd {
                Cmd::Reevaluate => {
                    retry_at = Instant::now();
                    // In dedicated AP mode, config pushes may change the SSID.
                    ap_restart_needed = true;
                }
                Cmd::Join { ssid, pass } => {
                    let mut store = store.lock().await;
                    if let Err(err) = store.upsert_network(&ssid, &pass) {
                        log::error!("supervisor: join save failed {:?}", err);
                    }
                    pending_join = Some((ssid, pass));
                    retry_at = Instant::now();
                }
                Cmd::Scan => {
                    let entries = do_scan(&mut controller).await;
                    state::store_scan(entries.clone()).await;
                    SCAN_REPLY.signal(entries);
                }
            }
        }

        let cfg = {
            let mut store = store.lock().await;
            store.load()
        };

        // ---- heartbeat (periodic live state for serial monitoring) ------
        hb = hb.wrapping_add(1);
        if hb % 5 == 0 {
            log::info!(
                "hb: mode={:?} ap_up={} joined={} pico_online={}",
                cfg.mode,
                ap_up,
                joined.is_some(),
                state::pico_online(),
            );
        }

        // ---- mode handling ----------------------------------------------
        match cfg.mode {
            RadioMode::AccessPoint => {
                if ap_restart_needed {
                    let ap_cfg = AccessPointConfig::default()
                        .with_ssid(cfg.ap_ssid.clone())
                        .with_auth_method(AuthenticationMethod::Wpa2Personal)
                        .with_password("ledboard123".into());
                    if let Err(err) = controller.set_config(&Config::AccessPoint(ap_cfg)) {
                        log::error!("supervisor: AP config failed: {:?}", err);
                    } else {
                        ap_up = true;
                        ap_restart_needed = false;
                        log::info!("supervisor: ap channel={:?}", controller.channel());
                        start_ap_stack(&ap_stack).await;
                        publish(RadioMode::AccessPoint, None, None, Some(cfg.ap_ssid.clone()), true).await;
                        crate::link::push_event(RadioEvent::ApStarted {
                            ssid: cfg.ap_ssid.clone(),
                            ip: state::ap_ip_octets(),
                        })
                        .await;
                        spawn_mdns(&spawner, &ap_stack, state::AP_IP);

                        // ---- DIAG (temporary): prove our own beacon is on the air ----
                        log::info!("diag: self-scan via APSTA combo");
                        let combo = Config::AccessPointStation(
                            StationConfig::default(),
                            AccessPointConfig::default()
                                .with_ssid(cfg.ap_ssid.clone())
                                .with_auth_method(AuthenticationMethod::Wpa2Personal)
                                .with_password("ledboard123".into()),
                        );
                        match controller.set_config(&combo) {
                            Ok(()) => {
                                Timer::after_millis(1500).await;
                                match controller.scan_async(&ScanConfig::default()).await {
                                    Ok(found) => {
                                        let entries = to_scan_entries(&found);
                                        if entries.is_empty() {
                                            log::warn!("diag scan: EMPTY");
                                        }
                                        for e in entries.iter().take(12) {
                                            log::info!(
                                                "diag scan: '{}' rssi={}",
                                                e.ssid,
                                                e.signal_strength
                                            );
                                        }
                                    }
                                    Err(err) => log::warn!("diag scan err: {:?}", err),
                                }
                            }
                            Err(err) => log::warn!("diag combo err: {:?}", err),
                        }
                    }
                }
            }
            RadioMode::Station | RadioMode::ApStation => {
                // If we're the AP-only mode's successor, (re)join station.
                if !controller.is_connected() {
                    if ap_up && matches!(cfg.mode, RadioMode::Station) && joined.is_some() {
                        // Was up, fell over: report and start fallback below.
                        joined = None;
                    }

                    if Instant::now() >= retry_at {
                        let outcome = attempt_station(&mut controller, &sta_stack, &cfg, pending_join.take(), ap_up).await;

                        match outcome {
                            JoinOutcome::Joined(ssid, ip) => {
                                joined = Some((ssid.clone(), ip));
                                retry_at = Instant::now() + CONNECTIVITY_INTERVAL;
                                last_dns_check = Instant::now();

                                if ap_up {
                                    // Keep the AP up quietly; report as station.
                                    publish(RadioMode::Station, Some(ssid.clone()), Some(ip), Some(cfg.ap_ssid.clone()), true).await;
                                } else {
                                    publish(RadioMode::Station, Some(ssid.clone()), Some(ip), None, false).await;
                                }

                                crate::link::push_event(RadioEvent::Joined { ssid, ip }).await;
                                spawn_mdns(&spawner, &sta_stack, ip.into());
                            }
                            JoinOutcome::Failed { no_networks } => {
                                retry_at = Instant::now() + RETRY_INTERVAL;

                                if !ap_up {
                                    // Fallback: open provisioning network.
                                    let ap_cfg = AccessPointConfig::default()
                                        .with_ssid(cfg.ap_ssid.clone())
                                        .with_auth_method(AuthenticationMethod::Wpa2Personal)
                                        .with_password("ledboard123".into());
                                    let combo = Config::AccessPointStation(StationConfig::default(), ap_cfg);
                                    match controller.set_config(&combo) {
                                        Ok(()) => {
                                            ap_up = true;
                                            start_ap_stack(&ap_stack).await;
                                            publish(RadioMode::AccessPoint, None, None, Some(cfg.ap_ssid.clone()), true).await;
                                            crate::link::push_event(RadioEvent::ApStarted {
                                                ssid: cfg.ap_ssid.clone(),
                                                ip: state::ap_ip_octets(),
                                            })
                                            .await;
                                            spawn_mdns(&spawner, &ap_stack, state::AP_IP);
                                        }
                                        Err(err) => log::error!("supervisor: fallback AP failed: {:?}", err),
                                    }
                                } else if no_networks {
                                    publish(RadioMode::AccessPoint, None, None, Some(cfg.ap_ssid.clone()), true).await;
                                }

                                if ap_up {
                                    // AP combo config replaces whatever was set.
                                    ap_restart_needed = false;
                                }
                            }
                        }
                    }
                } else {
                    // Connected: health check periodically.
                    if Instant::now() - last_dns_check >= CONNECTIVITY_INTERVAL {
                        last_dns_check = Instant::now();

                        if !check_connectivity(&sta_stack).await {
                            log::warn!("supervisor: connectivity lost");
                            let _ = controller.disconnect_async().await;
                            joined = None;
                            retry_at = Instant::now() + Duration::from_secs(15);
                            crate::link::push_event(RadioEvent::Interrupted).await;
                        } else if let Some((ssid, ip)) = joined.clone() {
                            publish(
                                RadioMode::Station,
                                Some(ssid),
                                Some(ip),
                                if ap_up { Some(cfg.ap_ssid.clone()) } else { None },
                                ap_up,
                            )
                            .await;
                        }
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

enum JoinOutcome {
    Joined(String, [u8; 4]),
    Failed { no_networks: bool },
}

async fn attempt_station(
    controller: &mut WifiController<'static>,
    sta_stack: &Stack<'static>,
    cfg: &crate::store::StoredConfig,
    pending: Option<(String, String)>,
    ap_up: bool,
) -> JoinOutcome {
    // Pick target credentials: portal join wins, else best scan match.
    let target = match pending {
        Some((ssid, pass)) => Some((ssid, pass)),
        None => {
            if cfg.networks.is_empty() {
                return JoinOutcome::Failed { no_networks: true };
            }
            best_network(controller, &cfg.networks).await
        }
    };

    let Some((ssid, pass)) = target else {
        // Scan missed every saved network; that is NOT "no configured networks".
        return JoinOutcome::Failed { no_networks: false };
    };

    let mut sta_cfg = StationConfig::default().with_ssid(ssid.clone());
    if !pass.is_empty() {
        sta_cfg = sta_cfg.with_password(pass.clone());
    }

    let config = if ap_up {
        let ap_cfg = AccessPointConfig::default()
            .with_ssid(cfg.ap_ssid.clone())
            .with_auth_method(AuthenticationMethod::Wpa2Personal)
            .with_password("ledboard123".into());
        Config::AccessPointStation(sta_cfg, ap_cfg)
    } else {
        Config::Station(sta_cfg)
    };

    if let Err(err) = controller.set_config(&config) {
        log::error!("supervisor: station config failed: {:?}", err);
        return JoinOutcome::Failed { no_networks: false };
    }

    log::info!("supervisor: connecting to {}", ssid);

    match controller.connect_async().await {
        Ok(_info) => {
            // Wait for DHCP.
            sta_stack.wait_link_up().await;

            for _ in 0..100 {
                if let Some(info) = sta_stack.config_v4() {
                    let address = info.address.address();
                    log::info!("supervisor: connected {} as {}", ssid, address);
                    return JoinOutcome::Joined(ssid, address.octets());
                }
                Timer::after_millis(200).await;
            }

            log::warn!("supervisor: joined but no DHCP lease");
            JoinOutcome::Failed { no_networks: false }
        }
        Err(err) => {
            log::warn!("supervisor: join {} failed: {:?}", ssid, err);
            JoinOutcome::Failed { no_networks: false }
        }
    }
}

/// Scan up to 3 times and return the strongest known network (deduped).
async fn best_network(controller: &mut WifiController<'static>, networks: &[Network]) -> Option<(String, String)> {
    let mut best: Option<(String, String, i8)> = None;

    for attempt in 0..3 {
        match controller.scan_async(&ScanConfig::default()).await {
            Ok(found) => {
                let entries = to_scan_entries(&found);
                // ---- DIAG (temporary): raw scan visibility for antenna testing ----
                log::info!("diag scan[{}]: {} aps", attempt, found.len());
                for ap in found.iter().take(8) {
                    log::info!(
                        "diag scan[{}]: '{}' rssi={}",
                        attempt,
                        String::from(ap.ssid.as_str()),
                        ap.signal_strength
                    );
                }
                // ---- end DIAG ----
                state::store_scan(entries).await;

                for ap in &found {
                    let ssid = String::from(ap.ssid.as_str());
                    if ssid.is_empty() {
                        continue;
                    }
                    if let Some(known) = networks.iter().find(|n| n.ssid == ssid) {
                        let strength = signal_of(&found, &ssid);
                        match &best {
                            Some((_, _, b)) if strength <= *b => {}
                            _ => best = Some((known.ssid.clone(), known.pass.clone(), strength)),
                        }
                    }
                }
            }
            Err(err) => log::warn!("supervisor: scan error: {:?}", err),
        }

        if best.is_some() && attempt == 0 {
            break;
        }
        Timer::after_millis(500).await;
    }

    // ---- DIAG (temporary): outcome of network selection ----
    match &best {
        Some((ssid, _, rssi)) => log::info!("diag best: '{}' rssi={}", ssid, rssi),
        None => log::info!("diag best: NO MATCH (no saved network in scan)"),
    }
    // ---- end DIAG ----
    best.map(|(ssid, pass, _)| (ssid, pass))
}

fn signal_of(found: &[esp_radio::wifi::ap::AccessPointInfo], ssid: &str) -> i8 {
    found
        .iter()
        .filter(|ap| ap.ssid.as_str() == ssid)
        .map(|ap| ap.signal_strength)
        .max()
        .unwrap_or(i8::MIN)
}

async fn do_scan(controller: &mut WifiController<'static>) -> Vec<ScanEntry> {
    match controller.scan_async(&ScanConfig::default()).await {
        Ok(found) => {
            let entries = to_scan_entries(&found);
            state::store_scan(entries.clone()).await;
            entries
        }
        Err(err) => {
            log::warn!("supervisor: on-demand scan failed: {:?}", err);
            state::SCAN_CACHE.read().await.clone()
        }
    }
}

/// Dedupe by SSID, strongest first, with the UI's expected field shapes.
fn to_scan_entries(found: &[esp_radio::wifi::ap::AccessPointInfo]) -> Vec<ScanEntry> {
    let mut entries: Vec<ScanEntry> = Vec::new();

    for ap in found {
        let ssid = ap.ssid.as_str();
        if ssid.is_empty() {
            continue;
        }

        let password_required = !matches!(ap.auth_method, None | Some(AuthenticationMethod::None));

        match entries.iter_mut().find(|e| e.ssid == ssid) {
            Some(existing) => {
                if ap.signal_strength > existing.signal_strength {
                    existing.signal_strength = ap.signal_strength;
                    existing.password_required = password_required;
                }
            }
            None => entries.push(ScanEntry {
                ssid: String::from(ssid),
                signal_strength: ap.signal_strength,
                password_required,
            }),
        }
    }

    entries.sort_by_key(|e| core::cmp::Reverse(e.signal_strength));
    entries.truncate(24);
    entries
}

async fn check_connectivity(stack: &Stack<'static>) -> bool {
    for _ in 0..5 {
        if embassy_time::with_timeout(
            Duration::from_secs(1),
            stack.dns_query("google.com", embassy_net::dns::DnsQueryType::A),
        )
        .await
        .is_ok()
        {
            return true;
        }
        Timer::after_millis(500).await;
    }
    false
}

async fn start_ap_stack(ap_stack: &Stack<'static>) {
    ap_stack.set_config_v4(embassy_net::ConfigV4::Static(embassy_net::StaticConfigV4 {
        address: embassy_net::Ipv4Cidr::new(state::AP_IP, state::AP_IP_CIDR),
        gateway: Some(state::AP_IP),
        dns_servers: Default::default(),
    }));

    ap_stack.wait_config_up().await;
    log::info!("supervisor: AP stack up on {}", state::AP_IP);
}

fn spawn_mdns(spawner: &Spawner, stack: &Stack<'static>, ip: core::net::Ipv4Addr) {
    if let Ok(token) = crate::mdns::mdns_task(*stack, String::from(MDNS_NAME), ip) {
        spawner.spawn(token);
    }
}

async fn publish(
    mode: RadioMode,
    ssid: Option<String>,
    ip: Option<[u8; 4]>,
    ap_ssid: Option<String>,
    ap_up: bool,
) {
    state::publish(state::build_status(
        mode,
        ssid,
        ip,
        None,
        ap_ssid,
        if ap_up { Some(state::ap_ip_octets()) } else { None },
    ))
    .await;
}

/// Public handle for the supervisor task (spawned from main).
pub fn start(spawner: &Spawner, init: SupervisorInit) {
    spawner.spawn(supervisor(init).unwrap());
}
