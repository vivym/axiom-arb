use domain::{
    ActivationDecision, DecisionInput, DecisionVerdict, ExecutionMode, IntentCandidate,
    RecoveryIntent,
};

#[test]
fn recovery_only_rejects_strategy_inputs_but_allows_recovery_inputs() {
    let activation = ActivationDecision::new(
        ExecutionMode::RecoveryOnly,
        "family-a",
        "",
        "policy-v1",
        Some("rule-7"),
    );
    let strategy = sample_strategy_input("family-a");
    let recovery = sample_recovery_input("family-a");

    assert!(matches!(
        risk::evaluate_decision(&strategy, &activation),
        DecisionVerdict::Rejected
    ));
    assert!(matches!(
        risk::evaluate_decision(&recovery, &activation),
        DecisionVerdict::Approved
    ));
}

fn sample_strategy_input(scope: &str) -> DecisionInput {
    DecisionInput::Strategy(IntentCandidate::new("intent-1", "snapshot-1", scope))
}

fn sample_recovery_input(scope: &str) -> DecisionInput {
    DecisionInput::Recovery(RecoveryIntent::new("recovery-1", "snapshot-1", scope))
}
