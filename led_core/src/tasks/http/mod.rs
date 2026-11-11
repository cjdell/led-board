#[macro_use]
mod macros;

pub mod common;
mod config;
mod info;
mod ota;
mod playlists;
mod web_socket;
mod wifi;

use crate::board::Board;
use crate::tasks::http::{
    common::*,
    config::{GetConfigHandler, SaveConfigHandler},
    info::{GetAnimationsHandler, GetPlaylistHandler, GetStatusHandler, GetVersionHandler},
    ota::OtaUpdateHandler,
    playlists::PlaylistLibraryHandler,
    web_socket::WebSocketHandler,
    wifi::{JoinStatusHandler, WifiForgetHandler, WifiJoinHandler, WifiNetworksHandler, WifiScanHandler},
};
use crate::types::*;
use alloc::boxed::Box;
use embassy_net::Stack;
use embedded_tools::info;
use picoserve::{
    AppBuilder, AppRouter, Router, Server,
    response::WebSocketUpgrade,
    routing::{PathRouter, get, get_service, post, post_service},
};

/// Everything the web app needs from the running system, cloned into the
/// routes at build time. `B` is the machine the app is running on — the
/// Pico firmware or the simulator pass their own implementation.
pub struct AppProps<B: Board> {
    web_socket_incoming_sender: WebSocketIncomingSender,
    status_watch: &'static StatusWatch,
    playlist_watch: &'static PlaylistWatch,
    ota: B::Ota,
    local_fs: B::Fs,
    config_file: LedBoardConfigFile<B::Fs>,
    board: B,
}

impl<B: Board> AppProps<B> {
    pub fn new(
        web_socket_incoming_sender: WebSocketIncomingSender,
        status_watch: &'static StatusWatch,
        playlist_watch: &'static PlaylistWatch,
        ota: B::Ota,
        local_fs: B::Fs,
        config_file: LedBoardConfigFile<B::Fs>,
        board: B,
    ) -> Self {
        Self {
            web_socket_incoming_sender,
            status_watch,
            playlist_watch,
            ota,
            local_fs,
            config_file,
            board,
        }
    }
}

impl<B: Board> AppBuilder for AppProps<B> {
    type PathRouter = impl PathRouter;

    fn build_app(self) -> Router<Self::PathRouter> {
        // These handles are cheap clones of shared state: move one in and
        // clone at each registration point instead of numbered copies.
        let board = self.board;
        let config_file = self.config_file;
        let local_fs = self.local_fs;
        let ota = self.ota;
        let status_watch = self.status_watch;
        let playlist_watch = self.playlist_watch;

        Router::from_service(CustomNotFound)
            .route("/", get(async |_: RequestInfo| html_app_response()))
            // SPA pages — every one needs a route so deep links survive a
            // reload (the bundle handles routing client-side from there).
            .route("/now", get(async |_: RequestInfo| html_app_response()))
            .route("/playlists", get(async |_: RequestInfo| html_app_response()))
            .route("/grid", get(async |_: RequestInfo| html_app_response()))
            .route("/wifi", get(async |_: RequestInfo| html_app_response()))
            .route("/setup", get(async |_: RequestInfo| html_app_response()))
            // Old paths (bookmarked by the festival crews): same bundle,
            // the client redirects to the current page name.
            .route("/remote", get(async |_: RequestInfo| html_app_response()))
            .route("/config", get(async |_: RequestInfo| html_app_response()))
            .nest(
                "/api",
                Router::new()
                    .route(
                        "/config",
                        get_service(GetConfigHandler::new(config_file.clone()))
                            .post_service(SaveConfigHandler::new(config_file.clone(), board.clone())),
                    )
                    .route(
                        "/reboot",
                        post({
                            let board = board.clone();
                            async move || {
                                board.reboot().await;
                                "Unreachable"
                            }
                        })
                        .options(async || cors_options_response()),
                    )
                    // Recovery hatch that does not depend on this app's USB
                    // stack: the ROM re-enumerates as a fresh BOOTSEL device,
                    // visible to picotool even when `/dev/cu.usbmodem*` is
                    // gone. `curl -X POST http://<board>/api/bootsel`.
                    .route(
                        "/bootsel",
                        post({
                            let board = board.clone();
                            async move || {
                                board.bootsel().await;
                                "Unreachable"
                            }
                        })
                        .options(async || cors_options_response()),
                    )
                    .route(
                        "/wifi",
                        get_service(WifiScanHandler::<B>::new(board.clone()))
                            .post_service(WifiJoinHandler::<B>::new(config_file.clone(), board.clone()))
                            .options(async || cors_options_response()),
                    )
                    .route(
                        "/networks",
                        get_service(WifiNetworksHandler::<B>::new(config_file.clone()))
                            .options(async || cors_options_response()),
                    )
                    .route(
                        "/forget",
                        post_service(WifiForgetHandler::<B>::new(config_file.clone(), board.clone()))
                            .options(async || cors_options_response()),
                    )
                    .route("/join-status", get_service(JoinStatusHandler::<B>::new(board.clone())))
                    .route("/status", get_service(GetStatusHandler::new(status_watch)))
                    .route("/version", get_service(GetVersionHandler {}))
                    .route_service("/playlists", PlaylistLibraryHandler::new(local_fs.clone()))
                    .nest(
                        "/info",
                        Router::new()
                            .route("/animations", get_service(GetAnimationsHandler::new()))
                            .route("/playlist", get_service(GetPlaylistHandler::new(playlist_watch))),
                    )
                    .route(
                        "/ws",
                        get(async move |upgrade: WebSocketUpgrade| {
                            info!("Upgrade WebSocket connection...");
                            upgrade
                                .on_upgrade(WebSocketHandler::new(
                                    self.web_socket_incoming_sender,
                                    status_watch,
                                    playlist_watch,
                                ))
                                .with_protocol("json")
                        })
                        .options(async || cors_options_response()),
                    )
                    .route(
                        "/ota",
                        post_service(OtaUpdateHandler::<B>::new(ota.clone(), board.clone()))
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

pub const WEB_TASK_POOL_SIZE: usize = 5;

pub static CONFIG: picoserve::Config = picoserve::Config::new(picoserve::Timeouts {
    start_read_request: picoserve::time::Duration::from_secs(300),
    persistent_start_read_request: picoserve::time::Duration::from_secs(300),
    read_request: picoserve::time::Duration::from_secs(300),
    write: picoserve::time::Duration::from_secs(300),
});

/// Leak the finished router so connection tasks on any runtime (embassy
/// executor here, tokio in the simulator) can share it by `&'static`.
pub fn build_app<B: Board>(props: AppProps<B>) -> &'static AppRouter<AppProps<B>> {
    Box::leak(Box::new(props.build_app()))
}

/// Per-connection body of an embassy-net web task: serve HTTP on one
/// listener slot. `#[embassy_executor::task]` pools cannot be generic, so
/// each platform declares a concrete wrapper around this.
pub async fn web_task_body<B: Board>(id: usize, stack: Stack<'static>, app: &'static AppRouter<AppProps<B>>) {
    info!("Starting Web Task...");

    let port = 80;

    // Throughput through these buffers is buffer/RTT-bound: picoserve blocks
    // whenever a write exceeds the remaining space. 1 KB against Wi-Fi LAN RTTs
    // (3–15 ms) caps a connection around 0.5–2.5 Mbit/s no matter how fast the
    // radio bus is. 8 KB TX + 4 KB RX per task; with 5 tasks that is 100 KB of
    // the 192 KB heap, and it matches smoltcp's default send window so the
    // socket stays full (see the blog-post numbers in ws.rs on the bus side).
    let mut tcp_rx_buffer = alloc::vec::Vec::new();
    tcp_rx_buffer.resize(4096, 0);
    let mut tcp_tx_buffer = alloc::vec::Vec::new();
    tcp_tx_buffer.resize(8192, 0);
    let mut http_buffer = alloc::vec::Vec::new();
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
