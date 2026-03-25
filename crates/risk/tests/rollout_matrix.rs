use domain::ExecutionMode;
use risk::{ActivationPolicy, RolloutRule};

#[test]
fn activation_for_falls_back_to_default_scope_and_keeps_replay_anchors() {
    let policy = ActivationPolicy::from_rules(
        "phase-three-rules",
        vec![RolloutRule::new(
            "neg-risk",
            "default",
            ExecutionMode::Shadow,
            "default-shadow",
        )],
    );

    let activation = policy.activation_for("neg-risk", "family-b", "snapshot-22");

    assert_eq!(activation.mode, ExecutionMode::Shadow);
    assert_eq!(activation.scope, "family-b");
    assert_eq!(activation.policy_version, "phase-three-rules");
    assert_eq!(
        activation.matched_rule_id.as_deref(),
        Some("default-shadow")
    );
    assert!(activation.reason.contains("snapshot-22"));
}
