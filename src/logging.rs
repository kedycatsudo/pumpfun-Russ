use tracing_subscriber::{fmt, EnvFilter};

use crate::{config::LoggingConfig, errors::LoggingError};

pub fn init(config: &LoggingConfig) -> Result<(), LoggingError> {
    let env_filter = EnvFilter::try_new(config.level.clone())
        .map_err(|error| LoggingError::Init(error.to_string()))?;

    fmt()
        .with_env_filter(env_filter)
        .with_target(false)
        .without_time()
        .try_init()
        .map_err(|error| LoggingError::Init(error.to_string()))?;

    Ok(())
}