use tracing::info;

use crate::{
    config::AppConfig,
    errors::AppError,
    network,
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

    let rpc_report = network::verify_http_rpc(&config)?;
    network::log_rpc_startup_report(&rpc_report);

    let state = AppState::new(config.clone(), wallet);
    print_startup_summary(&state);

    tokio::spawn(network::run_http_rpc_monitor(config));

    info!("application started");
    shutdown::wait_for_signal().await;
    info!("application shutdown complete");

    Ok(())
}

fn print_startup_summary(state: &AppState) {
    let fallback_status = if state.config.network.fallback_http_rpc.is_some() {
        "configured"
    } else {
        "not configured"
    };

    let yellowstone_status = if state.config.network.yellowstone.is_some() {
        "configured"
    } else {
        "not configured"
    };

    info!(
        app_name = state.runtime.app_name.as_str(),
        environment = state.runtime.environment.as_str(),
        rpc_label = state.config.network.http_rpc.label.as_str(),
        rpc_commitment = state.config.network.http_rpc.commitment.as_str(),
        fallback_rpc = fallback_status,
        yellowstone = yellowstone_status,
        "startup summary"
    );

    info!(
        "startup | app={} | env={} | rpc={} | commitment={} | fallback={} | yellowstone={}",
        state.runtime.app_name,
        state.runtime.environment,
        state.config.network.http_rpc.label,
        state.config.network.http_rpc.commitment,
        fallback_status,
        yellowstone_status,
    );
}