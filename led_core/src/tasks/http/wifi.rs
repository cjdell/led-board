//! WiFi provisioning API — scan, join, saved networks, and join forensics.
//!
//! The platform behaviour lives in [`Board`]: the cyw43 build reboots into
//! its join loop after a save, the slave-radio build has the ESP32 join
//! live (and mirrors the config over the link), and the simulator answers
//! with canned radio data. Everything above the trait — persistence,
//! response shapes, error handling — is this file, shared by all of them.
//!
//! Endpoint set (all JSON):
//! * `GET  /api/wifi`        — scan results `[{ssid, signal_strength, password_required}]`
//! * `POST /api/wifi`        — `{ssid, pass}`: persist + connect
//! * `GET  /api/networks`    — `{networks: [{ssid}], mode}` — SSIDs only, never passwords
//! * `POST /api/forget`      — `{ssid}`: drop a saved network (takes effect next boot)
//! * `GET /api/join-status`  — join-loop self-report for the web UI's status card
//!
//! The response shapes match the web app's schemas in `web/src/lib/device`.

use alloc::format;
use alloc::string::{String, ToString as _};
use alloc::vec::Vec;
use embedded_tools::config::ConfigFileTrait as _;
use picoserve::{
    ResponseSent,
    io::Read,
    request::Request,
    response::{IntoResponse, Response, ResponseWriter, StatusCode},
    routing::RequestHandlerService,
};
use serde::{Deserialize, Serialize};

use crate::board::{Board, JoinOutcome};
use crate::config::LedBoardConfigurator as _;
use crate::types::*;

#[derive(Serialize)]
struct ScanRow {
    ssid: String,
    signal_strength: i8,
    password_required: bool,
}

fn scan_json(rows: &[ScanRow]) -> String {
    serde_json::to_string(rows).unwrap_or_else(|_| "[]".to_string())
}

fn error_response(code: StatusCode, message: &str) -> impl IntoResponse + use<'_> {
    let body = serde_json::json!({ "error": message }).to_string();
    Response::new(code, body).with_headers([
        ("Content-Type", "application/json"),
        ("Access-Control-Allow-Origin", "*"),
    ])
}

pub struct WifiScanHandler<B: Board> {
    board: B,
}

impl<B: Board> WifiScanHandler<B> {
    pub fn new(board: B) -> Self {
        Self { board }
    }
}

impl<B: Board> RequestHandlerService<()> for WifiScanHandler<B> {
    async fn call_request_handler_service<R: Read, W: ResponseWriter<Error = R::Error>>(
        &self,
        _: &(),
        _: (),
        req: Request<'_, R>,
        writer: W,
    ) -> Result<ResponseSent, W::Error> {
        let entries = match self.board.scan().await {
            Ok(entries) => entries,
            Err(message) => {
                return error_response(StatusCode::SERVICE_UNAVAILABLE, &message)
                    .write_to(req.body_connection.finalize().await?, writer)
                    .await;
            }
        };

        let rows: Vec<ScanRow> = entries
            .into_iter()
            .map(|entry| ScanRow {
                ssid: entry.ssid,
                signal_strength: entry.signal_strength,
                password_required: entry.password_required,
            })
            .collect();

        json_response!(req, writer, scan_json(&rows).as_str())
    }
}

#[derive(Deserialize)]
pub struct JoinRequest {
    pub ssid: String,
    #[serde(default)]
    pub pass: String,
}

/// `GET /api/networks` — the saved WiFi networks (SSIDs only, never
/// passwords), so the web UI can show "Saved"/"Current" badges and offer
/// Forget without round-tripping the whole device config.
pub struct WifiNetworksHandler<B: Board> {
    config_file: LedBoardConfigFile<B::Fs>,
}

impl<B: Board> WifiNetworksHandler<B> {
    pub fn new(config_file: LedBoardConfigFile<B::Fs>) -> Self {
        Self { config_file }
    }
}

impl<B: Board> RequestHandlerService<()> for WifiNetworksHandler<B> {
    async fn call_request_handler_service<R: Read, W: ResponseWriter<Error = R::Error>>(
        &self,
        _: &(),
        _: (),
        req: Request<'_, R>,
        writer: W,
    ) -> Result<ResponseSent, W::Error> {
        let data = self.config_file.get_data().await;

        let rows: Vec<_> = data
            .known_wifi_networks
            .iter()
            .map(|n| serde_json::json!({ "ssid": n.ssid }))
            .collect();

        let body = serde_json::json!({
            "networks": rows,
            "mode": format!("{:?}", data.wifi_mode),
        })
        .to_string();

        json_response!(req, writer, body.as_str())
    }
}

#[derive(Deserialize)]
pub struct ForgetRequest {
    pub ssid: String,
}

/// `POST /api/forget` — drop a saved network. Takes effect on the next boot
/// (the join loop reads config once at startup); the current connection, if
/// any, is left alone.
pub struct WifiForgetHandler<B: Board> {
    config_file: LedBoardConfigFile<B::Fs>,
    board: B,
}

impl<B: Board> WifiForgetHandler<B> {
    pub fn new(config_file: LedBoardConfigFile<B::Fs>, board: B) -> Self {
        Self { config_file, board }
    }
}

impl<B: Board> RequestHandlerService<()> for WifiForgetHandler<B> {
    async fn call_request_handler_service<R: Read, W: ResponseWriter<Error = R::Error>>(
        &self,
        _: &(),
        _: (),
        mut req: Request<'_, R>,
        writer: W,
    ) -> Result<ResponseSent, W::Error> {
        let buffer = read_request_to_buffer!(req, writer);

        let Ok(forget) = serde_json::from_slice::<ForgetRequest>(&buffer) else {
            return error_response(StatusCode::BAD_REQUEST, "expected {ssid}")
                .write_to(req.body_connection.finalize().await?, writer)
                .await;
        };

        match self.config_file.remove_known_wifi_network(forget.ssid).await {
            Ok(removed) => {
                self.board.config_saved().await;

                let body = serde_json::json!({ "ok": true, "removed": removed }).to_string();
                json_response!(req, writer, body.as_str())
            }
            Err(err) => {
                error_response(StatusCode::INTERNAL_SERVER_ERROR, &format!("{err:?}"))
                    .write_to(req.body_connection.finalize().await?, writer)
                    .await
            }
        }
    }
}

/// `GET /api/join-status` — the wifi join loop's self-report. Readable from a
/// phone browser while the board sits in AP fallback, where there is no other
/// way to see *why* it refused to come up on the LAN.
pub struct JoinStatusHandler<B: Board> {
    board: B,
}

impl<B: Board> JoinStatusHandler<B> {
    pub fn new(board: B) -> Self {
        Self { board }
    }
}

impl<B: Board> RequestHandlerService<()> for JoinStatusHandler<B> {
    async fn call_request_handler_service<R: Read, W: ResponseWriter<Error = R::Error>>(
        &self,
        _: &(),
        _: (),
        req: Request<'_, R>,
        writer: W,
    ) -> Result<ResponseSent, W::Error> {
        json_response!(req, writer, self.board.join_status_json().await.as_str())
    }
}

pub struct WifiJoinHandler<B: Board> {
    config_file: LedBoardConfigFile<B::Fs>,
    board: B,
}

impl<B: Board> WifiJoinHandler<B> {
    pub fn new(config_file: LedBoardConfigFile<B::Fs>, board: B) -> Self {
        Self { config_file, board }
    }
}

impl<B: Board> RequestHandlerService<()> for WifiJoinHandler<B> {
    async fn call_request_handler_service<R: Read, W: ResponseWriter<Error = R::Error>>(
        &self,
        _: &(),
        (): (),
        mut req: Request<'_, R>,
        writer: W,
    ) -> Result<ResponseSent, W::Error> {
        let buffer = read_request_to_buffer!(req, writer);

        let Ok(join) = serde_json::from_slice::<JoinRequest>(&buffer) else {
            return error_response(StatusCode::BAD_REQUEST, "expected {ssid, pass}")
                .write_to(req.body_connection.finalize().await?, writer)
                .await;
        };

        // Credentials are authoritative on the board: persist first, then let
        // the platform act on them (live join on the slave radio, reboot into
        // the join loop on the cyw43).
        if let Err(err) = self
            .config_file
            .add_known_wifi_network(join.ssid.clone(), join.pass.clone())
            .await
        {
            return error_response(StatusCode::INTERNAL_SERVER_ERROR, &alloc::format!("{err:?}"))
                .write_to(req.body_connection.finalize().await?, writer)
                .await;
        }

        match self.board.join(join.ssid.clone(), join.pass.clone()).await {
            JoinOutcome::Joined { ssid, joined, mode } => {
                let body = serde_json::json!({ "joined": joined, "ssid": ssid, "mode": mode }).to_string();
                json_response!(req, writer, body.as_str())
            }
            JoinOutcome::RebootScheduled { ssid } => {
                let body = serde_json::json!({
                    "joined": false,
                    "rebooting": true,
                    "ssid": ssid,
                    "mode": "Station",
                })
                .to_string();

                json_response!(req, writer, body.as_str())?;

                // Give the response a moment to flush before the reset.
                embassy_time::Timer::after_millis(1_500).await;
                self.board.reboot().await;
                unreachable!();
            }
            JoinOutcome::Unavailable(message) => {
                error_response(StatusCode::SERVICE_UNAVAILABLE, &message)
                    .write_to(req.body_connection.finalize().await?, writer)
                    .await
            }
        }
    }
}
