use std::{fs, path::Path};

use serde::Deserialize;

use crate::errors::ConfigError;

#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    pub runtime: RuntimeConfig,
    pub logging: LoggingConfig,
    pub wallet: WalletConfig,
    pub network: NetworkConfig,
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

        if self.network.http_rpc.url.trim().is_empty() {
            return Err(ConfigError::Validation(
                "network.http_rpc.url must not be empty".to_string(),
            ));
        }

        if self.network.http_rpc.request_timeout_secs == 0 {
            return Err(ConfigError::Validation(
                "network.http_rpc.request_timeout_secs must be greater than 0".to_string(),
            ));
        }

        if self.network.http_rpc.health_check_interval_secs == 0 {
            return Err(ConfigError::Validation(
                "network.http_rpc.health_check_interval_secs must be greater than 0".to_string(),
            ));
        }

        if self.network.http_rpc.blockhash_refresh_interval_secs == 0 {
            return Err(ConfigError::Validation(
                "network.http_rpc.blockhash_refresh_interval_secs must be greater than 0"
                    .to_string(),
            ));
        }

        if self.network.http_rpc.commitment.trim().is_empty() {
            return Err(ConfigError::Validation(
                "network.http_rpc.commitment must not be empty".to_string(),
            ));
        }
        if self.network.http_rpc.label.trim().is_empty() {
            return Err(ConfigError::Validation(
        "network.http_rpc.label must not be empty".to_string(),
             ));
        }
        if self.network.http_rpc.heartbeat_interval_secs == 0 {
            return Err(ConfigError::Validation(
        "network.http_rpc.heartbeat_interval_secs must be greater than 0".to_string(),
            ));
        }


        if let Some(yellowstone) = &self.network.yellowstone {
            if yellowstone.endpoint.trim().is_empty() {
                return Err(ConfigError::Validation(
                    "network.yellowstone.endpoint must not be empty when yellowstone is provided"
                        .to_string(),
                ));
            }

            if yellowstone.connect_timeout_secs == 0 {
                return Err(ConfigError::Validation(
                    "network.yellowstone.connect_timeout_secs must be greater than 0".to_string(),
                ));
            }

            if yellowstone.reconnect_base_delay_secs == 0 {
                return Err(ConfigError::Validation(
                    "network.yellowstone.reconnect_base_delay_secs must be greater than 0"
                        .to_string(),
                ));
            }

            if yellowstone.reconnect_max_delay_secs == 0 {
                return Err(ConfigError::Validation(
                    "network.yellowstone.reconnect_max_delay_secs must be greater than 0"
                        .to_string(),
                ));
            }

            if yellowstone.stream_liveness_timeout_secs == 0 {
                return Err(ConfigError::Validation(
                    "network.yellowstone.stream_liveness_timeout_secs must be greater than 0"
                        .to_string(),
                ));
            }

            if yellowstone.reconnect_base_delay_secs > yellowstone.reconnect_max_delay_secs {
                return Err(ConfigError::Validation(
                    "network.yellowstone.reconnect_base_delay_secs must be less than or equal to network.yellowstone.reconnect_max_delay_secs".to_string(),
                ));
            }
        }

        if let Some(fallback) = &self.network.fallback_http_rpc {
            if fallback.url.trim().is_empty() {
                return Err(ConfigError::Validation(
                    "network.fallback_http_rpc.url must not be empty when provided".to_string(),
                ));
                
            }
            if fallback.label.trim().is_empty() {
        return Err(ConfigError::Validation(
            "network.fallback_http_rpc.label must not be empty when provided".to_string(),
        ));
    }
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

#[derive(Debug, Clone, Deserialize)]
pub struct NetworkConfig {
    pub http_rpc: HttpRpcConfig,
    pub yellowstone: Option<YellowstoneConfig>,
    pub fallback_http_rpc: Option<HttpRpcEndpointConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HttpRpcConfig {
    pub label: String,
    pub url: String,
    pub request_timeout_secs: u64,
    pub health_check_interval_secs: u64,
    pub blockhash_refresh_interval_secs: u64,
    pub heartbeat_interval_secs: u64,
    pub commitment: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HttpRpcEndpointConfig {
    pub label: String,
    pub url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct YellowstoneConfig {
    pub endpoint: String,
    pub auth_token: Option<String>,
    pub connect_timeout_secs: u64,
    pub reconnect_base_delay_secs: u64,
    pub reconnect_max_delay_secs: u64,
    pub stream_liveness_timeout_secs: u64,
}