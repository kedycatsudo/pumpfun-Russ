use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("configuration error: {0}")]
    Config(#[from] ConfigError),

    #[error("logging error: {0}")]
    Logging(#[from] LoggingError),

    #[error("wallet error: {0}")]
    Wallet(#[from] WalletError),

    #[error("rpc error: {0}")]
    Rpc(#[from] solana_client::client_error::ClientError),
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("failed to read config file at '{path}': {source}")]
    Read {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to parse config file at '{path}': {source}")]
    Parse {
        path: String,
        #[source]
        source: toml::de::Error,
    },

    #[error("invalid configuration: {0}")]
    Validation(String),
}

#[derive(Debug, Error)]
pub enum LoggingError {
    #[error("failed to initialize logging: {0}")]
    Init(String),
}

#[derive(Debug, Error)]
pub enum WalletError {
    #[error("wallet file at '{path}' is invalid: {message}")]
    Parse {
        path: String,
        message: String,
    },
}
