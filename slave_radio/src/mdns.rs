//! mDNS responder: advertises `led-board.local` on whichever interface is up.
//! Straight port of the Rustagon reference (registry edge-mdns 0.8, whose
//! random-source bound the esp-hal hardware RNG satisfies).

use alloc::string::String;
use core::net::{Ipv4Addr, Ipv6Addr};

use edge_mdns::{buf::VecBufAccess, domain::base::Ttl, io::MdnsIoError, HostAnswersMdnsHandler};
use edge_nal::UdpSplit as _;
use edge_nal_embassy::UdpError;
use embassy_net::Stack;
use embassy_sync::{blocking_mutex::raw::NoopRawMutex, signal::Signal};
use embassy_time::{Duration, Timer};

#[embassy_executor::task(pool_size = 4)]
pub async fn mdns_task(stack: Stack<'static>, our_name: String, our_ip: Ipv4Addr) {
    Timer::after(Duration::from_millis(1_000)).await;

    log::info!("mdns: responding as {}.local at {}", our_name, our_ip);

    if let Err(err) = mdns_runner(stack, &our_name, our_ip).await {
        log::warn!("mdns: responder stopped: {:?}", err);
    }
}

pub async fn mdns_runner(stack: Stack<'static>, our_name: &str, our_ip: Ipv4Addr) -> Result<(), MdnsIoError<UdpError>> {
    let udp_buffers: edge_nal_embassy::UdpBuffers<5, 1024, 1024, 5> = edge_nal_embassy::UdpBuffers::new();
    let udp = edge_nal_embassy::Udp::new(stack, &udp_buffers);

    let (recv_buf, send_buf) = (
        VecBufAccess::<NoopRawMutex, 1500>::new(),
        VecBufAccess::<NoopRawMutex, 1500>::new(),
    );

    let mut socket = edge_mdns::io::bind(
        &udp,
        edge_mdns::io::IPV4_DEFAULT_SOCKET,
        Some(Ipv4Addr::UNSPECIFIED),
        None,
    )
    .await?;

    let (recv, send) = socket.split();

    let host = edge_mdns::host::Host {
        hostname: our_name,
        ipv4: our_ip,
        ipv6: Ipv6Addr::UNSPECIFIED,
        ttl: Ttl::from_secs(60),
    };

    let signal = Signal::<NoopRawMutex, _>::new();

    let mdns = edge_mdns::io::Mdns::new(
        Some(Ipv4Addr::UNSPECIFIED),
        None,
        recv,
        send,
        recv_buf,
        send_buf,
        esp_hal::rng::Rng::new(),
        &signal,
    );

    mdns.run(HostAnswersMdnsHandler::new(&host)).await
}
