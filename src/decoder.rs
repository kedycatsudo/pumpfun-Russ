use std::{
    path::PathBuf,
    time::SystemTime,
};

use reqwest::Client;
use serde::Serialize;
use serde_json::{json, Value};
use tokio::fs;
use tracing::{info, warn};

use crate::{
    config::AppConfig,
    watcher::RawMayhemCandidate,
};

pub async fn capture_log_messages_only(
    client: &Client,
    config: &AppConfig,
    candidate: &RawMayhemCandidate,
) {
    let tx = match fetch_transaction_json(client, config, candidate).await {
        Ok(value) => value,
        Err(error) => {
            warn!(
                signature = candidate.signature.as_str(),
                slot = candidate.slot,
                "[DECODER][WARN] failed to fetch transaction for creation candidate: {error}"
            );
            return;
        }
    };

    if let Err(error) = dump_mapped_transaction_artifacts(candidate, &tx).await {
        warn!(
            signature = candidate.signature.as_str(),
            slot = candidate.slot,
            "[DECODER][WARN] failed to write mapped transaction artifacts: {error}"
        );
    }
}

async fn fetch_transaction_json(
    client: &Client,
    config: &AppConfig,
    candidate: &RawMayhemCandidate,
) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
    let request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getTransaction",
        "params": [
            candidate.signature,
            {
                "encoding": "jsonParsed",
                "commitment": config.network.http_rpc.commitment,
                "maxSupportedTransactionVersion": 0
            }
        ]
    });

    let response = client
        .post(config.network.http_rpc.url.as_str())
        .json(&request)
        .send()
        .await?;

    let response_json: Value = response.json().await?;

    if let Some(error) = response_json.get("error") {
        return Err(format!("rpc getTransaction returned error: {error}").into());
    }

    Ok(response_json)
}

async fn dump_mapped_transaction_artifacts(
    candidate: &RawMayhemCandidate,
    tx: &Value,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let first4 = candidate.signature.chars().take(4).collect::<String>();
    let short_sig = candidate.signature.chars().take(8).collect::<String>();

    let dir = PathBuf::from("artifacts").join(format!("{}_{}", first4, short_sig));
    fs::create_dir_all(&dir).await?;

    let summary = ExtractionSummary {
        signature: candidate.signature.clone(),
        slot: candidate.slot,
        observed_at_unix_secs: candidate
            .observed_at
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
        source_kind: candidate.source_kind.as_str().to_string(),
        matched_program_id: candidate.matched_program_id.clone(),
        candidate_error: candidate.err.clone(),
        websocket_logs: candidate.logs.clone(),
        block_time: tx.pointer("/result/blockTime").and_then(|v| v.as_i64()),
        transaction_index: tx.pointer("/result/transactionIndex").and_then(|v| v.as_u64()),
        version: tx.pointer("/result/version").cloned().unwrap_or(Value::Null),
        meta_err: tx.pointer("/result/meta/err").cloned().unwrap_or(Value::Null),
        meta_status: tx.pointer("/result/meta/status").cloned().unwrap_or(Value::Null),
    };

    let top_level_instructions = tx
        .pointer("/result/transaction/message/instructions")
        .cloned()
        .unwrap_or(Value::Array(vec![]));

    let inner_instructions = tx
        .pointer("/result/meta/innerInstructions")
        .cloned()
        .unwrap_or(Value::Array(vec![]));

    let account_keys = tx
        .pointer("/result/transaction/message/accountKeys")
        .cloned()
        .unwrap_or(Value::Array(vec![]));

    let address_table_lookups = tx
        .pointer("/result/transaction/message/addressTableLookups")
        .cloned()
        .unwrap_or(Value::Array(vec![]));

    let token_balances = json!({
        "preTokenBalances": tx.pointer("/result/meta/preTokenBalances").cloned().unwrap_or(Value::Array(vec![])),
        "postTokenBalances": tx.pointer("/result/meta/postTokenBalances").cloned().unwrap_or(Value::Array(vec![])),
    });

    let sol_balances = json!({
        "preBalances": tx.pointer("/result/meta/preBalances").cloned().unwrap_or(Value::Array(vec![])),
        "postBalances": tx.pointer("/result/meta/postBalances").cloned().unwrap_or(Value::Array(vec![])),
    });

    let log_messages = tx
        .pointer("/result/meta/logMessages")
        .cloned()
        .unwrap_or(Value::Array(vec![]));

    let program_data_logs = extract_program_data_logs(&log_messages);

    write_json_file(dir.join("summary.json"), &summary).await?;
    write_json_value_file(dir.join(format!("{}_log_messages.json", first4)), &log_messages).await?;
    write_json_value_file(dir.join("top_level_instructions.json"), &top_level_instructions).await?;
    write_json_value_file(dir.join("inner_instructions.json"), &inner_instructions).await?;
    write_json_value_file(dir.join("account_keys.json"), &account_keys).await?;
    write_json_value_file(dir.join("address_table_lookups.json"), &address_table_lookups).await?;
    write_json_value_file(dir.join("token_balances.json"), &token_balances).await?;
    write_json_value_file(dir.join("sol_balances.json"), &sol_balances).await?;
    write_json_value_file(dir.join("program_data_logs.json"), &program_data_logs).await?;

    info!(
        signature = candidate.signature.as_str(),
        slot = candidate.slot,
        artifact_dir = dir.display().to_string(),
        "[DECODER] mapped transaction artifacts written"
    );

    Ok(())
}

fn extract_program_data_logs(log_messages: &Value) -> Value {
    let extracted = log_messages
        .as_array()
        .map(|logs| {
            logs.iter()
                .filter_map(|entry| entry.as_str())
                .filter(|line| line.starts_with("Program data:"))
                .map(|line| Value::String(line.to_string()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    Value::Array(extracted)
}

async fn write_json_file<T: Serialize>(
    path: PathBuf,
    data: &T,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let pretty = serde_json::to_string_pretty(data)?;
    fs::write(path, pretty).await?;
    Ok(())
}

async fn write_json_value_file(
    path: PathBuf,
    data: &Value,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let pretty = serde_json::to_string_pretty(data)?;
    fs::write(path, pretty).await?;
    Ok(())
}

#[derive(Debug, Serialize)]
struct ExtractionSummary {
    signature: String,
    slot: u64,
    observed_at_unix_secs: u64,
    source_kind: String,
    matched_program_id: String,
    candidate_error: Option<String>,
    websocket_logs: Vec<String>,
    block_time: Option<i64>,
    transaction_index: Option<u64>,
    version: Value,
    meta_err: Value,
    meta_status: Value,
}