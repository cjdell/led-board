#[macro_use]
mod macros;

mod common;
mod config;
mod info;
mod ota;
mod web_socket;

use crate::{
    flash::FlashStorage,
    tasks::http::{
        common::*,
        config::{GetConfigHandler, SaveConfigHandler},
        info::{GetAnimationsHandler, GetPlaylistHandler},
        ota::OtaUpdateHandler,
        web_socket::WebSocketHandler,
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

        let local_fs_1 = self.local_fs.clone();

        Router::from_service(CustomNotFound)
            .route("/", get(async |_: RequestInfo| html_app_response()))
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

const WEB_TASK_POOL_SIZE: usize = 4;

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

    let mut tcp_rx_buffer = Vec::new();
    tcp_rx_buffer.resize(1024, 0);
    let mut tcp_tx_buffer = Vec::new();
    tcp_tx_buffer.resize(1024, 0);
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
