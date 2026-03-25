use domain::{DecisionVerdict, ExecutionMode};
use state::{NegRiskFamilyRolloutReadiness, NegRiskView};

#[test]
fn live_mode_requires_all_family_readiness_gates() {
    let view = sample_negrisk_view_with_family(
        "family-a",
        true,
        true,
        false,
        true,
        true,
        true,
    );

    assert_eq!(
        risk::negrisk::evaluate_negrisk_family(&view, "family-a", ExecutionMode::Live),
        DecisionVerdict::Rejected
    );
}

#[test]
fn shadow_mode_allows_any_published_family_record() {
    let view = sample_negrisk_view_with_family(
        "family-a",
        false,
        false,
        false,
        false,
        false,
        false,
    );

    assert_eq!(
        risk::negrisk::evaluate_negrisk_family(&view, "family-a", ExecutionMode::Shadow),
        DecisionVerdict::Approved
    );
}

#[test]
fn live_mode_approves_when_all_family_readiness_gates_are_true() {
    let view = sample_negrisk_view_with_family(
        "family-a", true, true, true, true, true, true,
    );

    assert_eq!(
        risk::negrisk::evaluate_negrisk_family(&view, "family-a", ExecutionMode::Live),
        DecisionVerdict::Approved
    );
}

fn sample_negrisk_view_with_family(
    family_id: &str,
    shadow_parity_ready: bool,
    recovery_ready: bool,
    replay_drift_ready: bool,
    fault_injection_ready: bool,
    conversion_path_ready: bool,
    halt_semantics_ready: bool,
) -> NegRiskView {
    NegRiskView {
        snapshot_id: "snapshot-negrisk-rollout".to_owned(),
        state_version: 12,
        families: vec![NegRiskFamilyRolloutReadiness {
            family_id: family_id.to_owned(),
            shadow_parity_ready,
            recovery_ready,
            replay_drift_ready,
            fault_injection_ready,
            conversion_path_ready,
            halt_semantics_ready,
        }],
    }
}
