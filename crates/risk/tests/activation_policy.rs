use domain::ExecutionMode;
use risk::{ActivationPolicy, RolloutRule};

#[test]
fn activation_policy_returns_live_for_fullset_and_shadow_for_negrisk() {
    let policy = ActivationPolicy::phase_one_defaults();

    assert_eq!(
        policy.mode_for_route("full-set", "market-a"),
        ExecutionMode::Live
    );
    assert_eq!(
        policy.mode_for_route("neg-risk", "family-a"),
        ExecutionMode::Shadow
    );
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
    let policy = ActivationPolicy::phase_one_defaults().with_overlay(
        "full-set",
        "family-a",
        ExecutionMode::Disabled,
    );

    assert_eq!(
        policy.mode_for_route("full-set", "family-a"),
        ExecutionMode::Disabled
    );
    assert_eq!(
        policy.mode_for_route("neg-risk", "family-a"),
        ExecutionMode::Shadow
    );
}

#[test]
fn neg_risk_live_overlay_is_explicitly_disabled_in_phase_one() {
    let policy = ActivationPolicy::phase_one_defaults().with_overlay(
        "neg-risk",
        "family-a",
        ExecutionMode::Live,
    );

    assert_eq!(
        policy.mode_for_route("neg-risk", "family-a"),
        ExecutionMode::Disabled
    );
}

#[test]
fn family_specific_live_rule_overrides_default_shadow_rule() {
    let policy = ActivationPolicy::from_rules(
        "phase-three-rules",
        vec![
            RolloutRule::new(
                "neg-risk",
                "default",
                ExecutionMode::Shadow,
                "default-shadow",
            ),
            RolloutRule::new("neg-risk", "family-a", ExecutionMode::Live, "family-a-live"),
        ],
    );

    let activation = policy.activation_for("neg-risk", "family-a", "snapshot-12");
    assert_eq!(activation.mode, ExecutionMode::Live);
    assert_eq!(activation.matched_rule_id.as_deref(), Some("family-a-live"));
}
