use std::{
    collections::HashSet,
    sync::Arc,
    time::SystemTime,
};

use tokio::sync::Mutex;
use tracing::info;


#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandidateDecisionKind {
    Accepted,
    Rejected,
    Ignored,
}

impl CandidateDecisionKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::Rejected => "rejected",
            Self::Ignored => "ignored",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandidateDecisionReason {
    Qualified,
    MissingMint,
    NotCreation,
    NotFirstSeen,
    MayhemNotOn,
    DecodeConfidenceTooLow,
    AlreadyArmed,
}

impl CandidateDecisionReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Qualified => "qualified",
            Self::MissingMint => "missing_mint",
            Self::NotCreation => "not_creation",
            Self::NotFirstSeen => "not_first_seen",
            Self::MayhemNotOn => "mayhem_not_on",
            Self::DecodeConfidenceTooLow => "decode_confidence_too_low",
            Self::AlreadyArmed => "already_armed",
        }
    }
}

#[derive(Debug, Clone)]
pub struct MayhemTrackingRefs {
    pub mint: String,
    pub matched_program_id: String,
    pub relevant_accounts: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ArmedTradeCandidate {
    pub mint: String,
    pub signature: String,
    pub slot: u64,
    pub armed_at: SystemTime,
    pub initial_mayhem_status: MayhemStatus,
    pub decode_confidence: DecodeConfidence,
    pub matched_program_id: String,
    pub relevant_accounts: Vec<String>,
    pub mayhem_tracking_refs: MayhemTrackingRefs,
}

#[derive(Debug, Clone)]
pub struct CandidateDecision {
    pub kind: CandidateDecisionKind,
    pub reason: CandidateDecisionReason,
    pub mint: Option<String>,
    pub signature: String,
    pub slot: u64,
    pub armed_candidate: Option<ArmedTradeCandidate>,
}

#[derive(Debug, Default)]
pub struct QualifierState {
    armed_mints: HashSet<String>,
}

pub type SharedQualifierState = Arc<Mutex<QualifierState>>;

pub fn new_shared_qualifier_state() -> SharedQualifierState {
    Arc::new(Mutex::new(QualifierState::default()))
}

pub async fn qualify_decoded_event(
    qualifier_state: &SharedQualifierState,
    event: &DecodedMayhemEvent,
) -> CandidateDecision {
    let Some(mint) = event.mint.clone() else {
        let decision = CandidateDecision {
            kind: CandidateDecisionKind::Rejected,
            reason: CandidateDecisionReason::MissingMint,
            mint: None,
            signature: event.signature.clone(),
            slot: event.slot,
            armed_candidate: None,
        };
        log_candidate_decision(&decision);
        return decision;
    };

    if event.event_kind != DecodedEventKind::Creation || !event.is_creation_event {
        let decision = CandidateDecision {
            kind: CandidateDecisionKind::Rejected,
            reason: CandidateDecisionReason::NotCreation,
            mint: Some(mint),
            signature: event.signature.clone(),
            slot: event.slot,
            armed_candidate: None,
        };
        log_candidate_decision(&decision);
        return decision;
    }

    if !event.is_first_seen_mint {
        let decision = CandidateDecision {
            kind: CandidateDecisionKind::Rejected,
            reason: CandidateDecisionReason::NotFirstSeen,
            mint: Some(mint),
            signature: event.signature.clone(),
            slot: event.slot,
            armed_candidate: None,
        };
        log_candidate_decision(&decision);
        return decision;
    }

    if event.mayhem_status != MayhemStatus::On {
        let decision = CandidateDecision {
            kind: CandidateDecisionKind::Rejected,
            reason: CandidateDecisionReason::MayhemNotOn,
            mint: Some(mint),
            signature: event.signature.clone(),
            slot: event.slot,
            armed_candidate: None,
        };
        log_candidate_decision(&decision);
        return decision;
    }

    if event.decode_confidence != DecodeConfidence::High {
        let decision = CandidateDecision {
            kind: CandidateDecisionKind::Rejected,
            reason: CandidateDecisionReason::DecodeConfidenceTooLow,
            mint: Some(mint),
            signature: event.signature.clone(),
            slot: event.slot,
            armed_candidate: None,
        };
        log_candidate_decision(&decision);
        return decision;
    }

    {
        let mut state = qualifier_state.lock().await;
        if state.armed_mints.contains(&mint) {
            let decision = CandidateDecision {
                kind: CandidateDecisionKind::Ignored,
                reason: CandidateDecisionReason::AlreadyArmed,
                mint: Some(mint),
                signature: event.signature.clone(),
                slot: event.slot,
                armed_candidate: None,
            };
            log_candidate_decision(&decision);
            return decision;
        }

        state.armed_mints.insert(mint.clone());
    }

    let tracking_refs = MayhemTrackingRefs {
        mint: mint.clone(),
        matched_program_id: event.matched_program_id.clone(),
        relevant_accounts: event.relevant_accounts.clone(),
    };

    let armed = ArmedTradeCandidate {
        mint: mint.clone(),
        signature: event.signature.clone(),
        slot: event.slot,
        armed_at: SystemTime::now(),
        initial_mayhem_status: event.mayhem_status,
        decode_confidence: event.decode_confidence,
        matched_program_id: event.matched_program_id.clone(),
        relevant_accounts: event.relevant_accounts.clone(),
        mayhem_tracking_refs: tracking_refs,
    };

    let decision = CandidateDecision {
        kind: CandidateDecisionKind::Accepted,
        reason: CandidateDecisionReason::Qualified,
        mint: Some(mint),
        signature: event.signature.clone(),
        slot: event.slot,
        armed_candidate: Some(armed),
    };

    log_candidate_decision(&decision);
    decision
}

fn log_candidate_decision(decision: &CandidateDecision) {
    let mint = decision.mint.as_deref().unwrap_or("unknown");

    info!("+==========================================================+");
    info!("| PHASE 5: CANDIDATE DECISION                              |");
    info!("+----------------------------------------------------------+");
    info!(
        "| signature={} | slot={} |",
        shorten(decision.signature.as_str()),
        decision.slot,
    );
    info!(
        "| mint={} | decision={} |",
        shorten(mint),
        decision.kind.as_str(),
    );
    info!("| reason={} |", decision.reason.as_str());

    if let Some(armed) = &decision.armed_candidate {
        info!(
            "| armed=true | initial_mayhem_status={} | confidence={} |",
            armed.initial_mayhem_status.as_str(),
            armed.decode_confidence.as_str(),
        );
        info!(
            "| tracking_accounts={} | program_id={} |",
            armed.mayhem_tracking_refs.relevant_accounts.len(),
            shorten(armed.matched_program_id.as_str()),
        );
    } else {
        info!("| armed=false |");
    }

    info!("+==========================================================+");
}

fn shorten(value: &str) -> String {
    if value.len() <= 12 {
        return value.to_string();
    }

    format!("{}...{}", &value[..6], &value[value.len() - 4..])
}