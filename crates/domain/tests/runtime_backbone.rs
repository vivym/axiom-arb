use chrono::{TimeZone, Utc};
use domain::{
    ActivationDecision, DecisionInput, ExecutionAttempt, ExecutionAttemptContext,
    ExecutionAttemptOutcome, ExecutionMode, ExecutionPlanRef, ExecutionReceipt, ExecutionRequest,
    ExternalFactEvent, ExternalFactPayload, ExternalFactPayloadData, IntentCandidate,
    RecoveryIntent, StateConfidence,
};

#[test]
fn decision_contracts_stay_input_neutral_and_attempt_scoped() {
    let strategy = DecisionInput::Strategy(IntentCandidate::new(
        "intent-1",
        "snapshot-7",
        "neg-risk",
        "full-set",
    ));
    let recovery = DecisionInput::Recovery(RecoveryIntent::new(
        "recovery-1",
        "snapshot-7",
        "family-lock",
    ));

    assert_eq!(strategy.decision_input_id(), "intent-1");
    assert_eq!(recovery.decision_input_id(), "recovery-1");

    let attempt = ExecutionAttempt::new("attempt-1", "plan-1", "snapshot-7", 1);
    assert_eq!(attempt.attempt_id.as_str(), "attempt-1");
    assert_eq!(attempt.plan_id.as_str(), "plan-1");

    let request = ExecutionRequest {
        request_id: "request-1".to_owned(),
        decision_input_id: strategy.decision_input_id().to_owned(),
        snapshot_id: "snapshot-7".to_owned(),
        route: "neg-risk".to_owned(),
        scope: "full-set".to_owned(),
        activation_mode: ExecutionMode::Shadow,
        matched_rule_id: Some("family-a-shadow".to_owned()),
    };
    assert_eq!(request.request_id, "request-1");
    assert_eq!(request.decision_input_id, "intent-1");
    assert_eq!(request.snapshot_id, "snapshot-7");
    assert_eq!(request.route, "neg-risk");
    assert_eq!(request.scope, "full-set");
    assert_eq!(request.activation_mode, ExecutionMode::Shadow);
    assert_eq!(request.matched_rule_id.as_deref(), Some("family-a-shadow"));

    let plan_ref = ExecutionPlanRef {
        plan_id: "plan-1".to_owned(),
        request_id: request.request_id.clone(),
    };
    assert_eq!(plan_ref.plan_id, "plan-1");
    assert_eq!(plan_ref.request_id, "request-1");

    let attempt_context = ExecutionAttemptContext {
        attempt_id: attempt.attempt_id.clone(),
        snapshot_id: attempt.snapshot_id.clone(),
        execution_mode: ExecutionMode::Shadow,
        route: "neg-risk".to_owned(),
        scope: "full-set".to_owned(),
        matched_rule_id: Some("family-a-shadow".to_owned()),
    };
    assert_eq!(attempt_context.execution_mode, ExecutionMode::Shadow);
    assert_eq!(attempt_context.snapshot_id, "snapshot-7");
    assert_eq!(attempt_context.route, "neg-risk");
    assert_eq!(attempt_context.scope, "full-set");
    assert_eq!(
        attempt_context.matched_rule_id.as_deref(),
        Some("family-a-shadow")
    );

    let state_confidence = StateConfidence::Certain;
    assert_eq!(state_confidence, StateConfidence::Certain);

    let receipt = ExecutionReceipt {
        attempt_id: attempt_context.attempt_id.clone(),
        outcome: ExecutionAttemptOutcome::FailedAmbiguous,
        submission_ref: None,
        pending_ref: Some("pending-1".to_owned()),
    };
    assert_eq!(receipt.attempt_id, "attempt-1");
    assert_eq!(receipt.outcome, ExecutionAttemptOutcome::FailedAmbiguous);
    assert_eq!(receipt.pending_ref.as_deref(), Some("pending-1"));

    let shadow_receipt = ExecutionReceipt {
        attempt_id: attempt_context.attempt_id.clone(),
        outcome: ExecutionAttemptOutcome::ShadowRecorded,
        submission_ref: None,
        pending_ref: None,
    };
    assert_eq!(
        shadow_receipt.outcome,
        ExecutionAttemptOutcome::ShadowRecorded
    );
}

#[test]
fn execution_request_preserves_route_scope_and_activation_anchor() {
    let intent = IntentCandidate::new("intent-1", "snapshot-7", "neg-risk", "family-a");
    assert_eq!(intent.route, "neg-risk");
    assert_eq!(intent.scope, "family-a");

    let request = ExecutionRequest {
        request_id: "request-1".to_owned(),
        decision_input_id: intent.intent_id.clone(),
        snapshot_id: "snapshot-7".to_owned(),
        route: "neg-risk".to_owned(),
        scope: "family-a".to_owned(),
        activation_mode: ExecutionMode::Live,
        matched_rule_id: Some("family-a-live".to_owned()),
    };

    assert_eq!(request.route, "neg-risk");
    assert_eq!(request.scope, "family-a");
    assert_eq!(request.activation_mode, ExecutionMode::Live);
    assert_eq!(request.matched_rule_id.as_deref(), Some("family-a-live"));
}

#[test]
fn activation_decision_keeps_policy_anchors_for_replay() {
    let decision = ActivationDecision::shadow("family-a", "policy-v1", Some("rule-3"));
    assert_eq!(decision.policy_version, "policy-v1");
    assert_eq!(decision.matched_rule_id.as_deref(), Some("rule-3"));
}

#[test]
fn external_fact_event_carries_normalizer_anchor() {
    let event = ExternalFactEvent::new(
        "market_ws",
        "session-1",
        "evt-1",
        "v1-market-normalizer",
        Utc::now(),
    );
    assert_eq!(event.normalizer_version, "v1-market-normalizer");
}

#[test]
fn external_fact_event_can_carry_negrisk_live_submit_fact() {
    let observed_at = Utc.with_ymd_and_hms(2026, 3, 25, 12, 34, 56).unwrap();
    let fact = ExternalFactEvent::negrisk_live_submit_observed(
        "session-live",
        "evt-1",
        "attempt-family-a-1",
        "family-a",
        "submission-family-a-1",
        observed_at,
    );

    assert_eq!(fact.source_kind, "negrisk_live_submit");
    assert_eq!(fact.payload.kind(), "negrisk_live_submit_observed");
    assert_eq!(fact.observed_at, observed_at);
}

#[test]
fn external_fact_payload_exposes_live_submit_fields() {
    let observed_at = Utc.with_ymd_and_hms(2026, 3, 25, 12, 35, 0).unwrap();
    let fact = ExternalFactEvent::negrisk_live_submit_observed(
        "session-live",
        "evt-1",
        "attempt-family-a-1",
        "family-a",
        "submission-family-a-1",
        observed_at,
    );

    match fact.payload.as_ref() {
        Some(ExternalFactPayloadData::NegRiskLiveSubmitObserved(payload)) => {
            assert_eq!(payload.attempt_id, "attempt-family-a-1");
            assert_eq!(payload.scope, "family-a");
            assert_eq!(payload.submission_ref, "submission-family-a-1");
        }
        other => panic!("unexpected payload: {other:?}"),
    }
}

#[test]
fn external_fact_payload_exposes_live_reconcile_fields() {
    let observed_at = Utc.with_ymd_and_hms(2026, 3, 25, 12, 36, 0).unwrap();
    let fact = ExternalFactEvent::negrisk_live_reconcile_observed(
        "session-live",
        "evt-2",
        "pending-family-a-1",
        "family-a",
        true,
        observed_at,
    );

    match fact.payload.as_ref() {
        Some(ExternalFactPayloadData::NegRiskLiveReconcileObserved(payload)) => {
            assert_eq!(payload.pending_ref, "pending-family-a-1");
            assert_eq!(payload.scope, "family-a");
            assert!(payload.terminal);
        }
        other => panic!("unexpected payload: {other:?}"),
    }

    assert_eq!(fact.observed_at, observed_at);
}

#[test]
fn external_fact_event_defaults_to_no_payload_for_legacy_calls() {
    let fact = ExternalFactEvent::new(
        "market_ws",
        "session-legacy",
        "evt-legacy",
        "v1",
        Utc::now(),
    );

    assert_eq!(fact.payload.kind(), "none");
    assert!(fact.payload.as_ref().is_none());
    assert_eq!(fact.payload, ExternalFactPayload::default());
}
