use bincode::{deserialize, serialize};
use log::{debug, error};
use rocket::figment::providers::{Env, Format, Serialized, Toml};
use rocket::figment::Figment;
use serde::{Deserialize, Serialize};
use std::fs;
use thiserror::Error;

use crate::augmentation::{Augmentation, AugmentationError};

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("Could not parse config")]
    ParseError,
}

#[derive(Deserialize, Serialize)]
pub struct InternalConfig {
    pub augmentations: Vec<Augmentation>,
    pub last_badge_update: u64,
}

#[derive(Deserialize, Serialize)]
pub struct ExternalConfig {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub pass: String,
    pub vsid: i32,
    pub bind_addr: String,
    pub bind_port: u16,
}

impl Default for ExternalConfig {
    fn default() -> ExternalConfig {
        ExternalConfig {
            host: "127.0.0.1".into(),
            port: 10011,
            user: "serveradmin".into(),
            pass: "password".into(),
            vsid: 1,
            bind_addr: "0.0.0.0".into(),
            bind_port: 8000,
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct Config {
    pub internal: InternalConfig,
    pub external: ExternalConfig,
}

impl Config {
    fn read_internal_config() -> Result<InternalConfig, ConfigError> {
        // check if state.toml exists
        if let Ok(config_file) = fs::read("state.bin") {
            match deserialize::<InternalConfig>(&config_file) {
                Ok(config) => return Ok(config),
                Err(e) => {
                    error!("Could not parse state.bin: {}", e);
                    return Err(ConfigError::ParseError);
                }
            }
        }
        // create empty config file
        fs::write(
            "state.bin",
            serialize(&InternalConfig {
                augmentations: Vec::new(),
                last_badge_update: 0,
            })
            .unwrap(),
        )
        .unwrap();
        Ok(InternalConfig {
            augmentations: Vec::new(),
            last_badge_update: 0,
        })
    }

    fn read_external_config() -> Result<ExternalConfig, ConfigError> {
        match Figment::from(Serialized::defaults(ExternalConfig::default()))
            .merge(Toml::file("config.toml"))
            .merge(Env::raw().only(&["HOST", "PORT", "USER", "PASS"]))
            .extract::<ExternalConfig>()
        {
            Ok(config) => Ok(config),
            Err(e) => {
                error!("Could not load config: {}", e);
                Err(ConfigError::ParseError)
            }
        }
    }

    pub fn read_config() -> Result<Config, ConfigError> {
        // try loading internal config
        let internal = Config::read_internal_config()?;

        // load external config from env variables and config.toml
        // sensible defaults are provided
        let external = Config::read_external_config()?;

        debug!(
            "External config: Host: {}, Port: {}, User: {}, Pass: {}, BindAddr: {}, BindPort: {}",
            external.host,
            external.port,
            external.user,
            external.pass,
            external.bind_addr,
            external.bind_port
        );

        let config = Config { internal, external };
        config.write_internal_config().unwrap();
        Ok(config)
    }

    pub fn add_augmentation(&mut self, augmentation: Augmentation) {
        self.internal.augmentations.push(augmentation);
        self.write_internal_config().unwrap();
    }

    pub fn remove_augmentation(
        &mut self,
        identifier: &str,
    ) -> Result<Augmentation, AugmentationError> {
        let index = self
            .internal
            .augmentations
            .iter()
            .position(|c| c.identifier == identifier)
            .ok_or(AugmentationError::NotFound)?;
        let augmentation = self.internal.augmentations.remove(index);
        self.write_internal_config().unwrap();
        Ok(augmentation)
    }

    pub fn write_internal_config(&self) -> std::io::Result<()> {
        let data = serialize(&self.internal).unwrap();
        fs::write("state.bin", data)
    }
}
