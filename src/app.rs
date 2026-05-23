use tracing::{error, info};

use crate::{
    config::AppConfig,
    errors::AppError,
    shutdown,
    state::AppState,
    wallet::LoadedWallet,
};

pub async fn run() -> Result<(), AppError> {
    let config = AppConfig::load("config/default.toml")?;

    crate::logging::init(&config.logging)?;

    info!("configuration loaded");

    let wallet = LoadedWallet::load(&config.wallet.keypair_path)?;
    info!("wallet loaded successfully");

    let state = AppState::new(config, wallet);
    print_startup_summary(&state);

    info!("application started");
    shutdown::wait_for_signal().await;
    info!("application shutdown complete");

    Ok(())
}

fn print_startup_summary(state: &AppState) {
    info!(
        app_name = state.runtime.app_name.as_str(),
        environment = state.runtime.environment.as_str(),
        "startup summary"
    );
}
