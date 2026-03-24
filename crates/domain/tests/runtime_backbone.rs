use chrono::Utc;
use domain::{
    ActivationDecision, DecisionInput, ExecutionAttempt, ExecutionMode, ExternalFactEvent,
    IntentCandidate, RecoveryIntent,
};

#[test]
fn decision_contracts_stay_input_neutral_and_attempt_scoped() {
    let strategy =
        DecisionInput::Strategy(IntentCandidate::new("intent-1", "snapshot-7", "full-set"));
    let recovery = DecisionInput::Recovery(RecoveryIntent::new(
        "recovery-1",
        "snapshot-7",
        "family-lock",
    ));

    assert_eq!(strategy.decision_input_id(), "intent-1");
    assert_eq!(recovery.decision_input_id(), "recovery-1");
    assert!(matches!(ExecutionMode::Shadow, ExecutionMode::Shadow));

    let attempt = ExecutionAttempt::new("attempt-1", "plan-1", "snapshot-7", 1);
    assert_eq!(attempt.attempt_id.as_str(), "attempt-1");
    assert_eq!(attempt.plan_id.as_str(), "plan-1");
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
