use tracing::info;

use crate::{
    config::AppConfig,
    errors::AppError,
    network,
    shutdown,
    state::AppState,
    wallet::LoadedWallet,
    watcher,
};

pub async fn run() -> Result<(), AppError> {
    let config = AppConfig::load("config/default.toml")?;

    crate::logging::init(&config.logging)?;

    log_space();
    info!("============================================================");
    info!("[STARTUP] pumpfun-sniper boot sequence");
    info!("============================================================");
    log_space();

    info!("[STARTUP][CONFIG] configuration loaded");
    log_space();

    let wallet = LoadedWallet::load(&config.wallet.keypair_path)?;
    info!(
        wallet_mode = config.wallet.mode.as_str(),
        wallet_path = wallet.keypair_path.as_str(),
        wallet_pubkey = wallet.pubkey.to_string(),
        wallet_pubkey_short = wallet.pubkey_short.as_str(),
        "[STARTUP][WALLET] wallet loaded successfully"
    );
    log_space();

    let rpc_report = network::verify_http_rpc(&config)?;
    network::log_rpc_startup_report(&rpc_report);
    log_space();

    let state = AppState::new(config.clone(), wallet);
    print_startup_summary(&state);
    log_space();

    tokio::spawn(network::run_http_rpc_monitor(config.clone()));
    tokio::spawn(watcher::run_raw_chain_watcher(config.clone()));

    info!("------------------------------------------------------------");
    info!("[STARTUP] application started");
    log_space();
    shutdown::wait_for_signal().await;
    log_space();

    info!("============================================================");
    info!("[SHUTDOWN] application shutdown complete");
    info!("============================================================");
    log_space();

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

    info!("------------------------------------------------------------");
    info!("[STARTUP][SUMMARY] runtime summary");
    log_space();
    info!(
        app_name = state.runtime.app_name.as_str(),
        environment = state.runtime.environment.as_str(),
        wallet_mode = state.config.wallet.mode.as_str(),
        wallet_path = state.wallet.keypair_path.as_str(),
        wallet_pubkey = state.wallet.pubkey.to_string(),
        wallet_pubkey_short = state.wallet.pubkey_short.as_str(),
        rpc_label = state.config.network.http_rpc.label.as_str(),
        rpc_commitment = state.config.network.http_rpc.commitment.as_str(),
        fallback_rpc = fallback_status,
        yellowstone = yellowstone_status,
        "[STARTUP][SUMMARY] startup summary"
    );

    info!(
        "[STARTUP][SUMMARY] app={} | env={} | wallet_mode={} | wallet={} | wallet_path={} | rpc={} | commitment={} | fallback={} | yellowstone={}",
        state.runtime.app_name,
        state.runtime.environment,
        state.config.wallet.mode,
        state.wallet.pubkey_short,
        state.wallet.keypair_path,
        state.config.network.http_rpc.label,
        state.config.network.http_rpc.commitment,
        fallback_status,
        yellowstone_status,
    );
    log_space();
}

fn log_space() {
    info!("");
}