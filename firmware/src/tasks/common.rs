use alloc::{format, string::String};
use core::net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4};
use defmt::*;
use edge_mdns::{HostAnswersMdnsHandler, buf::VecBufAccess, domain::base::Ttl, io::MdnsIoError};
use edge_nal::{UdpBind as _, UdpSplit as _};
use edge_nal_embassy::UdpError;
use embassy_net::Stack;
use embassy_rp::{peripherals::TRNG, trng::Trng};
use embassy_sync::{blocking_mutex::raw::NoopRawMutex, channel::Sender, signal::Signal};
use embassy_time::{Duration, Timer};

pub struct WifiControl {
    sender: Sender<'static, NoopRawMutex, bool, 1>,
}

impl WifiControl {
    pub fn new(sender: Sender<'static, NoopRawMutex, bool, 1>) -> Self {
        Self { sender }
    }

    pub async fn set_led(&self, state: bool) {
        self.sender.send(state).await;
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

#[embassy_executor::task]
pub async fn mdns_task(stack: Stack<'static>, trng: Trng<'static, TRNG>, our_name: String, our_ip: Ipv4Addr) {
    info!("mdns_task");

    Timer::after(Duration::from_millis(1_000)).await;

    mdns_runner(stack, trng, &our_name, our_ip)
        .await
        .expect("mdns_runner failed");
}

pub async fn mdns_runner(
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
    let mut socket = edge_mdns::io::bind(
        &udp,
        edge_mdns::io::DEFAULT_SOCKET,
        Some(Ipv4Addr::UNSPECIFIED),
        Some(0),
    )
    .await?;

    let (recv, send) = socket.split();

    info!("mdns_runner: Creating host...");
    let host = edge_mdns::host::Host {
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
