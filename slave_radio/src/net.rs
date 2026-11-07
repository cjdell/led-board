//! Fallback-AP infrastructure: DHCP server (with captive URL) and captive DNS.
//! Ported from the Pico firmware (same edge-* fork APIs).

use alloc::format;
use core::net::{Ipv4Addr, SocketAddr, SocketAddrV4};

use edge_dhcp::io::{self, DEFAULT_SERVER_PORT};
use edge_dhcp::server::ServerOptions;
use edge_nal::UdpBind as _;
use edge_nal_embassy::{Udp, UdpBuffers};
use embassy_net::Stack;
use embassy_time::{Duration, Timer};

#[embassy_executor::task]
pub async fn dhcp_task(stack: Stack<'static>, ap_ip_address: Ipv4Addr) {
    log::info!("DHCP: Task started");

    let mut buf = [0u8; 1500];

    let mut gw_buf = [Ipv4Addr::UNSPECIFIED];
    let dns = [ap_ip_address];

    let buffers = UdpBuffers::<3, 1024, 1024, 10>::new();
    let unbound_socket = Udp::new(stack, &buffers);
    let mut bound_socket = unbound_socket
        .bind(SocketAddr::V4(SocketAddrV4::new(
            Ipv4Addr::UNSPECIFIED,
            DEFAULT_SERVER_PORT,
        )))
        .await
        .unwrap();

    loop {
        let captive_url = format!("http://{}/", ap_ip_address);

        let mut options = ServerOptions::new(ap_ip_address, Some(&mut gw_buf));
        options.dns = &dns;
        options.captive_url = Some(&captive_url);

        if let Err(err) = io::server::run(
            &mut edge_dhcp::server::Server::<_, 64>::new_with_et(ap_ip_address),
            &options,
            &mut bound_socket,
            &mut buf,
        )
        .await
        {
            log::warn!("DHCP: Server error: {:?}", err);
        }

        Timer::after(Duration::from_millis(500)).await;
    }
}

#[embassy_executor::task]
pub async fn captive_task(stack: Stack<'static>, ap_ip_address: Ipv4Addr) {
    log::info!("Captive: Task started");

    loop {
        let udp_buffers: UdpBuffers<5, 1024, 1024, 5> = UdpBuffers::new();
        let udp = Udp::new(stack, &udp_buffers);

        let mut tx_buf = [0u8; 1500];
        let mut rx_buf = [0u8; 1500];

        let _ = edge_captive::io::run(
            &udp,
            SocketAddr::new(core::net::IpAddr::V4(Ipv4Addr::UNSPECIFIED), 53),
            &mut tx_buf,
            &mut rx_buf,
            ap_ip_address,
            core::time::Duration::from_secs(60),
        )
        .await;

        log::info!("Captive: Stopped, restarting");
    }
}
