//! `GET/POST /api/wifi` — WiFi scan + connect, backed by the ESP32-C3 slave
//! radio when the `radio` feature is enabled.
//!
//! The response shapes match the web app's `WifiResultSchema`, so the existing
//! `/wifi` provisioning UI works against either backend.

use alloc::string::{String, ToString as _};
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

pub struct WifiScanHandler {
    config_file: LedBoardConfigFile,
}

impl WifiScanHandler {
    pub fn new(config_file: LedBoardConfigFile) -> Self {
        Self { config_file }
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
            let _ = &self.config_file;
            error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                "wifi scanning requires the radio build",
            )
            .write_to(req.body_connection.finalize().await?, writer)
            .await
        }
    }
}

#[derive(Deserialize)]
pub struct JoinRequest {
    pub ssid: String,
    #[serde(default)]
    pub pass: String,
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
            let _ = (&self.config_file, &mut req);
            error_response(StatusCode::SERVICE_UNAVAILABLE, "wifi joining requires the radio build")
                .write_to(req.body_connection.finalize().await?, writer)
                .await
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
