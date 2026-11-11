use alloc::{format, string::String};
use core::net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4};
use core::pin::pin;
use crate::types::{EthernetSignal, EthernetSignalMessage};
use defmt::*;
use edge_mdns::{HostAnswersMdnsHandler, buf::VecBufAccess, domain::base::Ttl, io::MdnsIoError};
use edge_nal::{UdpBind as _, UdpSplit as _};
use edge_nal_embassy::UdpError;
use embassy_futures::select::{Either, select};
use embassy_net::Stack;
use embassy_rp::{peripherals::TRNG, trng::Trng};
use embassy_sync::{
    blocking_mutex::raw::{CriticalSectionRawMutex, NoopRawMutex},
    channel::Sender,
    signal::Signal,
    watch,
};
use embassy_time::{Duration, Timer};

#[derive(Clone)]
pub struct WifiControl {
    sender: Sender<'static, NoopRawMutex, bool, 1>,
}

/// Anything the heartbeat task can blink — the cyw43 LED or the C3 status LED.
pub trait LedControl: Clone {
    fn blink(&self, on: bool) -> impl core::future::Future<Output = ()>;
}

impl WifiControl {
    pub fn new(sender: Sender<'static, NoopRawMutex, bool, 1>) -> Self {
        Self { sender }
    }

    pub async fn set_led(&self, state: bool) {
        self.sender.send(state).await;
    }
}

impl LedControl for WifiControl {
    fn blink(&self, on: bool) -> impl core::future::Future<Output = ()> {
        self.set_led(on)
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

// ── mDNS responder ─────────────────────────────────────────────────────────
//
// The old shape spawned one responder once, with the IP of the very first
// lease: DHCP re-assigns (rejoin after a flap, AP fallback) kept announcing
// the stale address forever, and any UDP error ended `Mdns::run` — which the
// task `.expect()`-ed, so `*.local` was dead until reboot. Now it is a
// supervised session loop: every session binds fresh (that re-joins the
// multicast group too — smoltcp memberships die with the link), serves the
// *current* address, and the loop rebinds on every connection event, on a
// dead/drifted config (self-check in `announce_ticker`), and on any IO error
// with backoff. Announcements repeat at ¼ TTL so records survive lost
// replies and refresh without waiting for a query.

/// mDNS refresh cadence: ¼ of the answer TTL — several redundant announces
/// per cache lifetime; the packets are tens of bytes.
const ANNOUNCE_INTERVAL: Duration = Duration::from_secs(15);
const MDNS_TTL_SECS: u32 = 60;

type EthernetSignalReceiver<'a> =
    watch::Receiver<'a, CriticalSectionRawMutex, EthernetSignalMessage, 10>;

#[embassy_executor::task]
pub async fn mdns_task(
    stack: Stack<'static>,
    mut trng: Trng<'static, TRNG>,
    our_name: String,
    ethernet_signal: &'static EthernetSignal,
) {
    info!("mdns_task");

    let mut receiver = match ethernet_signal.receiver() {
        Some(receiver) => receiver,
        None => {
            error!("mdns: no signal receiver slot left");
            return;
        }
    };

    loop {
        // Wait for an IPv4 address — current or next event.
        let ip = loop {
            let msg = match receiver.try_get() {
                Some(msg) => msg,
                None => receiver.changed().await,
            };

            match msg {
                EthernetSignalMessage::Connected(ip) => break ip,
                EthernetSignalMessage::Disconnected => {
                    info!("mdns: link down, responder idle");
                    Timer::after(Duration::from_secs(2)).await;
                }
            }
        };

        info!("mdns: serving {} → {}.local", ip, our_name.as_str());

        // Let a fresh lease/link settle before binding and announcing.
        Timer::after(Duration::from_millis(1_000)).await;

        match mdns_session(stack, &mut trng, &our_name, ip, &mut receiver).await {
            // Address or link moved underneath us — rebind (fresh join +
            // fresh announcement) with the new state immediately.
            Ok(()) => {}
            Err(err) => {
                warn!("mdns: session error: {:?} — rebinding", Debug2Format(&err));
                // Backoff so a full SocketSet or flapping link cannot spin.
                Timer::after(Duration::from_secs(1)).await;
            }
        }
    }
}

/// One responder lifetime for `our_ip`. Ends on an IO error, on the
/// announce-ticker spotting address/link drift, or on any connection event
/// — the caller rebuilds with the current state either way.
async fn mdns_session(
    stack: Stack<'static>,
    trng: &mut Trng<'static, TRNG>,
    our_name: &str,
    our_ip: Ipv4Addr,
    receiver: &mut EthernetSignalReceiver<'_>,
) -> Result<(), MdnsIoError<UdpError>> {
    let udp_buffers: edge_nal_embassy::UdpBuffers<5, 1024, 1024, 5> = edge_nal_embassy::UdpBuffers::new();
    let udp = edge_nal_embassy::Udp::new(stack, &udp_buffers);

    let (recv_buf, send_buf) = (
        VecBufAccess::<NoopRawMutex, 1500>::new(),
        VecBufAccess::<NoopRawMutex, 1500>::new(),
    );

    let mut socket = edge_mdns::io::bind(
        &udp,
        edge_mdns::io::DEFAULT_SOCKET,
        Some(Ipv4Addr::UNSPECIFIED),
        Some(0),
    )
    .await?;

    let (recv, send) = socket.split();

    let host = edge_mdns::host::Host {
        hostname: our_name,
        ipv4: our_ip,
        ipv6: Ipv6Addr::UNSPECIFIED,
        ttl: Ttl::from_secs(MDNS_TTL_SECS),
    };

    // Signals the edge_mdns broadcast loop into re-announcing (its own loop
    // would announce once at startup and then wait forever).
    let signal = Signal::<NoopRawMutex, ()>::new();

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

    let run = pin!(mdns.run(HostAnswersMdnsHandler::new(&host)));
    let serve = pin!(select(run, announce_ticker(&signal, stack, our_ip)));
    let changed = pin!(receiver.changed());

    match select(serve, changed).await {
        Either::First(Either::First(result)) => result,
        // Ticker saw the address/link move, or a connection event arrived:
        // a clean rebind, not an error.
        _ => Ok(()),
    }
}

/// Periodic gratuitous announce; also the self-check that catches a DHCP
/// renew or link loss that nobody signalled — ends the session so the
/// responder rebinds with fresh state.
async fn announce_ticker(signal: &Signal<NoopRawMutex, ()>, stack: Stack<'static>, served_ip: Ipv4Addr) {
    loop {
        Timer::after(ANNOUNCE_INTERVAL).await;

        match stack.config_v4() {
            Some(cfg) if cfg.address.address() == served_ip => signal.signal(()),
            _ => {
                info!("mdns: address or link changed under the responder — rebinding");
                return;
            }
        }
    }
}
