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

    let watcher_enabled = if state.config.watcher.enabled { "enabled" } else { "disabled" };
    let watcher_websocket_enabled = if state.config.watcher.websocket.enabled {
        "enabled"
    } else {
        "disabled"
    };

    info!("+==========================================================+");
    info!("| STARTUP SUMMARY                                           |");
    info!("+----------------------------------------------------------+");
    info!("| [STARTUP]                                                |");
    info!("| app_name={} |", state.runtime.app_name);
    info!("| environment={} |", state.runtime.environment);
    info!("+----------------------------------------------------------+");
    info!("| [WALLET]                                                 |");
    info!("| wallet_mode={} |", state.config.wallet.mode);
    info!("| wallet_path={} |", state.wallet.keypair_path);
    info!("| wallet_pubkey={} |", state.wallet.pubkey_short);
    info!("+----------------------------------------------------------+");
    info!("| [STARTUP RPC]                                            |");
    info!("| rpc_label={} |", state.config.network.http_rpc.label);
    info!(
        "| rpc_commitment={} |",
        state.config.network.http_rpc.commitment,
    );
    info!("+----------------------------------------------------------+");
    info!("| [NETWORK]                                                |");
    info!("| fallback_rpc={} |", fallback_status);
    info!("| yellowstone={} |", yellowstone_status);
    info!("+----------------------------------------------------------+");
    info!("| [WATCHER]                                                |");
    info!("| watcher={} |", watcher_enabled);
    info!("| watcher_websocket={} |", watcher_websocket_enabled);
    info!("| watcher_source=websocket_logs |");
    info!(
        "| watcher_program_id={} |",
        state.config.watcher.mayhem.program_id,
    );
    info!(
        "| watcher_heartbeat_interval_secs={} |",
        state.config.watcher.heartbeat_interval_secs,
    );
    info!(
        "| watcher_silence_warning_secs={} |",
        state.config.watcher.silence_warning_secs,
    );
    info!("+==========================================================+");
    log_space();
}

fn log_space() {
    info!("");
}