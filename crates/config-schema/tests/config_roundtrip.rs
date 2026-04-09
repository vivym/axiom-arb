use config_schema::{load_raw_config_from_str, render_raw_config_to_string};

#[test]
fn raw_config_round_trips_target_source_operator_target_revision() {
    let raw = load_raw_config_from_str(
        r#"
[runtime]
mode = "live"

[negrisk.target_source]
source = "adopted"
operator_target_revision = "targets-rev-9"
"#,
    )
    .unwrap();

    let text = render_raw_config_to_string(&raw).unwrap();
    assert!(text.contains("operator_target_revision = \"targets-rev-9\""));
}

#[test]
fn raw_config_round_trips_strategy_control_and_route_sections() {
    let raw = load_raw_config_from_str(
        r#"
[runtime]
mode = "live"

[strategy_control]
source = "adopted"
operator_strategy_revision = "strategy-rev-12"

[strategies.full_set]
enabled = true

[strategies.neg_risk]
enabled = true

[strategies.neg_risk.rollout]
approved_scopes = ["family-a", "family-b"]
ready_scopes = ["family-a"]
"#,
    )
    .unwrap();

    let text = render_raw_config_to_string(&raw).unwrap();
    assert!(text.contains("[strategy_control]"));
    assert!(text.contains("operator_strategy_revision = \"strategy-rev-12\""));
    assert!(text.contains("approved_scopes = ["));
    assert!(text.contains("\"family-a\""));
    assert!(text.contains("\"family-b\""));
    assert!(text.contains("ready_scopes = ["));
}

#[test]
fn raw_config_round_trips_safe_empty_rollout_and_adopted_target_source() {
    let raw = load_raw_config_from_str(
        r#"
[runtime]
mode = "live"

[negrisk.target_source]
source = "adopted"

[negrisk.rollout]
approved_families = []
ready_families = []
"#,
    )
    .unwrap();

    let text = render_raw_config_to_string(&raw).unwrap();
    assert!(text.contains("source = \"adopted\""));
    assert!(text.contains("approved_families = []"));
    assert!(text.contains("ready_families = []"));
}

#[test]
fn raw_config_round_trips_preserved_operator_target_revision_and_rollout() {
    let raw = load_raw_config_from_str(
        r#"
[runtime]
mode = "live"

[polymarket.account]
address = "0x1111111111111111111111111111111111111111"
signature_type = "eoa"
wallet_route = "eoa"
api_key = "poly-api-key"
secret = "poly-secret"
passphrase = "poly-passphrase"

[negrisk.target_source]
source = "adopted"
operator_target_revision = "targets-rev-9"

[negrisk.rollout]
approved_families = ["family-a"]
ready_families = ["family-b"]
"#,
    )
    .unwrap();

    let text = render_raw_config_to_string(&raw).unwrap();
    assert!(text.contains("operator_target_revision = \"targets-rev-9\""));
    assert!(text.contains("approved_families = [\"family-a\"]"));
    assert!(text.contains("ready_families = [\"family-b\"]"));
}

#[test]
fn raw_config_round_trips_present_empty_targets_array_for_legacy_detection() {
    let raw = load_raw_config_from_str(
        r#"
[runtime]
mode = "live"

[negrisk]
targets = []
"#,
    )
    .unwrap();

    let text = render_raw_config_to_string(&raw).unwrap();
    assert!(text.contains("targets = []"));
    assert!(raw
        .negrisk
        .as_ref()
        .expect("negrisk should be present")
        .targets
        .is_present());
    assert!(raw
        .negrisk
        .as_ref()
        .expect("negrisk should be present")
        .targets
        .is_empty());
}
