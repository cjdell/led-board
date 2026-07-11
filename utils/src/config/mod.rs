pub mod storage;

use crate::config::storage::ConfigFileStorage;
use alloc::{format, string::String, sync::Arc};
use defmt::*;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, rwlock::RwLock};
use serde::{Serialize, de::DeserializeOwned};

#[derive(Clone)]
pub struct ConfigFile<State, STORAGE: ConfigFileStorage> {
    storage: STORAGE,
    state: Arc<RwLock<CriticalSectionRawMutex, State>>,
}

impl<STATE: Clone + DeserializeOwned + Serialize, STORAGE: ConfigFileStorage> ConfigFile<STATE, STORAGE> {
    pub async fn new(storage: STORAGE, initial: STATE) -> Self {
        let mut instance = Self {
            storage,
            state: Arc::new(RwLock::new(initial)),
        };

        instance.init().await;

        instance
    }

    async fn init(&mut self) {
        let json = match self.read_json().await {
            Ok(json) => json,
            Err(err) => {
                warn!("ConfigFile: Could not read JSON! {:?}", defmt::Debug2Format(&err));
                return;
            }
        };

        let state = match serde_json::from_str::<STATE>(&json) {
            Ok(state) => state,
            Err(err) => {
                warn!(
                    "ConfigFile: Could not decode JSON! {:?} {}",
                    defmt::Debug2Format(&err),
                    json.as_str()
                );
                return;
            }
        };

        *self.state.write().await = state;
    }

    async fn read_json(&self) -> Result<String, StateError> {
        self.storage
            .read_json()
            .await
            .map_err(|_| StateError::Error(format!("Read text file error")))
    }

    pub async fn get_json(&self) -> Result<String, StateError> {
        let state = self.state.read().await;

        serde_json::to_string::<STATE>(&state).map_err(|err| StateError::Error(format!("{err:?}")))
    }

    pub async fn set_json(&self, json: &[u8]) -> Result<(), StateError> {
        let mut state = self.state.write().await;

        *state = serde_json::from_slice::<STATE>(json).map_err(|err| StateError::Error(format!("{err:?}")))?;

        Ok(())
    }

    pub async fn get_data(&self) -> STATE {
        self.state.read().await.clone()
    }

    pub async fn set_data(&self, new_state: STATE) -> () {
        let mut state = self.state.write().await;

        *state = new_state;
    }

    pub async fn save(&self) -> Result<(), StateError> {
        let json = self.get_json().await?;

        info!("ConfigFile.save: {}", defmt::Debug2Format(&json));

        self.storage
            .write_json(json)
            .await
            .map_err(|_| StateError::Error(format!("Read text file error")))?;

        Ok(())
    }
}

#[derive(Debug)]
pub enum StateError {
    Error(String),
}
