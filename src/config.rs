use std::{fs, path::Path};

use serde::Deserialize;

use crate::errors::ConfigError;

#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    pub runtime: RuntimeConfig,
    pub logging: LoggingConfig,
    pub wallet: WalletConfig,
}

impl AppConfig {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let path_ref = path.as_ref();

        let raw = fs::read_to_string(path_ref).map_err(|source| ConfigError::Read {
            path: path_ref.display().to_string(),
            source,
        })?;

        let config: AppConfig = toml::from_str(&raw).map_err(|source| ConfigError::Parse {
            path: path_ref.display().to_string(),
            source,
        })?;

        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<(), ConfigError> {
        if self.runtime.app_name.trim().is_empty() {
            return Err(ConfigError::Validation(
                "runtime.app_name must not be empty".to_string(),
            ));
        }

        if self.runtime.environment.trim().is_empty() {
            return Err(ConfigError::Validation(
                "runtime.environment must not be empty".to_string(),
            ));
        }

        if self.logging.level.trim().is_empty() {
            return Err(ConfigError::Validation(
                "logging.level must not be empty".to_string(),
            ));
        }

        if self.wallet.keypair_path.trim().is_empty() {
            return Err(ConfigError::Validation(
                "wallet.keypair_path must not be empty".to_string(),
            ));
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct RuntimeConfig {
    pub app_name: String,
    pub environment: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LoggingConfig {
    pub level: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WalletConfig {
    pub keypair_path: String,
}