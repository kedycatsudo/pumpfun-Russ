use std::time::{Duration, Instant, SystemTime};

use solana_client::rpc_client::RpcClient;
use solana_sdk::commitment_config::CommitmentConfig;
use tokio::time;
use tracing::{error, info, warn};

use crate::{
    config::{AppConfig, HttpRpcConfig},
    errors::AppError,
};

#[derive(Debug, Clone)]
pub struct RpcStartupReport {
    pub label: String,
    pub url: String,
    pub commitment: String,
    pub fallback_configured: bool,
    pub yellowstone_configured: bool,
    pub health_check_latency_ms: u128,
    pub blockhash_latency_ms: u128,
    pub latest_blockhash: String,
}

#[derive(Debug, Clone)]
pub struct RpcMonitorState {
    pub label: String,
    pub commitment: String,
    pub fallback_configured: bool,
    pub yellowstone_configured: bool,
    pub last_successful_health_check_at: Option<SystemTime>,
    pub last_successful_blockhash_fetch_at: Option<SystemTime>,
    pub latest_health_check_latency_ms: Option<u128>,
    pub latest_blockhash_latency_ms: Option<u128>,
    pub latest_blockhash: Option<String>,
    pub consecutive_health_check_failures: u64,
    pub consecutive_blockhash_failures: u64,
}

impl RpcMonitorState {
    pub fn new(config: &AppConfig) -> Self {
        Self {
            label: config.network.http_rpc.label.clone(),
            commitment: config.network.http_rpc.commitment.clone(),
            fallback_configured: config.network.fallback_http_rpc.is_some(),
            yellowstone_configured: config.network.yellowstone.is_some(),
            last_successful_health_check_at: None,
            last_successful_blockhash_fetch_at: None,
            latest_health_check_latency_ms: None,
            latest_blockhash_latency_ms: None,
            latest_blockhash: None,
            consecutive_health_check_failures: 0,
            consecutive_blockhash_failures: 0,
        }
    }

    pub fn health_status(&self) -> &'static str {
        if self.consecutive_health_check_failures > 0 || self.consecutive_blockhash_failures > 0 {
            "degraded"
        } else {
            "ok"
        }
    }
}

pub fn verify_http_rpc(config: &AppConfig) -> Result<RpcStartupReport, AppError> {
    let rpc_config = &config.network.http_rpc;
    let commitment = parse_commitment(&rpc_config.commitment)?;

    let client = build_rpc_client(rpc_config, commitment);

    let health_start = Instant::now();
    client.get_health()?;
    let health_check_latency_ms = health_start.elapsed().as_millis();

    let blockhash_start = Instant::now();
    let latest_blockhash = client.get_latest_blockhash()?;
    let blockhash_latency_ms = blockhash_start.elapsed().as_millis();

    Ok(RpcStartupReport {
        label: rpc_config.label.clone(),
        url: rpc_config.url.clone(),
        commitment: rpc_config.commitment.clone(),
        fallback_configured: config.network.fallback_http_rpc.is_some(),
        yellowstone_configured: config.network.yellowstone.is_some(),
        health_check_latency_ms,
        blockhash_latency_ms,
        latest_blockhash: latest_blockhash.to_string(),
    })
}

pub fn log_rpc_startup_report(report: &RpcStartupReport) {
    info!(
        rpc_label = report.label.as_str(),
        rpc_url = report.url.as_str(),
        commitment = report.commitment.as_str(),
        fallback_configured = report.fallback_configured,
        yellowstone_configured = report.yellowstone_configured,
        health_check_latency_ms = report.health_check_latency_ms,
        blockhash_latency_ms = report.blockhash_latency_ms,
        latest_blockhash = report.latest_blockhash.as_str(),
        "http rpc startup verification passed"
    );

    info!(
        "rpc [{}] healthy | commitment={} | fallback={} | yellowstone={} | health={}ms | blockhash={}ms",
        report.label,
        report.commitment,
        if report.fallback_configured { "configured" } else { "not-configured" },
        if report.yellowstone_configured { "configured" } else { "not-configured" },
        report.health_check_latency_ms,
        report.blockhash_latency_ms,
    );
}

pub async fn run_http_rpc_monitor(config: AppConfig) {
    let rpc_config = config.network.http_rpc.clone();
    let commitment = match parse_commitment(&rpc_config.commitment) {
        Ok(commitment) => commitment,
        Err(error) => {
            error!("failed to parse rpc commitment for monitor: {error}");
            return;
        }
    };

    let client = build_rpc_client(&rpc_config, commitment);
    let mut state = RpcMonitorState::new(&config);

    info!(
        rpc_label = rpc_config.label.as_str(),
        health_check_interval_secs = rpc_config.health_check_interval_secs,
        blockhash_refresh_interval_secs = rpc_config.blockhash_refresh_interval_secs,
        heartbeat_interval_secs = rpc_config.heartbeat_interval_secs,
        "starting http rpc monitor"
    );

    let mut health_interval =
        time::interval(Duration::from_secs(rpc_config.health_check_interval_secs));
    let mut blockhash_interval =
        time::interval(Duration::from_secs(rpc_config.blockhash_refresh_interval_secs));
    let mut heartbeat_interval =
        time::interval(Duration::from_secs(rpc_config.heartbeat_interval_secs));

    loop {
        tokio::select! {
            _ = health_interval.tick() => {
                perform_health_check(&client, &mut state);
            }
            _ = blockhash_interval.tick() => {
                perform_blockhash_refresh(&client, &mut state);
            }
            _ = heartbeat_interval.tick() => {
                log_heartbeat(&state);
            }
        }
    }
}

fn perform_health_check(client: &RpcClient, state: &mut RpcMonitorState) {
    let started_at = Instant::now();

    match client.get_health() {
        Ok(_) => {
            let latency_ms = started_at.elapsed().as_millis();
            let previous_failures = state.consecutive_health_check_failures;

            state.last_successful_health_check_at = Some(SystemTime::now());
            state.latest_health_check_latency_ms = Some(latency_ms);
            state.consecutive_health_check_failures = 0;

            if previous_failures > 0 {
                info!(
                    rpc_label = state.label.as_str(),
                    latency_ms,
                    "rpc health check recovered"
                );
            }
        }
        Err(error) => {
            state.consecutive_health_check_failures += 1;

            warn!(
                rpc_label = state.label.as_str(),
                consecutive_failures = state.consecutive_health_check_failures,
                "rpc health check failed: {error}"
            );
        }
    }
}

fn perform_blockhash_refresh(client: &RpcClient, state: &mut RpcMonitorState) {
    let started_at = Instant::now();

    match client.get_latest_blockhash() {
        Ok(blockhash) => {
            let latency_ms = started_at.elapsed().as_millis();
            let previous_failures = state.consecutive_blockhash_failures;

            state.last_successful_blockhash_fetch_at = Some(SystemTime::now());
            state.latest_blockhash_latency_ms = Some(latency_ms);
            state.latest_blockhash = Some(blockhash.to_string());
            state.consecutive_blockhash_failures = 0;

            if previous_failures > 0 {
                info!(
                    rpc_label = state.label.as_str(),
                    latency_ms,
                    latest_blockhash = state.latest_blockhash.as_deref().unwrap_or("unknown"),
                    "latest blockhash fetch recovered"
                );
            }
        }
        Err(error) => {
            state.consecutive_blockhash_failures += 1;

            warn!(
                rpc_label = state.label.as_str(),
                consecutive_failures = state.consecutive_blockhash_failures,
                "latest blockhash fetch failed: {error}"
            );
        }
    }
}

fn log_heartbeat(state: &RpcMonitorState) {
    let health_latency = format_optional_ms(state.latest_health_check_latency_ms);
    let blockhash_latency = format_optional_ms(state.latest_blockhash_latency_ms);
    let last_health_age = format_optional_age(state.last_successful_health_check_at);
    let last_blockhash_age = format_optional_age(state.last_successful_blockhash_fetch_at);

    info!(
        rpc_label = state.label.as_str(),
        status = state.health_status(),
        health_latency_ms = state.latest_health_check_latency_ms,
        blockhash_latency_ms = state.latest_blockhash_latency_ms,
        health_failures = state.consecutive_health_check_failures,
        blockhash_failures = state.consecutive_blockhash_failures,
        latest_blockhash = state.latest_blockhash.as_deref().unwrap_or("unknown"),
        "http rpc heartbeat"
    );

    info!(
        "heartbeat | rpc={} | commitment={} | status={} | health_latency={} | blockhash_latency={} | last_health={} | last_blockhash={} | health_failures={} | blockhash_failures={} | fallback={} | yellowstone={}",
        state.label,
        state.commitment,
        state.health_status(),
        health_latency,
        blockhash_latency,
        last_health_age,
        last_blockhash_age,
        state.consecutive_health_check_failures,
        state.consecutive_blockhash_failures,
        if state.fallback_configured { "configured" } else { "not configured" },
        if state.yellowstone_configured { "configured" } else { "not configured" },
    );
}

fn format_optional_ms(value: Option<u128>) -> String {
    match value {
        Some(ms) => format!("{ms}ms"),
        None => "n/a".to_string(),
    }
}

fn format_optional_age(value: Option<SystemTime>) -> String {
    match value {
        Some(time) => match SystemTime::now().duration_since(time) {
            Ok(duration) => format_duration_secs(duration.as_secs()),
            Err(_) => "time-error".to_string(),
        },
        None => "never".to_string(),
    }
}

fn format_duration_secs(secs: u64) -> String {
    format!("{secs}s ago")
}

fn build_rpc_client(config: &HttpRpcConfig, commitment: CommitmentConfig) -> RpcClient {
    RpcClient::new_with_timeout_and_commitment(
        config.url.clone(),
        Duration::from_secs(config.request_timeout_secs),
        commitment,
    )
}

fn parse_commitment(value: &str) -> Result<CommitmentConfig, AppError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "processed" => Ok(CommitmentConfig::processed()),
        "confirmed" => Ok(CommitmentConfig::confirmed()),
        "finalized" => Ok(CommitmentConfig::finalized()),
        other => Err(AppError::Config(crate::errors::ConfigError::Validation(
            format!(
                "network.http_rpc.commitment must be one of: processed, confirmed, finalized; got '{other}'"
            ),
        ))),
    }
}