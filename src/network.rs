use std::time::{Duration, Instant, SystemTime};

use solana_client::rpc_client::RpcClient;
use solana_sdk::commitment_config::CommitmentConfig;
use tokio::time;
use tracing::{info, warn};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpRpcMode {
    Healthy,
    Degraded,
    Retrying,
    Unhealthy,
}

impl HttpRpcMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Healthy => "healthy",
            Self::Degraded => "degraded",
            Self::Retrying => "retrying",
            Self::Unhealthy => "unhealthy",
        }
    }
}

#[derive(Debug, Clone)]
pub struct RpcMonitorState {
    pub label: String,
    pub commitment: String,
    pub fallback_configured: bool,
    pub yellowstone_configured: bool,
    pub mode: HttpRpcMode,
    pub last_successful_health_check_at: Option<SystemTime>,
    pub last_successful_blockhash_fetch_at: Option<SystemTime>,
    pub latest_health_check_latency_ms: Option<u128>,
    pub latest_blockhash_latency_ms: Option<u128>,
    pub latest_blockhash: Option<String>,
    pub consecutive_health_check_failures: u64,
    pub consecutive_blockhash_failures: u64,
    pub last_health_retry_count: u32,
    pub last_blockhash_retry_count: u32,
    pub total_health_recoveries: u64,
    pub total_blockhash_recoveries: u64,
}

impl RpcMonitorState {
    pub fn new(config: &AppConfig) -> Self {
        Self {
            label: config.network.http_rpc.label.clone(),
            commitment: config.network.http_rpc.commitment.clone(),
            fallback_configured: config.network.fallback_http_rpc.is_some(),
            yellowstone_configured: config.network.yellowstone.is_some(),
            mode: HttpRpcMode::Healthy,
            last_successful_health_check_at: None,
            last_successful_blockhash_fetch_at: None,
            latest_health_check_latency_ms: None,
            latest_blockhash_latency_ms: None,
            latest_blockhash: None,
            consecutive_health_check_failures: 0,
            consecutive_blockhash_failures: 0,
            last_health_retry_count: 0,
            last_blockhash_retry_count: 0,
            total_health_recoveries: 0,
            total_blockhash_recoveries: 0,
        }
    }

    pub fn recompute_mode(
        &mut self,
        health_stale_after: Duration,
        blockhash_stale_after: Duration,
    ) {
        let health_is_stale = is_stale(self.last_successful_health_check_at, health_stale_after);
        let blockhash_is_stale =
            is_stale(self.last_successful_blockhash_fetch_at, blockhash_stale_after);

        self.mode = if self.consecutive_health_check_failures > 0
            && self.consecutive_blockhash_failures > 0
        {
            HttpRpcMode::Unhealthy
        } else if self.last_health_retry_count > 0 || self.last_blockhash_retry_count > 0 {
            HttpRpcMode::Degraded
        } else if health_is_stale || blockhash_is_stale {
            HttpRpcMode::Degraded
        } else {
            HttpRpcMode::Healthy
        };
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
    log_space();
    info!("============================================================");
    info!("[STARTUP][RPC] Chainstack HTTP RPC startup verification");
    info!("------------------------------------------------------------");
    log_space();

    info!(
        rpc_label = report.label.as_str(),
        rpc_url = report.url.as_str(),
        commitment = report.commitment.as_str(),
        fallback_configured = report.fallback_configured,
        yellowstone_configured = report.yellowstone_configured,
        health_check_latency_ms = report.health_check_latency_ms,
        blockhash_latency_ms = report.blockhash_latency_ms,
        latest_blockhash = report.latest_blockhash.as_str(),
        "[STARTUP][RPC] HTTP RPC startup verification passed"
    );

    info!(
        "[STARTUP][RPC] rpc={} | commitment={} | fallback={} | yellowstone={} | health={}ms | blockhash={}ms",
        report.label,
        report.commitment,
        if report.fallback_configured { "configured" } else { "not-configured" },
        if report.yellowstone_configured { "configured" } else { "not-configured" },
        report.health_check_latency_ms,
        report.blockhash_latency_ms,
    );
    log_space();
}

pub async fn run_http_rpc_monitor(config: AppConfig) {
    let rpc_config = config.network.http_rpc.clone();
    let commitment = match parse_commitment(&rpc_config.commitment) {
        Ok(commitment) => commitment,
        Err(error) => {
            warn!("[NETWORK][RPC] failed to parse rpc commitment for monitor: {error}");
            return;
        }
    };

    let client = build_rpc_client(&rpc_config, commitment);
    let mut state = RpcMonitorState::new(&config);

    log_space();
    info!("============================================================");
    info!("[NETWORK][RPC] Starting HTTP RPC monitor");
    info!("------------------------------------------------------------");
    log_space();
    info!(
        rpc_label = rpc_config.label.as_str(),
        health_check_interval_secs = rpc_config.health_check_interval_secs,
        blockhash_refresh_interval_secs = rpc_config.blockhash_refresh_interval_secs,
        heartbeat_interval_secs = rpc_config.heartbeat_interval_secs,
        max_retries = rpc_config.max_retries,
        retry_base_delay_ms = rpc_config.retry_base_delay_ms,
        retry_max_delay_ms = rpc_config.retry_max_delay_ms,
        "[NETWORK][RPC] monitor configuration loaded"
    );
    log_space();

    let mut health_interval =
        time::interval(Duration::from_secs(rpc_config.health_check_interval_secs));
    let mut blockhash_interval =
        time::interval(Duration::from_secs(rpc_config.blockhash_refresh_interval_secs));
    let mut heartbeat_interval =
        time::interval(Duration::from_secs(rpc_config.heartbeat_interval_secs));

    let health_stale_after = Duration::from_secs(rpc_config.health_check_interval_secs * 2);
    let blockhash_stale_after =
        Duration::from_secs(rpc_config.blockhash_refresh_interval_secs * 2);

    loop {
        tokio::select! {
            _ = health_interval.tick() => {
                perform_health_check(&client, &rpc_config, &mut state).await;
                state.recompute_mode(health_stale_after, blockhash_stale_after);
            }
            _ = blockhash_interval.tick() => {
                perform_blockhash_refresh(&client, &rpc_config, &mut state).await;
                state.recompute_mode(health_stale_after, blockhash_stale_after);
            }
            _ = heartbeat_interval.tick() => {
                state.recompute_mode(health_stale_after, blockhash_stale_after);
                log_heartbeat(&state, health_stale_after, blockhash_stale_after);
            }
        }
    }
}

async fn perform_health_check(
    client: &RpcClient,
    rpc_config: &HttpRpcConfig,
    state: &mut RpcMonitorState,
) {
    state.mode = HttpRpcMode::Retrying;

    match retry_rpc_operation(
        "health_check",
        rpc_config,
        || client.get_health().map(|_| ()),
        state.label.as_str(),
    )
    .await
    {
        Ok(result) => {
            let previous_failures = state.consecutive_health_check_failures;
            state.last_successful_health_check_at = Some(SystemTime::now());
            state.latest_health_check_latency_ms = Some(result.latency_ms);
            state.last_health_retry_count = result.retry_count;
            state.consecutive_health_check_failures = 0;

            if result.retry_count > 0 {
                state.total_health_recoveries += 1;
                info!(
                    rpc_label = state.label.as_str(),
                    latency_ms = result.latency_ms,
                    retry_count = result.retry_count,
                    total_recoveries = state.total_health_recoveries,
                    "[RECOVERY][RPC] health check recovered after retries"
                );
            } else if previous_failures > 0 {
                state.total_health_recoveries += 1;
                info!(
                    rpc_label = state.label.as_str(),
                    latency_ms = result.latency_ms,
                    total_recoveries = state.total_health_recoveries,
                    "[RECOVERY][RPC] health check recovered"
                );
            }
        }
        Err(error) => {
            state.consecutive_health_check_failures += 1;
            state.last_health_retry_count = rpc_config.max_retries;

            warn!(
                rpc_label = state.label.as_str(),
                consecutive_failures = state.consecutive_health_check_failures,
                retries_exhausted = rpc_config.max_retries,
                "[WARN][RPC] health check failed after retries: {error}"
            );
        }
    }
}

async fn perform_blockhash_refresh(
    client: &RpcClient,
    rpc_config: &HttpRpcConfig,
    state: &mut RpcMonitorState,
) {
    state.mode = HttpRpcMode::Retrying;

    match retry_rpc_operation(
        "blockhash_refresh",
        rpc_config,
        || client.get_latest_blockhash().map(|value| value.to_string()),
        state.label.as_str(),
    )
    .await
    {
        Ok(result) => {
            let previous_failures = state.consecutive_blockhash_failures;
            state.last_successful_blockhash_fetch_at = Some(SystemTime::now());
            state.latest_blockhash_latency_ms = Some(result.latency_ms);
            state.latest_blockhash = Some(result.value);
            state.last_blockhash_retry_count = result.retry_count;
            state.consecutive_blockhash_failures = 0;

            if result.retry_count > 0 {
                state.total_blockhash_recoveries += 1;
                info!(
                    rpc_label = state.label.as_str(),
                    latency_ms = result.latency_ms,
                    retry_count = result.retry_count,
                    latest_blockhash = state.latest_blockhash.as_deref().unwrap_or("unknown"),
                    total_recoveries = state.total_blockhash_recoveries,
                    "[RECOVERY][RPC] latest blockhash refresh recovered after retries"
                );
            } else if previous_failures > 0 {
                state.total_blockhash_recoveries += 1;
                info!(
                    rpc_label = state.label.as_str(),
                    latency_ms = result.latency_ms,
                    latest_blockhash = state.latest_blockhash.as_deref().unwrap_or("unknown"),
                    total_recoveries = state.total_blockhash_recoveries,
                    "[RECOVERY][RPC] latest blockhash refresh recovered"
                );
            }
        }
        Err(error) => {
            state.consecutive_blockhash_failures += 1;
            state.last_blockhash_retry_count = rpc_config.max_retries;

            warn!(
                rpc_label = state.label.as_str(),
                consecutive_failures = state.consecutive_blockhash_failures,
                retries_exhausted = rpc_config.max_retries,
                "[WARN][RPC] latest blockhash refresh failed after retries: {error}"
            );
        }
    }
}

#[derive(Debug)]
struct RetryOutcome<T> {
    value: T,
    latency_ms: u128,
    retry_count: u32,
}

async fn retry_rpc_operation<T, F>(
    operation_name: &str,
    rpc_config: &HttpRpcConfig,
    mut operation: F,
    rpc_label: &str,
) -> Result<RetryOutcome<T>, solana_client::client_error::ClientError>
where
    F: FnMut() -> Result<T, solana_client::client_error::ClientError>,
{
    let mut retry_count = 0;

    loop {
        let started_at = Instant::now();

        match operation() {
            Ok(value) => {
                return Ok(RetryOutcome {
                    value,
                    latency_ms: started_at.elapsed().as_millis(),
                    retry_count,
                });
            }
            Err(error) => {
                if retry_count >= rpc_config.max_retries {
                    return Err(error);
                }

                retry_count += 1;

                let delay_ms = compute_backoff_delay_ms(
                    retry_count,
                    rpc_config.retry_base_delay_ms,
                    rpc_config.retry_max_delay_ms,
                );

                warn!(
                    rpc_label,
                    operation = operation_name,
                    retry_attempt = retry_count,
                    max_retries = rpc_config.max_retries,
                    retry_delay_ms = delay_ms,
                    "[RETRY][RPC] operation failed, retrying: {error}"
                );

                time::sleep(Duration::from_millis(delay_ms)).await;
            }
        }
    }
}

fn compute_backoff_delay_ms(retry_attempt: u32, base_delay_ms: u64, max_delay_ms: u64) -> u64 {
    let multiplier = 2u64.saturating_pow(retry_attempt.saturating_sub(1));
    let delay = base_delay_ms.saturating_mul(multiplier);
    delay.min(max_delay_ms)
}

fn log_heartbeat(
    state: &RpcMonitorState,
    health_stale_after: Duration,
    blockhash_stale_after: Duration,
) {
    let health_latency = format_optional_ms(state.latest_health_check_latency_ms);
    let blockhash_latency = format_optional_ms(state.latest_blockhash_latency_ms);
    let last_health_age = format_optional_age(state.last_successful_health_check_at);
    let last_blockhash_age = format_optional_age(state.last_successful_blockhash_fetch_at);

    let health_stale = is_stale(state.last_successful_health_check_at, health_stale_after);
    let blockhash_stale = is_stale(state.last_successful_blockhash_fetch_at, blockhash_stale_after);

    let fallback = if state.fallback_configured {
        "configured"
    } else {
        "not configured"
    };

    let yellowstone = if state.yellowstone_configured {
        "configured"
    } else {
        "not configured"
    };

    info!(
        rpc_label = state.label.as_str(),
        mode = state.mode.as_str(),
        health_latency_ms = state.latest_health_check_latency_ms,
        blockhash_latency_ms = state.latest_blockhash_latency_ms,
        health_failures = state.consecutive_health_check_failures,
        blockhash_failures = state.consecutive_blockhash_failures,
        health_retry_count = state.last_health_retry_count,
        blockhash_retry_count = state.last_blockhash_retry_count,
        health_stale,
        blockhash_stale,
        latest_blockhash = state.latest_blockhash.as_deref().unwrap_or("unknown"),
        "[HEARTBEAT][RPC] HTTP RPC heartbeat"
    );

    log_space();
    info!("+==========================================================+");
    info!("| HEARTBEAT: HTTP RPC                                     |");
    info!("+----------------------------------------------------------+");
    info!(
        "| rpc={} | mode={} | commitment={} |",
        state.label,
        state.mode.as_str(),
        state.commitment,
    );
    info!(
        "| health_latency={} | blockhash_latency={} |",
        health_latency,
        blockhash_latency,
    );
    info!(
        "| last_health={} | last_blockhash={} |",
        last_health_age,
        last_blockhash_age,
    );
    info!(
        "| health_retries={} | blockhash_retries={} |",
        state.last_health_retry_count,
        state.last_blockhash_retry_count,
    );
    info!(
        "| health_failures={} | blockhash_failures={} |",
        state.consecutive_health_check_failures,
        state.consecutive_blockhash_failures,
    );
    info!(
        "| health_stale={} | blockhash_stale={} |",
        health_stale,
        blockhash_stale,
    );
    info!(
        "| fallback={} | yellowstone={} |",
        fallback,
        yellowstone,
    );
    info!("+==========================================================+");

    log_space();
}

fn log_space() {
    info!("");
}

fn is_stale(last_success: Option<SystemTime>, stale_after: Duration) -> bool {
    match last_success {
        Some(time) => match SystemTime::now().duration_since(time) {
            Ok(elapsed) => elapsed > stale_after,
            Err(_) => false,
        },
        None => true,
    }
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
            Ok(duration) => format!("{}s ago", duration.as_secs()),
            Err(_) => "time-error".to_string(),
        },
        None => "never".to_string(),
    }
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