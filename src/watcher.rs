use std::time::{Duration, SystemTime};

use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::json;
use tokio::time;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{info, warn};

use crate::config::AppConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RawActivitySourceKind {
    WebSocketLogs,
}

impl RawActivitySourceKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::WebSocketLogs => "websocket_logs",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RawActivityKind {
    MayhemLogMatch,
}

impl RawActivityKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::MayhemLogMatch => "mayhem_log_match",
        }
    }
}

#[derive(Debug, Clone)]
pub struct RawMayhemCandidate {
    pub source_kind: RawActivitySourceKind,
    pub activity_kind: RawActivityKind,
    pub rpc_label: String,
    pub observed_at: SystemTime,
    pub signature: String,
    pub slot: u64,
    pub err: Option<String>,
    pub logs: Vec<String>,
    pub matched_program_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatcherMode {
    Connecting,
    Healthy,
    Silent,
    Disconnected,
}

impl WatcherMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Connecting => "connecting",
            Self::Healthy => "healthy",
            Self::Silent => "silent",
            Self::Disconnected => "disconnected",
        }
    }
}

#[derive(Debug, Clone)]
pub struct WatcherState {
    pub enabled: bool,
    pub source_kind: RawActivitySourceKind,
    pub rpc_label: String,
    pub mode: WatcherMode,
    pub matched_program_id: String,
    pub total_notifications_seen: u64,
    pub total_candidates_forwarded: u64,
    pub ignored_notifications: u64,
    pub reconnect_count: u64,
    pub last_event_at: Option<SystemTime>,
    pub last_signature: Option<String>,
    pub last_slot: Option<u64>,
}

impl WatcherState {
    pub fn new(config: &AppConfig) -> Self {
        Self {
            enabled: config.watcher.enabled,
            source_kind: RawActivitySourceKind::WebSocketLogs,
            rpc_label: config.network.http_rpc.label.clone(),
            mode: WatcherMode::Connecting,
            matched_program_id: config.watcher.mayhem.program_id.clone(),
            total_notifications_seen: 0,
            total_candidates_forwarded: 0,
            ignored_notifications: 0,
            reconnect_count: 0,
            last_event_at: None,
            last_signature: None,
            last_slot: None,
        }
    }

    pub fn recompute_mode(&mut self, silence_warning_after: Duration) {
        self.mode = match self.last_event_at {
            Some(last_event_at) => match SystemTime::now().duration_since(last_event_at) {
                Ok(elapsed) if elapsed > silence_warning_after => WatcherMode::Silent,
                Ok(_) => WatcherMode::Healthy,
                Err(_) => WatcherMode::Connecting,
            },
            None => {
                if self.reconnect_count == 0 {
                    WatcherMode::Connecting
                } else {
                    WatcherMode::Disconnected
                }
            }
        };
    }
}

pub async fn run_raw_chain_watcher(config: AppConfig) {
    if !config.watcher.enabled {
        info!("[WATCHER] watcher disabled by configuration");
        return;
    }

    if !config.watcher.websocket.enabled {
        warn!("[WATCHER] websocket watcher disabled by configuration");
        return;
    }

    let mut state = WatcherState::new(&config);
    let heartbeat_interval_duration = Duration::from_secs(config.watcher.heartbeat_interval_secs);
    let silence_warning_duration = Duration::from_secs(config.watcher.silence_warning_secs);
    let mut heartbeat_interval = time::interval(heartbeat_interval_duration);

    info!("============================================================");
    info!("[WATCHER] mayhem websocket watcher started");
    info!("------------------------------------------------------------");
    info!(
        watcher_enabled = config.watcher.enabled,
        websocket_enabled = config.watcher.websocket.enabled,
        source_kind = state.source_kind.as_str(),
        rpc_label = state.rpc_label.as_str(),
        websocket_url = config.watcher.websocket.url.as_str(),
        websocket_commitment = config.watcher.websocket.commitment.as_str(),
        mayhem_program_id = config.watcher.mayhem.program_id.as_str(),
        heartbeat_interval_secs = config.watcher.heartbeat_interval_secs,
        silence_warning_secs = config.watcher.silence_warning_secs,
        "[WATCHER] watcher configuration loaded"
    );

    loop {
        tokio::select! {
            _ = heartbeat_interval.tick() => {
                state.recompute_mode(silence_warning_duration);
                log_watcher_heartbeat(&state);
            }
            result = run_logs_subscription_once(&config, &mut state) => {
                if let Err(error) = result {
                    state.reconnect_count += 1;
                    state.mode = WatcherMode::Disconnected;

                    warn!(
                        rpc_label = state.rpc_label.as_str(),
                        reconnect_count = state.reconnect_count,
                        "[WATCHER][WARN] websocket watcher disconnected: {error}"
                    );

                    time::sleep(Duration::from_secs(2)).await;
                }
            }
        }
    }
}

async fn run_logs_subscription_once(
    config: &AppConfig,
    state: &mut WatcherState,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    state.mode = WatcherMode::Connecting;

    let (ws_stream, _) = connect_async(config.watcher.websocket.url.as_str()).await?;
    let (mut write, mut read) = ws_stream.split();

    let subscribe_request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "logsSubscribe",
        "params": [
            {
                "mentions": [config.watcher.mayhem.program_id]
            },
            {
                "commitment": config.watcher.websocket.commitment
            }
        ]
    });

    write
        .send(Message::Text(subscribe_request.to_string()))
        .await?;

    info!(
        websocket_url = config.watcher.websocket.url.as_str(),
        mayhem_program_id = config.watcher.mayhem.program_id.as_str(),
        "[WATCHER] logsSubscribe request sent for Mayhem program"
    );

    while let Some(message_result) = read.next().await {
        let message = message_result?;

        match message {
            Message::Text(text) => {
                if handle_ws_text_message(&text, state, config).is_err() {
                    state.ignored_notifications += 1;
                }
            }
            Message::Ping(payload) => {
                write.send(Message::Pong(payload)).await?;
            }
            Message::Close(frame) => {
                let reason = frame
                    .map(|f| format!("code={} reason={}", u16::from(f.code), f.reason))
                    .unwrap_or_else(|| "no close frame".to_string());

                return Err(format!("websocket closed: {reason}").into());
            }
            _ => {}
        }
    }

    Err("websocket stream ended".into())
}

fn handle_ws_text_message(
    text: &str,
    state: &mut WatcherState,
    config: &AppConfig,
) -> Result<(), serde_json::Error> {
    if let Some(candidate) = parse_candidate_from_ws_text_message(text, state, config)? {
        log_candidate_event(&candidate, state);
    }

    Ok(())
}

fn parse_candidate_from_ws_text_message(
    text: &str,
    state: &mut WatcherState,
    config: &AppConfig,
) -> Result<Option<RawMayhemCandidate>, serde_json::Error> {
    let parsed: serde_json::Value = serde_json::from_str(text)?;

    if parsed.get("method").and_then(|v| v.as_str()) != Some("logsNotification") {
        return Ok(None);
    }

    let notification: LogsNotification = serde_json::from_value(parsed)?;
    state.total_notifications_seen += 1;

    let candidate = RawMayhemCandidate {
        source_kind: RawActivitySourceKind::WebSocketLogs,
        activity_kind: RawActivityKind::MayhemLogMatch,
        rpc_label: state.rpc_label.clone(),
        observed_at: SystemTime::now(),
        signature: notification.params.result.value.signature.clone(),
        slot: notification.params.result.context.slot,
        err: notification.params.result.value.err.map(|e| e.to_string()),
        logs: notification.params.result.value.logs.clone(),
        matched_program_id: config.watcher.mayhem.program_id.clone(),
    };

    state.total_candidates_forwarded += 1;
    state.last_event_at = Some(candidate.observed_at);
    state.last_signature = Some(candidate.signature.clone());
    state.last_slot = Some(candidate.slot);
    state.mode = WatcherMode::Healthy;

    Ok(Some(candidate))
}

fn log_candidate_event(candidate: &RawMayhemCandidate, state: &WatcherState) {
    info!(
        source_kind = candidate.source_kind.as_str(),
        activity_kind = candidate.activity_kind.as_str(),
        rpc_label = candidate.rpc_label.as_str(),
        signature = candidate.signature.as_str(),
        slot = candidate.slot,
        has_error = candidate.err.is_some(),
        log_count = candidate.logs.len(),
        total_notifications_seen = state.total_notifications_seen,
        total_candidates_forwarded = state.total_candidates_forwarded,
        matched_program_id = candidate.matched_program_id.as_str(),
        "[WATCHER][CANDIDATE] raw Mayhem candidate observed"
    );

    info!("+==========================================================+");
    info!("| MAYHEM CANDIDATE                                          |");
    info!("+----------------------------------------------------------+");
    info!(
        "| signature={} | slot={} |",
        shorten_signature(candidate.signature.as_str()),
        candidate.slot,
    );
    info!(
        "| source={} | activity={} |",
        candidate.source_kind.as_str(),
        candidate.activity_kind.as_str(),
    );
    info!(
        "| has_error={} | log_count={} |",
        candidate.err.is_some(),
        candidate.logs.len(),
    );
    info!(
        "| program_id={} |",
        shorten_signature(candidate.matched_program_id.as_str()),
    );
    info!("+==========================================================+");
}

fn log_watcher_heartbeat(state: &WatcherState) {
    let last_event_age = format_optional_age(state.last_event_at);
    let last_slot = state
        .last_slot
        .map(|value| value.to_string())
        .unwrap_or_else(|| "unknown".to_string());

    let last_signature = state
        .last_signature
        .as_deref()
        .unwrap_or("unknown");

    info!(
        source_kind = state.source_kind.as_str(),
        rpc_label = state.rpc_label.as_str(),
        mode = state.mode.as_str(),
        total_notifications_seen = state.total_notifications_seen,
        total_candidates_forwarded = state.total_candidates_forwarded,
        ignored_notifications = state.ignored_notifications,
        reconnect_count = state.reconnect_count,
        last_slot = state.last_slot,
        last_signature = last_signature,
        matched_program_id = state.matched_program_id.as_str(),
        "[WATCHER][HEARTBEAT] mayhem watcher heartbeat"
    );

    info!("+==========================================================+");
    info!("| WATCHER: MAYHEM WEBSOCKET                                |");
    info!("+----------------------------------------------------------+");
    info!(
        "| source={} | rpc={} | mode={} |",
        state.source_kind.as_str(),
        state.rpc_label,
        state.mode.as_str(),
    );
    info!(
        "| notifications_seen={} | candidates_forwarded={} | ignored={} |",
        state.total_notifications_seen,
        state.total_candidates_forwarded,
        state.ignored_notifications,
    );
    info!(
        "| reconnects={} | last_slot={} |",
        state.reconnect_count,
        last_slot,
    );
    info!(
        "| last_event={} | last_signature={} |",
        last_event_age,
        shorten_signature(last_signature),
    );
    info!(
        "| mayhem_program_id={} |",
        shorten_signature(state.matched_program_id.as_str()),
    );
    info!("+==========================================================+");
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

fn shorten_signature(value: &str) -> String {
    if value.len() <= 12 {
        return value.to_string();
    }

    format!("{}...{}", &value[..6], &value[value.len() - 4..])
}

#[derive(Debug, Deserialize)]
struct LogsNotification {
    params: LogsNotificationParams,
}

#[derive(Debug, Deserialize)]
struct LogsNotificationParams {
    result: LogsNotificationResult,
}

#[derive(Debug, Deserialize)]
struct LogsNotificationResult {
    context: LogsNotificationContext,
    value: LogsNotificationValue,
}

#[derive(Debug, Deserialize)]
struct LogsNotificationContext {
    slot: u64,
}

#[derive(Debug, Deserialize)]
struct LogsNotificationValue {
    signature: String,
    err: Option<serde_json::Value>,
    logs: Vec<String>,
}