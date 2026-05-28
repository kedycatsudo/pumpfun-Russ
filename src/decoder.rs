use std::{
    collections::HashSet,
    sync::Arc,
    time::SystemTime,
};

use reqwest::Client;
use serde_json::{json, Value};
use tokio::sync::Mutex;
use tracing::{info, warn};

use crate::{
    config::AppConfig,
    watcher::{RawActivitySourceKind, RawMayhemCandidate},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodedEventKind {
    Creation,
    OtherMayhem,
    Unknown,
}

impl DecodedEventKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Creation => "creation",
            Self::OtherMayhem => "other_mayhem",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MayhemStatus {
    On,
    Off,
    Paused,
    Unknown,
}

impl MayhemStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::On => "on",
            Self::Off => "off",
            Self::Paused => "paused",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeConfidence {
    High,
    Medium,
    Low,
}

impl DecodeConfidence {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::High => "high",
            Self::Medium => "medium",
            Self::Low => "low",
        }
    }
}

#[derive(Debug, Clone)]
pub struct DecodedMayhemEvent {
    pub signature: String,
    pub slot: u64,
    pub observed_at: SystemTime,
    pub mint: Option<String>,
    pub event_kind: DecodedEventKind,
    pub is_creation_event: bool,
    pub is_first_seen_mint: bool,
    pub mayhem_status: MayhemStatus,
    pub source_kind: RawActivitySourceKind,
    pub matched_program_id: String,
    pub relevant_accounts: Vec<String>,
    pub decode_confidence: DecodeConfidence,
}

#[derive(Debug, Default)]
pub struct DecoderState {
    seen_mints: HashSet<String>,
}

pub type SharedDecoderState = Arc<Mutex<DecoderState>>;

pub fn new_shared_decoder_state() -> SharedDecoderState {
    Arc::new(Mutex::new(DecoderState::default()))
}

pub async fn decode_raw_mayhem_candidate(
    client: &Client,
    config: &AppConfig,
    decoder_state: &SharedDecoderState,
    candidate: &RawMayhemCandidate,
) -> Option<DecodedMayhemEvent> {
    let tx = match fetch_transaction_json(client, config, candidate).await {
        Ok(value) => value,
        Err(error) => {
            warn!(
                signature = candidate.signature.as_str(),
                slot = candidate.slot,
                "[DECODER][WARN] failed to fetch transaction for candidate: {error}"
            );
            return None;
        }
    };

    let mint = extract_primary_mint(&tx);
    let relevant_accounts = extract_relevant_accounts(&tx);
    let creation_like = looks_like_creation(&tx, candidate, mint.as_deref());
    let is_first_seen_mint = mark_and_check_first_seen_mint(decoder_state, mint.as_deref()).await;

    let event_kind = if creation_like && is_first_seen_mint {
        DecodedEventKind::Creation
    } else if !candidate.logs.is_empty() {
        DecodedEventKind::OtherMayhem
    } else {
        DecodedEventKind::Unknown
    };

    let mayhem_status = classify_mayhem_status(&tx, candidate);
    let decode_confidence =
        classify_decode_confidence(&mint, event_kind, mayhem_status, is_first_seen_mint);

    let decoded = DecodedMayhemEvent {
        signature: candidate.signature.clone(),
        slot: candidate.slot,
        observed_at: candidate.observed_at,
        mint,
        event_kind,
        is_creation_event: matches!(event_kind, DecodedEventKind::Creation),
        is_first_seen_mint,
        mayhem_status,
        source_kind: candidate.source_kind,
        matched_program_id: candidate.matched_program_id.clone(),
        relevant_accounts,
        decode_confidence,
    };

    log_decoded_event(&decoded);

    Some(decoded)
}

async fn mark_and_check_first_seen_mint(
    decoder_state: &SharedDecoderState,
    mint: Option<&str>,
) -> bool {
    let Some(mint) = mint else {
        return false;
    };

    let mut state = decoder_state.lock().await;

    if state.seen_mints.contains(mint) {
        false
    } else {
        state.seen_mints.insert(mint.to_string());
        true
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

fn extract_primary_mint(tx: &Value) -> Option<String> {
    let post_balances = tx
        .pointer("/result/meta/postTokenBalances")
        .and_then(|value| value.as_array())?;

    let pre_balances = tx
        .pointer("/result/meta/preTokenBalances")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();

    let mut pre_mints = std::collections::HashSet::new();
    for entry in &pre_balances {
        if let Some(mint) = entry.get("mint").and_then(|value| value.as_str()) {
            pre_mints.insert(mint.to_string());
        }
    }

    let mut post_mints = Vec::new();
    for entry in post_balances {
        if let Some(mint) = entry.get("mint").and_then(|value| value.as_str()) {
            let mint = mint.to_string();
            if !post_mints.contains(&mint) {
                post_mints.push(mint);
            }
        }
    }

    if post_mints.is_empty() {
        return None;
    }

    if let Some(new_mint) = post_mints.iter().find(|mint| !pre_mints.contains(*mint)) {
        return Some(new_mint.clone());
    }

    post_mints.first().cloned()
}

fn extract_relevant_accounts(tx: &Value) -> Vec<String> {
    let mut accounts = Vec::new();

    if let Some(account_keys) = tx
        .pointer("/result/transaction/message/accountKeys")
        .and_then(|value| value.as_array())
    {
        for entry in account_keys {
            if let Some(pubkey) = entry.get("pubkey").and_then(|value| value.as_str()) {
                accounts.push(pubkey.to_string());
            } else if let Some(pubkey) = entry.as_str() {
                accounts.push(pubkey.to_string());
            }
        }
    }

    if accounts.len() > 12 {
        accounts.truncate(12);
    }

    accounts
}

fn looks_like_creation(
    tx: &Value,
    candidate: &RawMayhemCandidate,
    mint: Option<&str>,
) -> bool {
    let logs_lower = candidate
        .logs
        .iter()
        .map(|log| log.to_ascii_lowercase())
        .collect::<Vec<_>>();

    let has_initialize_mint_log = logs_lower.iter().any(|log| {
        log.contains("initialize mint") || log.contains("initializemint")
    });

    let has_mint_to_log = logs_lower.iter().any(|log| {
        log.contains("mintto") || log.contains("mint_to")
    });

    let has_new_mint = mint.is_some()
        && tx
            .pointer("/result/meta/postTokenBalances")
            .and_then(|value| value.as_array())
            .map(|balances| !balances.is_empty())
            .unwrap_or(false);

    has_initialize_mint_log || has_mint_to_log || has_new_mint
}

fn classify_mayhem_status(
    _tx: &Value,
    candidate: &RawMayhemCandidate,
) -> MayhemStatus {
    let logs_lower = candidate
        .logs
        .iter()
        .map(|log| log.to_ascii_lowercase())
        .collect::<Vec<_>>();

    if logs_lower.iter().any(|log| log.contains("pause")) {
        return MayhemStatus::Paused;
    }

    if logs_lower.iter().any(|log| log.contains("disable") || log.contains(" off")) {
        return MayhemStatus::Off;
    }

    MayhemStatus::On
}

fn classify_decode_confidence(
    mint: &Option<String>,
    event_kind: DecodedEventKind,
    mayhem_status: MayhemStatus,
    is_first_seen_mint: bool,
) -> DecodeConfidence {
    match (mint.is_some(), event_kind, mayhem_status, is_first_seen_mint) {
        (true, DecodedEventKind::Creation, MayhemStatus::On, true) => DecodeConfidence::High,
        (true, _, _, _) => DecodeConfidence::Medium,
        _ => DecodeConfidence::Low,
    }
}

fn log_decoded_event(event: &DecodedMayhemEvent) {
    let mint_display = event.mint.as_deref().unwrap_or("unknown");
    let accounts_preview = if event.relevant_accounts.is_empty() {
        "none".to_string()
    } else {
        event.relevant_accounts
            .iter()
            .take(3)
            .map(|value| shorten(value))
            .collect::<Vec<_>>()
            .join(", ")
    };

    info!("+==========================================================+");
    info!("| PHASE 4: DECODED MAYHEM EVENT                            |");
    info!("+----------------------------------------------------------+");
    info!(
        "| signature={} | slot={} |",
        shorten(event.signature.as_str()),
        event.slot,
    );
    info!(
        "| mint={} | event_kind={} |",
        shorten(mint_display),
        event.event_kind.as_str(),
    );
    info!(
        "| creation={} | first_seen={} | mayhem_status={} |",
        event.is_creation_event,
        event.is_first_seen_mint,
        event.mayhem_status.as_str(),
    );
    info!(
        "| confidence={} | accounts={} |",
        event.decode_confidence.as_str(),
        accounts_preview,
    );
    info!(
        "| source={} | program_id={} |",
        event.source_kind.as_str(),
        shorten(event.matched_program_id.as_str()),
    );
    info!("+==========================================================+");
}

fn shorten(value: &str) -> String {
    if value.len() <= 12 {
        return value.to_string();
    }

    format!("{}...{}", &value[..6], &value[value.len() - 4..])
}