use domain::{
    ActivationDecision, DecisionInput, DecisionVerdict, ExecutionMode, IntentCandidate,
    RecoveryIntent,
};
use state::{NegRiskFamilyRolloutReadiness, NegRiskView};

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

#[test]
fn reduce_only_rejects_strategy_inputs_but_allows_recovery_inputs() {
    let activation = ActivationDecision::new(
        ExecutionMode::ReduceOnly,
        "family-a",
        "",
        "policy-v1",
        Some("rule-8"),
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

#[test]
fn negrisk_entrypoint_rejects_live_mode_even_with_usable_projection() {
    let verdict =
        risk::negrisk::evaluate_negrisk_intent(&sample_negrisk_view(), ExecutionMode::Live);

    assert!(matches!(verdict, DecisionVerdict::Rejected));
}

#[test]
fn negrisk_entrypoint_rejects_unusable_projection() {
    let verdict = risk::negrisk::evaluate_negrisk_intent(
        &NegRiskView {
            snapshot_id: "snapshot-empty".to_owned(),
            state_version: 9,
            families: Vec::new(),
        },
        ExecutionMode::Shadow,
    );

    assert!(matches!(verdict, DecisionVerdict::Rejected));
}

#[test]
fn negrisk_entrypoint_approves_shadow_mode_when_projection_is_usable() {
    let verdict =
        risk::negrisk::evaluate_negrisk_intent(&sample_negrisk_view(), ExecutionMode::Shadow);

    assert!(matches!(verdict, DecisionVerdict::Approved));
}

#[test]
fn negrisk_entrypoint_rejects_reduce_only_even_with_usable_projection() {
    let verdict =
        risk::negrisk::evaluate_negrisk_intent(&sample_negrisk_view(), ExecutionMode::ReduceOnly);

    assert!(matches!(verdict, DecisionVerdict::Rejected));
}

#[test]
fn negrisk_entrypoint_rejects_recovery_only_even_with_usable_projection() {
    let verdict =
        risk::negrisk::evaluate_negrisk_intent(&sample_negrisk_view(), ExecutionMode::RecoveryOnly);

    assert!(matches!(verdict, DecisionVerdict::Rejected));
}

fn sample_strategy_input(scope: &str) -> DecisionInput {
    DecisionInput::Strategy(IntentCandidate::new("intent-1", "snapshot-1", scope))
}

fn sample_recovery_input(scope: &str) -> DecisionInput {
    DecisionInput::Recovery(RecoveryIntent::new("recovery-1", "snapshot-1", scope))
}

fn sample_negrisk_view() -> NegRiskView {
    NegRiskView {
        snapshot_id: "snapshot-negrisk-1".to_owned(),
        state_version: 8,
        families: vec![sample_family("family-a")],
    }
}

fn sample_family(family_id: &str) -> NegRiskFamilyRolloutReadiness {
    NegRiskFamilyRolloutReadiness {
        family_id: family_id.to_owned(),
        shadow_parity_ready: false,
        recovery_ready: false,
        replay_drift_ready: false,
        fault_injection_ready: false,
        conversion_path_ready: false,
        halt_semantics_ready: false,
    }
}
