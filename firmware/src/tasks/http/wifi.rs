//! WiFi provisioning API — scan, join, saved networks, and join forensics.
//!
//! Built-in WiFi (cyw43) is the primary backend: the scan runs on the Pico's
//! own radio and a join persists to the config file, then reboots into the
//! join loop (`wifi_task` reads config once at boot). The `radio` feature
//! keeps the ESP32-C3 relay paths working (scan/join go over the link, saved
//! networks stay authoritative on the Pico).
//!
//! Endpoint set (all JSON):
//! * `GET  /api/wifi`        — scan results `[{ssid, signal_strength, password_required}]`
//! * `POST /api/wifi`        — `{ssid, pass}`: persist + connect (reboots on wifi builds)
//! * `GET  /api/networks`    — `{networks: [{ssid}], mode}` — SSIDs only, never passwords
//! * `POST /api/forget`      — `{ssid}`: drop a saved network (takes effect next boot)
//! * `GET  /api/join-status` — join-loop self-report for the web UI's status card
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

pub struct WifiScanHandler;

impl WifiScanHandler {
    pub fn new() -> Self {
        Self
    }
}

impl RequestHandlerService<()> for WifiScanHandler {
    async fn call_request_handler_service<R: Read, W: ResponseWriter<Error = R::Error>>(
        &self,
        _: &(),
        _: (),
        req: Request<'_, R>,
        writer: W,
    ) -> Result<ResponseSent, W::Error> {
        #[cfg(feature = "radio")]
        {
            let Some(link) = crate::tasks::radio::link().await else {
                return error_response(StatusCode::SERVICE_UNAVAILABLE, "radio link not up")
                    .write_to(req.body_connection.finalize().await?, writer)
                    .await;
            };

            let rows: alloc::vec::Vec<ScanRow> = link
                .scan()
                .await
                .into_iter()
                .map(|entry| ScanRow {
                    ssid: entry.ssid,
                    signal_strength: entry.signal_strength,
                    password_required: entry.password_required,
                })
                .collect();

            if rows.is_empty() {
                return error_response(StatusCode::SERVICE_UNAVAILABLE, "radio scan failed")
                    .write_to(req.body_connection.finalize().await?, writer)
                    .await;
            }

            json_response!(req, writer, scan_json(&rows).as_str())
        }

        #[cfg(not(feature = "radio"))]
        {
            // wifi build: the cyw43 itself answers (no C3 to relay through).
            let Some(found) = crate::tasks::wifi::wifi_scan().await else {
                return error_response(StatusCode::SERVICE_UNAVAILABLE, "wifi control not ready")
                    .write_to(req.body_connection.finalize().await?, writer)
                    .await;
            };

            let rows: alloc::vec::Vec<ScanRow> = found
                .into_iter()
                .map(|(ssid, signal_strength, password_required)| ScanRow {
                    ssid,
                    signal_strength,
                    password_required,
                })
                .collect();

            json_response!(req, writer, scan_json(&rows).as_str())
        }
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
pub struct WifiNetworksHandler {
    config_file: LedBoardConfigFile,
}

impl WifiNetworksHandler {
    pub fn new(config_file: LedBoardConfigFile) -> Self {
        Self { config_file }
    }
}

impl RequestHandlerService<()> for WifiNetworksHandler {
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
pub struct WifiForgetHandler {
    config_file: LedBoardConfigFile,
}

impl WifiForgetHandler {
    pub fn new(config_file: LedBoardConfigFile) -> Self {
        Self { config_file }
    }
}

impl RequestHandlerService<()> for WifiForgetHandler {
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
                #[cfg(feature = "radio")]
                crate::tasks::radio::sync_config(self.config_file.clone()).await;

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
pub struct JoinStatusHandler;

impl RequestHandlerService<()> for JoinStatusHandler {
    async fn call_request_handler_service<R: Read, W: ResponseWriter<Error = R::Error>>(
        &self,
        _: &(),
        _: (),
        req: Request<'_, R>,
        writer: W,
    ) -> Result<ResponseSent, W::Error> {
        json_response!(req, writer, crate::tasks::wifi::join_status_json().await.as_str())
    }
}

pub struct WifiJoinHandler {
    config_file: LedBoardConfigFile,
}

impl WifiJoinHandler {
    pub fn new(config_file: LedBoardConfigFile) -> Self {
        Self { config_file }
    }
}

impl RequestHandlerService<()> for WifiJoinHandler {
    async fn call_request_handler_service<R: Read, W: ResponseWriter<Error = R::Error>>(
        &self,
        _: &(),
        _: (),
        mut req: Request<'_, R>,
        writer: W,
    ) -> Result<ResponseSent, W::Error> {
        #[cfg(not(feature = "radio"))]
        {
            // wifi build: persist to the Pico's own config, then reboot — the
            // join loop reads config once at boot and there is no C3 to poke.
            let buffer = read_request_to_buffer!(req, writer);

            let Ok(join) = serde_json::from_slice::<JoinRequest>(&buffer) else {
                return error_response(StatusCode::BAD_REQUEST, "expected {ssid, pass}")
                    .write_to(req.body_connection.finalize().await?, writer)
                    .await;
            };

            if let Err(err) = self
                .config_file
                .add_known_wifi_network(join.ssid.clone(), join.pass.clone())
                .await
            {
                return error_response(StatusCode::INTERNAL_SERVER_ERROR, &alloc::format!("{err:?}"))
                    .write_to(req.body_connection.finalize().await?, writer)
                    .await;
            }

            let body = serde_json::json!({
                "joined": false,
                "rebooting": true,
                "ssid": join.ssid.clone(),
                "mode": "Station",
            })
            .to_string();

            json_response!(req, writer, body.as_str())?;

            // Give the response a moment to flush before the ROM reset.
            embassy_time::Timer::after_millis(1_500).await;
            embassy_rp::rom_data::reboot(0, 10, 0, 0);
            unreachable!();
        }

        #[cfg(feature = "radio")]
        {
            let buffer = read_request_to_buffer!(req, writer);

            let Ok(join) = serde_json::from_slice::<JoinRequest>(&buffer) else {
                return error_response(StatusCode::BAD_REQUEST, "expected {ssid, pass}")
                    .write_to(req.body_connection.finalize().await?, writer)
                    .await;
            };

            let Some(link) = crate::tasks::radio::link().await else {
                return error_response(StatusCode::SERVICE_UNAVAILABLE, "radio link not up")
                    .write_to(req.body_connection.finalize().await?, writer)
                    .await;
            };

            // Persist locally (source of truth stays on the Pico)...
            if let Err(err) = self
                .config_file
                .add_known_wifi_network(join.ssid.clone(), join.pass.clone())
                .await
            {
                return error_response(StatusCode::INTERNAL_SERVER_ERROR, &alloc::format!("{err:?}"))
                    .write_to(req.body_connection.finalize().await?, writer)
                    .await;
            }

            // ...then have the C3 remember it and connect right now.
            match link.join(join.ssid.clone(), join.pass.clone()).await {
                Some(status) => {
                    let body = serde_json::json!({
                        "joined": status.ip.is_some(),
                        "ssid": status.ssid,
                        "mode": alloc::format!("{:?}", status.mode),
                    })
                    .to_string();
                    json_response!(req, writer, body.as_str())
                }
                None => {
                    error_response(StatusCode::SERVICE_UNAVAILABLE, "radio did not answer")
                        .write_to(req.body_connection.finalize().await?, writer)
                        .await
                }
            }
        }
    }
}
