use crate::types::LedBoardConfigFile;
use alloc::string::ToString as _;
use alloc::vec;
use alloc::{string::String, vec::Vec};
use serde::{Deserialize, Serialize};
use utils::config::StateError;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)] // This will merge in defaults for new properties and effectively allow migrations
pub struct LedBoardConfig {
    pub wifi_mode: WifiMode,
    pub ap_ssid: String,
    pub known_wifi_networks: Vec<KnownWifiNetwork>,
}

impl Default for LedBoardConfig {
    fn default() -> Self {
        Self {
            wifi_mode: WifiMode::Station,
            ap_ssid: "LED Board".to_string(),
            known_wifi_networks: vec![],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WifiMode {
    Station,
    AccessPoint,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnownWifiNetwork {
    pub ssid: String,
    pub pass: String,
}

pub trait LedBoardConfigurator {
    fn add_known_wifi_network(&self, ssid: String, pass: String) -> impl Future<Output = Result<(), StateError>>;
}

impl LedBoardConfigurator for LedBoardConfigFile {
    async fn add_known_wifi_network(&self, ssid: String, pass: String) -> Result<(), StateError> {
        let mut data = self.get_data().await;
        let mut found = false;

        for known_wifi_network in &mut data.known_wifi_networks {
            if known_wifi_network.ssid == ssid {
                found = true;
                known_wifi_network.pass = pass.clone();
            }
        }

        if !found {
            data.known_wifi_networks.push(KnownWifiNetwork {
                ssid: ssid.clone(),
                pass: pass.clone(),
            });
        }

        self.set_data(data).await;

        self.save().await?;

        Ok(())
    }
}
