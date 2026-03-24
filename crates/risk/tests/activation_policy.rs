use domain::ExecutionMode;
use risk::ActivationPolicy;

#[test]
fn activation_policy_returns_live_for_fullset_and_shadow_for_negrisk() {
    let policy = ActivationPolicy::phase_one_defaults();

    assert_eq!(policy.mode_for_route("full-set", "market-a"), ExecutionMode::Live);
    assert_eq!(policy.mode_for_route("neg-risk", "family-a"), ExecutionMode::Shadow);
}

#[test]
fn recovery_overlay_takes_precedence_over_rollout_mode() {
    let policy = ActivationPolicy::phase_one_defaults().with_overlay(
        "neg-risk",
        "family-a",
        ExecutionMode::RecoveryOnly,
    );

    assert_eq!(
        policy.mode_for_route("neg-risk", "family-a"),
        ExecutionMode::RecoveryOnly
    );
}

#[test]
fn overlays_are_route_local_to_the_matching_route() {
    let policy =
        ActivationPolicy::phase_one_defaults().with_overlay("full-set", "family-a", ExecutionMode::Disabled);

    assert_eq!(policy.mode_for_route("full-set", "family-a"), ExecutionMode::Disabled);
    assert_eq!(policy.mode_for_route("neg-risk", "family-a"), ExecutionMode::Shadow);
}

#[test]
fn neg_risk_live_overlay_is_explicitly_disabled_in_phase_one() {
    let policy =
        ActivationPolicy::phase_one_defaults().with_overlay("neg-risk", "family-a", ExecutionMode::Live);

    assert_eq!(policy.mode_for_route("neg-risk", "family-a"), ExecutionMode::Disabled);
}
