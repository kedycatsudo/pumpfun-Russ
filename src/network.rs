use std::time::Instant;

use solana_client::rpc_client::RpcClient;
use solana_sdk::commitment_config::CommitmentConfig;
use tracing::info;

use crate::{
    config::{AppConfig, HttpRpcConfig},
    errors::AppError,
};

#[derive(Debug, Clone)]
pub struct RpcStartupReport {
    pub url: String,
    pub commitment: String,
    pub health_check_latency_ms: u128,
    pub blockhash_latency_ms: u128,
    pub latest_blockhash: String,
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
        url: rpc_config.url.clone(),
        commitment: rpc_config.commitment.clone(),
        health_check_latency_ms,
        blockhash_latency_ms,
        latest_blockhash: latest_blockhash.to_string(),
    })
}

pub fn log_rpc_startup_report(report: &RpcStartupReport) {
    info!(
        rpc_url = report.url.as_str(),
        commitment = report.commitment.as_str(),
        health_check_latency_ms = report.health_check_latency_ms,
        blockhash_latency_ms = report.blockhash_latency_ms,
        latest_blockhash = report.latest_blockhash.as_str(),
        "http rpc startup verification passed"
    );
}

fn build_rpc_client(config: &HttpRpcConfig, commitment: CommitmentConfig) -> RpcClient {
    RpcClient::new_with_timeout_and_commitment(
        config.url.clone(),
        std::time::Duration::from_secs(config.request_timeout_secs),
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