use crate::errors::Error;
use log::{debug, info};
use rocket::figment::providers::{Env, Format, Serialized, Toml};
use rocket::figment::Figment;
use rocket::serde;
use serde::{Deserialize, Serialize};
use std::fs;

use crate::augmentation::Augmentation;

#[derive(Deserialize, Serialize, Default)]
pub struct InternalConfig {
    pub augmentations: Vec<Augmentation>,
    pub last_badge_update: u64,
    #[serde(default)]
    pub afk_channel: Option<i32>,
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
            port: 10022,
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
    fn read_internal_config() -> Result<InternalConfig, Error> {
        // check if state.toml exists
        if let Ok(config_file) = fs::read_to_string("state.ron") {
            return Ok(ron::from_str(&config_file)?);
        }
        // create empty config file
        fs::write(
            "state.ron",
            ron::ser::to_string_pretty(
                &InternalConfig {
                    augmentations: Vec::new(),
                    last_badge_update: 0,
                    afk_channel: None,
                },
                ron::ser::PrettyConfig::default(),
            )?,
        )?;
        Ok(InternalConfig {
            augmentations: Vec::new(),
            last_badge_update: 0,
            afk_channel: None,
        })
    }

    fn read_external_config() -> Result<ExternalConfig, Error> {
        Ok(
            Figment::from(Serialized::defaults(ExternalConfig::default()))
                .merge(Toml::file("config.toml"))
                .merge(Env::raw().only(&[
                    "HOST",
                    "PORT",
                    "USER",
                    "PASS",
                    "VSID",
                    "BIND_ADDR",
                    "BIND_PORT",
                ]))
                .extract::<ExternalConfig>()?,
        )
    }

    pub fn read_config() -> Result<Config, Error> {
        info!("Loading internal state from state.ron");
        // try loading internal config
        let internal = Config::read_internal_config()?;

        info!("Loading configuration from environment variables and config.toml");
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

        info!("Successfully loaded configuration");

        let config = Config { internal, external };
        config.write_internal_config()?;
        Ok(config)
    }

    pub fn add_augmentation(&mut self, augmentation: Augmentation) -> Result<(), Error> {
        self.internal.augmentations.push(augmentation);
        self.write_internal_config()?;
        Ok(())
    }

    pub fn remove_augmentation(&mut self, identifier: &str) -> Result<Augmentation, Error> {
        let index = self
            .internal
            .augmentations
            .iter()
            .position(|c| c.identifier == identifier)
            .ok_or(Error::NotFound)?;
        let augmentation = self.internal.augmentations.remove(index);
        self.write_internal_config()?;
        Ok(augmentation)
    }

    pub fn write_internal_config(&self) -> Result<(), Error> {
        let data = ron::ser::to_string_pretty(&self.internal, ron::ser::PrettyConfig::default())?;
        fs::write("state.ron", data)?;
        Ok(())
    }
}
