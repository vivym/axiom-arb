use config_schema::{load_raw_config_from_path, ValidatedConfig};

#[path = "../src/commands/targets/config_file.rs"]
mod config_file;

use config_file::{
    rewrite_operator_strategy_revision, rewrite_operator_target_revision,
    rewrite_smoke_rollout_families,
};

const MINIMAL_TARGET_SOURCE_CONFIG: &str = r#"
# preserve this comment
[runtime]
mode = "live"

[negrisk.target_source]
source = "adopted"
operator_target_revision = ""
legacy_note = "keep-me"
"#;

const LEGACY_EXPLICIT_CONFIG: &str = r#"
# preserve this comment
[runtime]
mode = "live"

[polymarket.source]
clob_host = "https://clob.polymarket.com"
data_api_host = "https://gamma-api.polymarket.com"
relayer_host = "https://relayer-v2.polymarket.com"
market_ws_url = "wss://ws-subscriptions-clob.polymarket.com/ws/market"
user_ws_url = "wss://ws-subscriptions-clob.polymarket.com/ws/user"
heartbeat_interval_seconds = 15
relayer_poll_interval_seconds = 5
metadata_refresh_interval_seconds = 60

[polymarket.signer]
address = "0x1111111111111111111111111111111111111111"
funder_address = "0x2222222222222222222222222222222222222222"
signature_type = "eoa"
wallet_route = "eoa"
api_key = "poly-api-key-1"
passphrase = "poly-passphrase-1"
timestamp = "1700000000"
signature = "poly-signature-1"

[polymarket.relayer_auth]
kind = "builder_api_key"
api_key = "builder-api-key-1"
timestamp = "1700000001"
passphrase = "builder-passphrase-1"
signature = "builder-signature-1"

[negrisk.rollout]
approved_families = ["family-a"]
ready_families = ["family-a"]
legacy_note = "keep-me"

[[negrisk.targets]]
family_id = "family-a"

[[negrisk.targets.members]]
condition_id = "condition-1"
token_id = "token-1"
price = "0.43"
quantity = "5"
"#;

const STRATEGY_CONTROL_ROLLOUT_CONFIG: &str = r#"
# preserve this comment
[runtime]
mode = "live"

[strategy_control]
source = "adopted"
operator_strategy_revision = "strategy-rev-12"

[strategies.neg_risk.rollout]
approved_scopes = []
ready_scopes = []
legacy_note = "keep-me"
"#;

#[test]
fn rewrite_operator_target_revision_updates_the_same_config_file_for_minimal_target_source_config()
{
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("axiom-arb.local.toml");
    std::fs::write(&path, MINIMAL_TARGET_SOURCE_CONFIG).unwrap();

    rewrite_operator_target_revision(&path, "targets-rev-12").unwrap();

    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("# preserve this comment"));
    assert!(text.contains("operator_target_revision = \"targets-rev-12\""));
    assert!(text.contains("legacy_note = \"keep-me\""));

    let raw = load_raw_config_from_path(&path).unwrap();
    let validated = ValidatedConfig::new(raw).unwrap();
    let target_source = validated.target_source().unwrap();
    assert!(target_source.is_adopted());
    assert_eq!(
        target_source.operator_target_revision(),
        Some("targets-rev-12")
    );
}

#[test]
fn rewrite_operator_target_revision_fails_when_target_source_is_missing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("axiom-arb.local.toml");
    std::fs::write(
        &path,
        r#"
[runtime]
mode = "live"
"#,
    )
    .unwrap();

    let error = rewrite_operator_target_revision(&path, "targets-rev-12").unwrap_err();
    assert!(error
        .to_string()
        .contains("missing required section: negrisk.target_source"));
}

#[test]
fn rewrite_operator_strategy_revision_migrates_legacy_explicit_config_to_strategy_control() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("axiom-arb.local.toml");
    std::fs::write(&path, LEGACY_EXPLICIT_CONFIG).unwrap();

    rewrite_operator_strategy_revision(&path, "strategy-rev-12").unwrap();

    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("# preserve this comment"));
    assert!(text.contains("[strategy_control]"));
    assert!(text.contains("source = \"adopted\""));
    assert!(text.contains("operator_strategy_revision = \"strategy-rev-12\""));
    assert!(!text.contains("operator_target_revision ="));
    assert!(!text.contains("[[negrisk.targets]]"));
    assert!(text.contains("[strategies.neg_risk.rollout]"));
    assert!(text.contains("approved_scopes = [\"family-a\"]"));
    assert!(text.contains("ready_scopes = [\"family-a\"]"));
    assert!(!text.contains("approved_families ="));
    assert!(!text.contains("ready_families ="));
    assert!(text.contains("legacy_note = \"keep-me\""));

    let raw = load_raw_config_from_path(&path).unwrap();
    let rollout = raw
        .strategies
        .as_ref()
        .and_then(|strategies| strategies.neg_risk.as_ref())
        .and_then(|neg_risk| neg_risk.rollout.as_ref())
        .expect("route-owned rollout should exist after rewrite");
    assert_eq!(rollout.approved_scopes, vec!["family-a".to_owned()]);
    assert_eq!(rollout.ready_scopes, vec!["family-a".to_owned()]);

    let validated = ValidatedConfig::new(raw).unwrap();
    let live = validated.for_app_live().unwrap();
    assert_eq!(live.operator_strategy_revision(), Some("strategy-rev-12"));
}

#[test]
fn rewrite_smoke_rollout_families_updates_both_lists_and_preserves_comments() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("axiom-arb.local.toml");
    std::fs::write(
        &path,
        r#"
# preserve this comment
[runtime]
mode = "live"

[negrisk.target_source]
source = "adopted"
operator_target_revision = "targets-rev-12"

[negrisk.rollout]
approved_families = []
ready_families = []
legacy_note = "keep-me"
"#,
    )
    .unwrap();

    rewrite_smoke_rollout_families(&path, &["family-b".into(), "family-a".into()]).unwrap();

    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("# preserve this comment"));
    assert!(text.contains("approved_families = [\"family-a\", \"family-b\"]"));
    assert!(text.contains("ready_families = [\"family-a\", \"family-b\"]"));
    assert!(text.contains("legacy_note = \"keep-me\""));

    let raw = load_raw_config_from_path(&path).unwrap();
    let validated = ValidatedConfig::new(raw.clone()).unwrap();
    let rollout = raw.negrisk.unwrap().rollout.unwrap();
    let _ = validated;
    assert_eq!(
        rollout.approved_families,
        vec!["family-a".to_owned(), "family-b".to_owned()]
    );
    assert_eq!(
        rollout.ready_families,
        vec!["family-a".to_owned(), "family-b".to_owned()]
    );
}

#[test]
fn rewrite_smoke_rollout_families_supports_route_owned_rollout_after_strategy_migration() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("axiom-arb.local.toml");
    std::fs::write(&path, STRATEGY_CONTROL_ROLLOUT_CONFIG).unwrap();

    rewrite_smoke_rollout_families(&path, &["family-b".into(), "family-a".into()]).unwrap();

    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("# preserve this comment"));
    assert!(text.contains("approved_scopes = [\"family-a\", \"family-b\"]"));
    assert!(text.contains("ready_scopes = [\"family-a\", \"family-b\"]"));
    assert!(text.contains("legacy_note = \"keep-me\""));

    let raw = load_raw_config_from_path(&path).unwrap();
    let validated = ValidatedConfig::new(raw.clone()).unwrap();
    let _ = validated;
    let rollout = raw.strategies.unwrap().neg_risk.unwrap().rollout.unwrap();
    assert_eq!(
        rollout.approved_scopes,
        vec!["family-a".to_owned(), "family-b".to_owned()]
    );
    assert_eq!(
        rollout.ready_scopes,
        vec!["family-a".to_owned(), "family-b".to_owned()]
    );
}
