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
    let policy =
        ActivationPolicy::phase_one_defaults().with_overlay("family-a", ExecutionMode::RecoveryOnly);

    assert_eq!(
        policy.mode_for_route("neg-risk", "family-a"),
        ExecutionMode::RecoveryOnly
    );
}
