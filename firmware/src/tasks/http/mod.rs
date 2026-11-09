#[macro_use]
mod macros;

mod common;
mod config;
mod info;
mod ota;
mod web_socket;
mod wifi;

#[cfg(feature = "radio")]
pub mod tunnel;

use crate::{
    tasks::http::{
        common::*,
        config::{GetConfigHandler, SaveConfigHandler},
        info::{GetAnimationsHandler, GetPlaylistHandler},
        ota::OtaUpdateHandler,
        web_socket::WebSocketHandler,
        wifi::{JoinStatusHandler, WifiJoinHandler, WifiScanHandler},
    },
    types::*,
};
use alloc::{boxed::Box, vec::Vec};
use defmt::*;
use embassy_executor::Spawner;
use embassy_net::Stack;
use picoserve::{
    AppBuilder, AppRouter, Router, Server, make_static,
    response::WebSocketUpgrade,
    routing::{PathRouter, get, get_service, post, post_service},
};
use rp235x_embedded_tools::flash::FlashStorage;

#[cfg(feature = "radio")]
pub use tunnel::TunnelSocket;

pub struct AppProps {
    web_socket_incoming_sender: WebSocketIncomingSender,
    flash_storage: FlashStorage,
    local_fs: LocalFs,
    config_file: LedBoardConfigFile,
    watchdog: SharedWatchdog,
}

impl AppProps {
    pub fn new(
        web_socket_incoming_sender: WebSocketIncomingSender,
        flash_storage: FlashStorage,
        local_fs: LocalFs,
        config_file: LedBoardConfigFile,
        watchdog: SharedWatchdog,
    ) -> Self {
        Self {
            web_socket_incoming_sender,
            flash_storage,
            local_fs,
            config_file,
            watchdog,
        }
    }
}

impl AppBuilder for AppProps {
    type PathRouter = impl PathRouter;

    fn build_app(self) -> Router<Self::PathRouter> {
        let config_file_1 = self.config_file.clone();
        let config_file_2 = self.config_file.clone();
        let config_file_3 = self.config_file.clone();
        let config_file_4 = self.config_file.clone();

        let local_fs_1 = self.local_fs.clone();

        Router::from_service(CustomNotFound)
            .route("/", get(async |_: RequestInfo| html_app_response()))
            .route("/remote", get(async |_: RequestInfo| html_app_response()))
            .route("/grid", get(async |_: RequestInfo| html_app_response()))
            .route("/config", get(async |_: RequestInfo| html_app_response()))
            .nest(
                "/api",
                Router::new()
                    .route(
                        "/config",
                        get_service(GetConfigHandler::new(config_file_1))
                            .post_service(SaveConfigHandler::new(config_file_2)),
                    )
                    .route(
                        "/reboot",
                        post(async || {
                            embassy_rp::rom_data::reboot(0, 10, 0, 0);
                            "Unreachable"
                        })
                        .options(async || cors_options_response()),
                    )
                    .route(
                        "/wifi",
                        get_service(WifiScanHandler::new(config_file_3))
                            .post_service(WifiJoinHandler::new(config_file_4))
                            .options(async || cors_options_response()),
                    )
                    .route("/join-status", get_service(JoinStatusHandler))
                    .nest(
                        "/info",
                        Router::new()
                            .route("/animations", get_service(GetAnimationsHandler::new()))
                            .route("/playlist", get_service(GetPlaylistHandler::new(local_fs_1))),
                    )
                    .route(
                        "/ws",
                        get(async move |upgrade: WebSocketUpgrade| {
                            info!("Upgrade WebSocket connection...");
                            upgrade
                                .on_upgrade(WebSocketHandler::new(self.web_socket_incoming_sender))
                                .with_protocol("json")
                        })
                        .options(async || cors_options_response()),
                    )
                    .route(
                        "/ota",
                        post_service(OtaUpdateHandler::new(self.flash_storage.clone(), self.watchdog.clone()))
                            .options(async || cors_options_response()),
                    ),
            )
            // Captive Portal stuff...
            .route("/generate_204", get(async || redirect_home_response()))
            .route("/hotspot-detect.html", get(async || redirect_home_response()))
            .route("/connecttest.txt", get(async || redirect_home_response()))
            .route("/redirect", get(async || redirect_home_response()))
    }
}

const WEB_TASK_POOL_SIZE: usize = 5;

static CONFIG: picoserve::Config = picoserve::Config::new(picoserve::Timeouts {
    start_read_request: picoserve::time::Duration::from_secs(300),
    persistent_start_read_request: picoserve::time::Duration::from_secs(300),
    read_request: picoserve::time::Duration::from_secs(300),
    write: picoserve::time::Duration::from_secs(300),
});

#[embassy_executor::task(pool_size = WEB_TASK_POOL_SIZE)]
async fn web_task(id: usize, stack: Stack<'static>, app: &'static AppRouter<AppProps>) -> ! {
    info!("Starting Web Task...");

    let port = 80;

    // Throughput through these buffers is buffer/RTT-bound: picoserve blocks
    // whenever a write exceeds the remaining space. 1 KB against Wi-Fi LAN RTTs
    // (3–15 ms) caps a connection around 0.5–2.5 Mbit/s no matter how fast the
    // radio bus is. 8 KB TX + 4 KB RX per task; with 5 tasks that is 100 KB of
    // the 192 KB heap, and it matches smoltcp's default send window so the
    // socket stays full (see the blog-post numbers in ws.rs on the bus side).
    let mut tcp_rx_buffer = Vec::new();
    tcp_rx_buffer.resize(4096, 0);
    let mut tcp_tx_buffer = Vec::new();
    tcp_tx_buffer.resize(8192, 0);
    let mut http_buffer = Vec::new();
    http_buffer.resize(8 * 1024, 0);

    Box::new(
        Server::new(app, &CONFIG, http_buffer.as_mut())
            .listen_and_serve(
                id,
                stack,
                port,
                tcp_rx_buffer.as_mut_slice(),
                tcp_tx_buffer.as_mut_slice(),
            )
            .await,
    )
    .into_never()
}

pub fn start_http(spawner: Spawner, stack: Stack<'static>, app: AppProps) {
    let app = make_static!(AppRouter<AppProps>, app.build_app());

    for id in 0..WEB_TASK_POOL_SIZE {
        spawner.spawn(web_task(id, stack, app).unwrap());
    }
}

// ---------------------------------------------------------------------------
// Radio (ESP32-C3 tunnel) serving — no IP stack on the Pico.
// ---------------------------------------------------------------------------

/// App router pointer for tunnel serving. Stored as an integer because the
/// router type (via `LocalFs`) is intentionally not `Sync` — a single static
/// instance shared across connection tasks on one executor.
#[cfg(feature = "radio")]
static TUNNEL_APP: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

/// Prepare the app router for tunnel-served connections. Connections arrive
/// via [`crate::tasks::radio`], which calls [`serve_tunnel`] per stream.
#[cfg(feature = "radio")]
pub async fn start_http_radio(app: AppProps) {
    let app = make_static!(AppRouter<AppProps>, app.build_app());
    let ptr: *const AppRouter<AppProps> = app;
    TUNNEL_APP.store(ptr as usize, core::sync::atomic::Ordering::Relaxed);
}

/// Serve HTTP requests over a single relayed TCP connection from the C3.
#[cfg(feature = "radio")]
pub async fn serve_tunnel(
    socket: TunnelSocket,
) -> Result<picoserve::DisconnectionInfo<picoserve::NoGracefulShutdown>, picoserve::Error<tunnel::TunnelError>> {
    let app = {
        let ptr = TUNNEL_APP.load(core::sync::atomic::Ordering::Relaxed);
        if ptr == 0 {
            defmt::panic!("start_http_radio must run before serving");
        }
        unsafe { &*(ptr as *const AppRouter<AppProps>) }
    };

    let mut http_buffer = Vec::new();
    http_buffer.resize(8 * 1024, 0);

    Server::new(app, &CONFIG, http_buffer.as_mut()).serve(socket).await
}
