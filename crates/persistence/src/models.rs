use std::str::FromStr;

use chrono::{DateTime, Utc};
use domain::{
    ApprovalState, ApprovalStatus, ConditionId, DisputeState, ExecutionMode, IdentifierRecord,
    InventoryBucket, MarketId, MarketRoute, Order, OrderId, ResolutionState, ResolutionStatus,
    SettlementState, SignatureType, SignedOrderIdentity, SubmissionState, TokenId, VenueOrderState,
    WalletRoute,
};
use rust_decimal::Decimal;
use serde_json::Value;

use crate::{PersistenceError, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentifierRecordRow {
    pub event_id: String,
    pub event_family_id: String,
    pub market_id: String,
    pub condition_id: String,
    pub token_id: String,
    pub outcome_label: String,
    pub route: String,
}

impl IdentifierRecordRow {
    pub fn from_domain(record: &IdentifierRecord) -> Self {
        Self {
            event_id: record.event_id.as_str().to_owned(),
            event_family_id: record.event_family_id.as_str().to_owned(),
            market_id: record.market_id.as_str().to_owned(),
            condition_id: record.condition_id.as_str().to_owned(),
            token_id: record.token_id.as_str().to_owned(),
            outcome_label: record.outcome_label.clone(),
            route: market_route_to_str(record.route).to_owned(),
        }
    }

    pub fn into_domain(self) -> Result<IdentifierRecord> {
        Ok(IdentifierRecord {
            event_id: self.event_id.into(),
            event_family_id: self.event_family_id.into(),
            market_id: self.market_id.into(),
            condition_id: self.condition_id.into(),
            token_id: self.token_id.into(),
            outcome_label: self.outcome_label,
            route: market_route_from_str(&self.route)?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewOrderRow {
    pub order_id: String,
    pub market_id: String,
    pub condition_id: String,
    pub token_id: String,
    pub quantity: Decimal,
    pub price: Decimal,
    pub submission_state: SubmissionState,
    pub venue_state: VenueOrderState,
    pub settlement_state: SettlementState,
    pub signed_order_hash: Option<String>,
    pub salt: Option<String>,
    pub nonce: Option<String>,
    pub signature: Option<String>,
    pub retry_of_order_id: Option<String>,
}

impl NewOrderRow {
    pub fn from_domain(order: &Order, retry_of_order_id: Option<&OrderId>) -> Self {
        let signed = order.signed_order.as_ref();

        Self {
            order_id: order.order_id.as_str().to_owned(),
            market_id: order.market_id.as_str().to_owned(),
            condition_id: order.condition_id.as_str().to_owned(),
            token_id: order.token_id.as_str().to_owned(),
            quantity: order.quantity,
            price: order.price,
            submission_state: order.submission_state,
            venue_state: order.venue_state,
            settlement_state: order.settlement_state,
            signed_order_hash: signed.map(|value| value.signed_order_hash.clone()),
            salt: signed.map(|value| value.salt.clone()),
            nonce: signed.map(|value| value.nonce.clone()),
            signature: signed.map(|value| value.signature.clone()),
            retry_of_order_id: retry_of_order_id.map(|value| value.as_str().to_owned()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderRow {
    pub order_id: String,
    pub market_id: String,
    pub condition_id: String,
    pub token_id: String,
    pub quantity: Decimal,
    pub price: Decimal,
    pub submission_state: String,
    pub venue_state: String,
    pub settlement_state: String,
    pub signed_order_hash: Option<String>,
    pub salt: Option<String>,
    pub nonce: Option<String>,
    pub signature: Option<String>,
    pub retry_of_order_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl OrderRow {
    pub fn into_stored_order(self) -> Result<StoredOrder> {
        let order = Order {
            order_id: self.order_id.into(),
            market_id: MarketId::from(self.market_id),
            condition_id: ConditionId::from(self.condition_id),
            token_id: TokenId::from(self.token_id),
            quantity: self.quantity,
            price: self.price,
            submission_state: submission_state_from_str(&self.submission_state)?,
            venue_state: venue_order_state_from_str(&self.venue_state)?,
            settlement_state: settlement_state_from_str(&self.settlement_state)?,
            signed_order: signed_order_identity(
                self.signed_order_hash,
                self.salt,
                self.nonce,
                self.signature,
            )?,
        };

        Ok(StoredOrder {
            retry_of_order_id: self.retry_of_order_id.map(OrderId::from),
            order,
            created_at: self.created_at,
            updated_at: self.updated_at,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredOrder {
    pub order: Order,
    pub retry_of_order_id: Option<OrderId>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalStateRow {
    pub token_id: String,
    pub spender: String,
    pub owner_address: String,
    pub funder_address: String,
    pub wallet_route: String,
    pub signature_type: String,
    pub allowance: Decimal,
    pub required_min_allowance: Decimal,
    pub last_checked_at: DateTime<Utc>,
    pub approval_status: String,
    pub updated_at: DateTime<Utc>,
}

impl ApprovalStateRow {
    pub fn from_domain(state: &ApprovalState) -> Self {
        Self {
            token_id: state.token_id.as_str().to_owned(),
            spender: state.spender.clone(),
            owner_address: state.owner_address.clone(),
            funder_address: state.funder_address.clone(),
            wallet_route: wallet_route_to_str(state.wallet_route).to_owned(),
            signature_type: signature_type_to_str(state.signature_type).to_owned(),
            allowance: state.allowance,
            required_min_allowance: state.required_min_allowance,
            last_checked_at: state.last_checked_at,
            approval_status: approval_status_to_str(state.approval_status).to_owned(),
            updated_at: state.last_checked_at,
        }
    }

    pub fn into_domain(self) -> Result<ApprovalState> {
        Ok(ApprovalState {
            token_id: self.token_id.into(),
            spender: self.spender,
            owner_address: self.owner_address,
            funder_address: self.funder_address,
            wallet_route: wallet_route_from_str(&self.wallet_route)?,
            signature_type: signature_type_from_str(&self.signature_type)?,
            allowance: self.allowance,
            required_min_allowance: self.required_min_allowance,
            last_checked_at: self.last_checked_at,
            approval_status: approval_status_from_str(&self.approval_status)?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InventoryBucketRow {
    pub token_id: String,
    pub owner_address: String,
    pub bucket: String,
    pub quantity: Decimal,
    pub linked_order_id: Option<String>,
    pub ctf_operation_id: Option<String>,
    pub relayer_transaction_id: Option<String>,
    pub updated_at: DateTime<Utc>,
}

impl InventoryBucketRow {
    pub fn new(
        token_id: impl Into<String>,
        owner_address: impl Into<String>,
        bucket: InventoryBucket,
        quantity: Decimal,
    ) -> Self {
        Self {
            token_id: token_id.into(),
            owner_address: owner_address.into(),
            bucket: inventory_bucket_to_str(bucket).to_owned(),
            quantity,
            linked_order_id: None,
            ctf_operation_id: None,
            relayer_transaction_id: None,
            updated_at: Utc::now(),
        }
    }

    pub fn bucket(&self) -> Result<InventoryBucket> {
        inventory_bucket_from_str(&self.bucket)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolutionStateRow {
    pub condition_id: String,
    pub resolution_status: String,
    pub payout_vector: Value,
    pub resolved_at: Option<DateTime<Utc>>,
    pub dispute_state: String,
    pub redeemable_at: Option<DateTime<Utc>>,
    pub updated_at: DateTime<Utc>,
}

impl ResolutionStateRow {
    pub fn from_domain(state: &ResolutionState) -> Self {
        Self {
            condition_id: state.condition_id.as_str().to_owned(),
            resolution_status: resolution_status_to_str(state.resolution_status).to_owned(),
            payout_vector: payout_vector_to_json(&state.payout_vector),
            resolved_at: state.resolved_at,
            dispute_state: dispute_state_to_str(state.dispute_state).to_owned(),
            redeemable_at: state.redeemable_at,
            updated_at: Utc::now(),
        }
    }

    pub fn into_domain(self) -> Result<ResolutionState> {
        Ok(ResolutionState {
            condition_id: self.condition_id.into(),
            resolution_status: resolution_status_from_str(&self.resolution_status)?,
            payout_vector: payout_vector_from_json(&self.payout_vector)?,
            resolved_at: self.resolved_at,
            dispute_state: dispute_state_from_str(&self.dispute_state)?,
            redeemable_at: self.redeemable_at,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeProgressRow {
    pub last_journal_seq: i64,
    pub last_state_version: i64,
    pub last_snapshot_id: Option<String>,
    pub operator_target_revision: Option<String>,
    pub operator_strategy_revision: Option<String>,
    pub active_run_session_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunSessionState {
    Starting,
    Running,
    Exited,
    Failed,
}

impl RunSessionState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Exited => "exited",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunSessionRow {
    pub run_session_id: String,
    pub invoked_by: String,
    pub mode: String,
    pub state: RunSessionState,
    pub started_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub exit_status: Option<String>,
    pub exit_reason: Option<String>,
    pub config_path: String,
    pub config_fingerprint: String,
    pub target_source_kind: String,
    pub startup_target_revision_at_start: String,
    pub configured_operator_target_revision: Option<String>,
    pub active_operator_target_revision_at_start: Option<String>,
    pub configured_operator_strategy_revision: Option<String>,
    pub active_operator_strategy_revision_at_start: Option<String>,
    pub rollout_state_at_start: Option<String>,
    pub real_user_shadow_smoke: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StrategyCandidateSetRow {
    pub strategy_candidate_revision: String,
    pub snapshot_id: String,
    pub source_revision: String,
    pub payload: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AdoptableStrategyRevisionRow {
    pub adoptable_strategy_revision: String,
    pub strategy_candidate_revision: String,
    pub rendered_operator_strategy_revision: String,
    pub payload: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StrategyAdoptionProvenanceRow {
    pub operator_strategy_revision: String,
    pub adoptable_strategy_revision: String,
    pub strategy_candidate_revision: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperatorStrategyAdoptionHistoryRow {
    pub adoption_id: String,
    pub action_kind: String,
    pub operator_strategy_revision: String,
    pub previous_operator_strategy_revision: Option<String>,
    pub adoptable_strategy_revision: Option<String>,
    pub strategy_candidate_revision: Option<String>,
    pub adopted_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CandidateTargetSetRow {
    pub candidate_revision: String,
    pub snapshot_id: String,
    pub source_revision: String,
    pub payload: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AdoptableTargetRevisionRow {
    pub adoptable_revision: String,
    pub candidate_revision: String,
    pub rendered_operator_target_revision: String,
    pub payload: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateAdoptionProvenanceRow {
    pub operator_target_revision: String,
    pub adoptable_revision: String,
    pub candidate_revision: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperatorTargetAdoptionHistoryRow {
    pub adoption_id: String,
    pub action_kind: String,
    pub operator_target_revision: String,
    pub previous_operator_target_revision: Option<String>,
    pub adoptable_revision: Option<String>,
    pub candidate_revision: Option<String>,
    pub adopted_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotPublicationRow {
    pub snapshot_id: String,
    pub state_version: i64,
    pub committed_journal_seq: i64,
    pub fullset_ready: bool,
    pub negrisk_ready: bool,
    pub metadata: Value,
    pub published_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionAttemptRow {
    pub attempt_id: String,
    pub plan_id: String,
    pub snapshot_id: String,
    pub route: String,
    pub scope: String,
    pub matched_rule_id: Option<String>,
    pub execution_mode: ExecutionMode,
    pub attempt_no: i32,
    pub idempotency_key: String,
    pub run_session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionAttemptWithCreatedAtRow {
    pub attempt: ExecutionAttemptRow,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PendingReconcileRow {
    pub pending_ref: String,
    pub scope_kind: String,
    pub scope_id: String,
    pub reason: String,
    pub payload: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LiveSubmissionRecordRow {
    pub submission_ref: String,
    pub attempt_id: String,
    pub route: String,
    pub scope: String,
    pub provider: String,
    pub state: String,
    pub payload: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ShadowExecutionArtifactRow {
    pub attempt_id: String,
    pub stream: String,
    pub payload: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LiveExecutionArtifactRow {
    pub attempt_id: String,
    pub stream: String,
    pub payload: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NegRiskFamilyMemberRow {
    pub condition_id: String,
    pub token_id: String,
    pub outcome_label: String,
    pub is_placeholder: bool,
    pub is_other: bool,
    pub neg_risk_variant: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NegRiskFamilyValidationRow {
    pub event_family_id: String,
    pub validation_status: String,
    pub exclusion_reason: Option<String>,
    pub metadata_snapshot_hash: String,
    pub last_seen_discovery_revision: i64,
    pub member_count: i32,
    pub first_seen_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
    pub validated_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub member_vector: Vec<NegRiskFamilyMemberRow>,
    pub source_kind: String,
    pub source_session_id: String,
    pub source_event_id: String,
    pub event_ts: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FamilyHaltRow {
    pub event_family_id: String,
    pub halted: bool,
    pub reason: Option<String>,
    pub blocks_new_risk: bool,
    pub metadata_snapshot_hash: Option<String>,
    pub last_seen_discovery_revision: i64,
    pub set_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub member_vector: Vec<NegRiskFamilyMemberRow>,
    pub source_kind: String,
    pub source_session_id: String,
    pub source_event_id: String,
    pub event_ts: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NegRiskDiscoverySnapshotInput {
    pub discovery_revision: i64,
    pub metadata_snapshot_hash: String,
    pub family_ids: Vec<String>,
    pub captured_at: DateTime<Utc>,
    pub source_kind: String,
    pub source_session_id: String,
    pub source_event_id: String,
    pub dedupe_key: String,
    pub extra_payload: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct JournalEntryInput {
    pub stream: String,
    pub source_kind: String,
    pub source_session_id: String,
    pub source_event_id: String,
    pub dedupe_key: String,
    pub causal_parent_id: Option<i64>,
    pub event_type: String,
    pub event_ts: DateTime<Utc>,
    pub payload: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct JournalEntryRow {
    pub journal_seq: i64,
    pub stream: String,
    pub source_kind: String,
    pub source_session_id: String,
    pub source_event_id: String,
    pub dedupe_key: String,
    pub causal_parent_id: Option<i64>,
    pub event_type: String,
    pub event_ts: DateTime<Utc>,
    pub payload: Value,
    pub ingested_at: DateTime<Utc>,
}

fn signed_order_identity(
    signed_order_hash: Option<String>,
    salt: Option<String>,
    nonce: Option<String>,
    signature: Option<String>,
) -> Result<Option<SignedOrderIdentity>> {
    match (signed_order_hash, salt, nonce, signature) {
        (None, None, None, None) => Ok(None),
        (Some(signed_order_hash), Some(salt), Some(nonce), Some(signature)) => {
            Ok(Some(SignedOrderIdentity {
                signed_order_hash,
                salt,
                nonce,
                signature,
            }))
        }
        _ => Err(PersistenceError::IncompleteSignedOrderIdentity),
    }
}

fn payout_vector_to_json(values: &[Decimal]) -> Value {
    Value::Array(
        values
            .iter()
            .map(|value| Value::String(value.normalize().to_string()))
            .collect(),
    )
}

fn payout_vector_from_json(value: &Value) -> Result<Vec<Decimal>> {
    let items = value
        .as_array()
        .ok_or_else(|| PersistenceError::invalid_value("payout_vector", value.to_string()))?;

    items
        .iter()
        .map(|item| match item {
            Value::String(value) => Decimal::from_str(value)
                .map_err(|_| PersistenceError::invalid_value("payout_vector", value.clone())),
            Value::Number(value) => Decimal::from_str(&value.to_string())
                .map_err(|_| PersistenceError::invalid_value("payout_vector", value.to_string())),
            _ => Err(PersistenceError::invalid_value(
                "payout_vector",
                item.to_string(),
            )),
        })
        .collect()
}

fn market_route_to_str(value: MarketRoute) -> &'static str {
    match value {
        MarketRoute::Standard => "standard",
        MarketRoute::NegRisk => "negrisk",
    }
}

fn market_route_from_str(value: &str) -> Result<MarketRoute> {
    match value {
        "standard" => Ok(MarketRoute::Standard),
        "negrisk" => Ok(MarketRoute::NegRisk),
        _ => Err(PersistenceError::invalid_value("market_route", value)),
    }
}

fn submission_state_from_str(value: &str) -> Result<SubmissionState> {
    match value {
        "draft" => Ok(SubmissionState::Draft),
        "planned" => Ok(SubmissionState::Planned),
        "risk_approved" => Ok(SubmissionState::RiskApproved),
        "signed" => Ok(SubmissionState::Signed),
        "submitted" => Ok(SubmissionState::Submitted),
        "acked" => Ok(SubmissionState::Acked),
        "rejected" => Ok(SubmissionState::Rejected),
        "unknown" => Ok(SubmissionState::Unknown),
        _ => Err(PersistenceError::invalid_value("submission_state", value)),
    }
}

fn venue_order_state_from_str(value: &str) -> Result<VenueOrderState> {
    match value {
        "live" => Ok(VenueOrderState::Live),
        "matched" => Ok(VenueOrderState::Matched),
        "delayed" => Ok(VenueOrderState::Delayed),
        "unmatched" => Ok(VenueOrderState::Unmatched),
        "cancel_pending" => Ok(VenueOrderState::CancelPending),
        "cancelled" => Ok(VenueOrderState::Cancelled),
        "expired" => Ok(VenueOrderState::Expired),
        "unknown" => Ok(VenueOrderState::Unknown),
        _ => Err(PersistenceError::invalid_value("venue_state", value)),
    }
}

fn settlement_state_from_str(value: &str) -> Result<SettlementState> {
    match value {
        "matched" => Ok(SettlementState::Matched),
        "mined" => Ok(SettlementState::Mined),
        "confirmed" => Ok(SettlementState::Confirmed),
        "retrying" => Ok(SettlementState::Retrying),
        "failed" => Ok(SettlementState::Failed),
        "unknown" => Ok(SettlementState::Unknown),
        _ => Err(PersistenceError::invalid_value("settlement_state", value)),
    }
}

fn wallet_route_to_str(value: WalletRoute) -> &'static str {
    match value {
        WalletRoute::Eoa => "eoa",
        WalletRoute::Proxy => "proxy",
        WalletRoute::Safe => "safe",
    }
}

fn wallet_route_from_str(value: &str) -> Result<WalletRoute> {
    match value {
        "eoa" => Ok(WalletRoute::Eoa),
        "proxy" => Ok(WalletRoute::Proxy),
        "safe" => Ok(WalletRoute::Safe),
        _ => Err(PersistenceError::invalid_value("wallet_route", value)),
    }
}

fn signature_type_to_str(value: SignatureType) -> &'static str {
    match value {
        SignatureType::Eoa => "eoa",
        SignatureType::Proxy => "proxy",
        SignatureType::Safe => "safe",
    }
}

fn signature_type_from_str(value: &str) -> Result<SignatureType> {
    match value {
        "eoa" => Ok(SignatureType::Eoa),
        "proxy" => Ok(SignatureType::Proxy),
        "safe" => Ok(SignatureType::Safe),
        _ => Err(PersistenceError::invalid_value("signature_type", value)),
    }
}

fn approval_status_to_str(value: ApprovalStatus) -> &'static str {
    match value {
        ApprovalStatus::Unknown => "unknown",
        ApprovalStatus::Missing => "missing",
        ApprovalStatus::Pending => "pending",
        ApprovalStatus::Approved => "approved",
        ApprovalStatus::Rejected => "rejected",
    }
}

fn approval_status_from_str(value: &str) -> Result<ApprovalStatus> {
    match value {
        "unknown" => Ok(ApprovalStatus::Unknown),
        "missing" => Ok(ApprovalStatus::Missing),
        "pending" => Ok(ApprovalStatus::Pending),
        "approved" => Ok(ApprovalStatus::Approved),
        "rejected" => Ok(ApprovalStatus::Rejected),
        _ => Err(PersistenceError::invalid_value("approval_status", value)),
    }
}

fn inventory_bucket_to_str(value: InventoryBucket) -> &'static str {
    match value {
        InventoryBucket::Free => "free",
        InventoryBucket::ReservedForOrder => "reserved_for_order",
        InventoryBucket::MatchedUnsettled => "matched_unsettled",
        InventoryBucket::PendingCtfIn => "pending_ctf_in",
        InventoryBucket::PendingCtfOut => "pending_ctf_out",
        InventoryBucket::Redeemable => "redeemable",
        InventoryBucket::Quarantined => "quarantined",
    }
}

fn inventory_bucket_from_str(value: &str) -> Result<InventoryBucket> {
    match value {
        "free" => Ok(InventoryBucket::Free),
        "reserved_for_order" => Ok(InventoryBucket::ReservedForOrder),
        "matched_unsettled" => Ok(InventoryBucket::MatchedUnsettled),
        "pending_ctf_in" => Ok(InventoryBucket::PendingCtfIn),
        "pending_ctf_out" => Ok(InventoryBucket::PendingCtfOut),
        "redeemable" => Ok(InventoryBucket::Redeemable),
        "quarantined" => Ok(InventoryBucket::Quarantined),
        _ => Err(PersistenceError::invalid_value("inventory_bucket", value)),
    }
}

fn resolution_status_to_str(value: ResolutionStatus) -> &'static str {
    match value {
        ResolutionStatus::Unresolved => "unresolved",
        ResolutionStatus::Resolved => "resolved",
        ResolutionStatus::Cancelled => "cancelled",
    }
}

fn resolution_status_from_str(value: &str) -> Result<ResolutionStatus> {
    match value {
        "unresolved" => Ok(ResolutionStatus::Unresolved),
        "resolved" => Ok(ResolutionStatus::Resolved),
        "cancelled" => Ok(ResolutionStatus::Cancelled),
        _ => Err(PersistenceError::invalid_value("resolution_status", value)),
    }
}

fn dispute_state_to_str(value: DisputeState) -> &'static str {
    match value {
        DisputeState::None => "none",
        DisputeState::Disputed => "disputed",
        DisputeState::Challenged => "challenged",
        DisputeState::UnderReview => "under_review",
    }
}

fn dispute_state_from_str(value: &str) -> Result<DisputeState> {
    match value {
        "none" => Ok(DisputeState::None),
        "disputed" => Ok(DisputeState::Disputed),
        "challenged" => Ok(DisputeState::Challenged),
        "under_review" => Ok(DisputeState::UnderReview),
        _ => Err(PersistenceError::invalid_value("dispute_state", value)),
    }
}

pub(crate) fn execution_mode_to_str(value: ExecutionMode) -> &'static str {
    match value {
        ExecutionMode::Disabled => "disabled",
        ExecutionMode::Shadow => "shadow",
        ExecutionMode::Live => "live",
        ExecutionMode::ReduceOnly => "reduce_only",
        ExecutionMode::RecoveryOnly => "recovery_only",
    }
}

pub(crate) fn execution_mode_from_str(value: &str) -> Result<ExecutionMode> {
    match value {
        "disabled" => Ok(ExecutionMode::Disabled),
        "shadow" => Ok(ExecutionMode::Shadow),
        "live" => Ok(ExecutionMode::Live),
        "reduce_only" => Ok(ExecutionMode::ReduceOnly),
        "recovery_only" => Ok(ExecutionMode::RecoveryOnly),
        _ => Err(PersistenceError::invalid_value("execution_mode", value)),
    }
}
