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

#[test]
fn real_user_shadow_smoke_forces_shadow_across_risk_expanding_routes() {
    let policy = ActivationPolicy::from_rules(
        "phase-three-rules",
        vec![
            RolloutRule::new("full-set", "default", ExecutionMode::Live, "fullset-live"),
            RolloutRule::new("neg-risk", "family-a", ExecutionMode::Live, "family-a-live"),
        ],
    )
    .with_real_user_shadow_smoke();

    let fullset_activation = policy.activation_for("full-set", "market-a", "snapshot-12");
    assert_eq!(fullset_activation.mode, ExecutionMode::Shadow);
    assert_eq!(
        fullset_activation.matched_rule_id.as_deref(),
        Some("fullset-live")
    );

    let negrisk_activation = policy.activation_for("neg-risk", "family-a", "snapshot-12");
    assert_eq!(negrisk_activation.mode, ExecutionMode::Shadow);
    assert_eq!(
        negrisk_activation.matched_rule_id.as_deref(),
        Some("family-a-live")
    );
}

#[test]
fn smoke_mode_clamps_all_risk_expanding_routes_to_shadow() {
    let policy = ActivationPolicy::phase_one_defaults().with_real_user_shadow_smoke();

    assert_eq!(policy.mode_for_route("full-set", "default"), ExecutionMode::Shadow);
    assert_eq!(policy.mode_for_route("neg-risk", "family-a"), ExecutionMode::Shadow);
}

#[test]
fn real_user_shadow_smoke_preserves_negrisk_restrictive_modes() {
    let policy = ActivationPolicy::from_rules(
        "phase-three-rules",
        vec![
            RolloutRule::new(
                "neg-risk",
                "family-a",
                ExecutionMode::ReduceOnly,
                "family-a-reduce",
            ),
            RolloutRule::new(
                "neg-risk",
                "family-b",
                ExecutionMode::RecoveryOnly,
                "family-b-recovery",
            ),
            RolloutRule::new("neg-risk", "family-c", ExecutionMode::Live, "family-c-live"),
        ],
    )
    .with_real_user_shadow_smoke();

    assert_eq!(
        policy.mode_for_route("neg-risk", "family-a"),
        ExecutionMode::ReduceOnly
    );
    assert_eq!(
        policy.mode_for_route("neg-risk", "family-b"),
        ExecutionMode::RecoveryOnly
    );
    assert_eq!(
        policy.mode_for_route("neg-risk", "family-c"),
        ExecutionMode::Shadow
    );
}
