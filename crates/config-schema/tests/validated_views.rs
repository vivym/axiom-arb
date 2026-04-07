use std::path::{Path, PathBuf};

use config_schema::{
    load_raw_config_from_path, load_raw_config_from_str, RuntimeModeToml, ValidatedConfig,
};

#[test]
fn live_view_accepts_account_and_target_source_without_raw_targets() {
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

[polymarket.relayer_auth]
kind = "relayer_api_key"
api_key = "relay-key"
address = "0x1111111111111111111111111111111111111111"

[negrisk.target_source]
source = "adopted"
operator_target_revision = "targets-rev-9"
"#,
    )
    .unwrap();

    let validated = ValidatedConfig::new(raw).unwrap();
    let live = validated.for_app_live().unwrap();
    assert_eq!(
        live.target_source()
            .expect("live fixture should include target source")
            .operator_target_revision(),
        Some("targets-rev-9")
    );
}

#[test]
fn validated_config_accepts_strategy_control_and_route_sections() {
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
approved_scopes = ["family-a"]
ready_scopes = ["family-a"]

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
approved_families = []
ready_families = []
"#,
    )
    .unwrap();

    let validated = ValidatedConfig::new(raw).unwrap();
    let live = validated.for_app_live().unwrap();

    assert_eq!(live.operator_strategy_revision(), Some("strategy-rev-12"));
    assert!(!live.is_legacy_explicit_strategy_config());
}

#[test]
fn validated_config_accepts_pure_neutral_strategy_control_without_legacy_target_source() {
    let raw = load_raw_config_from_str(
        r#"
[runtime]
mode = "live"
real_user_shadow_smoke = true

[strategy_control]
source = "adopted"
operator_strategy_revision = "strategy-rev-12"

[strategies.full_set]
enabled = true

[strategies.neg_risk]
enabled = true

[strategies.neg_risk.rollout]
approved_scopes = []
ready_scopes = []

[polymarket.account]
address = "0x1111111111111111111111111111111111111111"
signature_type = "eoa"
wallet_route = "eoa"
api_key = "poly-api-key"
secret = "poly-secret"
passphrase = "poly-passphrase"

[polymarket.relayer_auth]
kind = "relayer_api_key"
api_key = "relay-key"
address = "0x1111111111111111111111111111111111111111"
"#,
    )
    .unwrap();

    let validated = ValidatedConfig::new(raw).unwrap();
    let live = validated.for_app_live().unwrap();

    assert!(live.has_adopted_strategy_source());
    assert_eq!(live.operator_strategy_revision(), Some("strategy-rev-12"));
    assert!(!live.has_target_source());
    assert!(!live.is_legacy_explicit_strategy_config());
}

#[test]
fn validated_config_accepts_pure_neutral_neg_risk_rollout_without_legacy_rollout() {
    let raw = load_raw_config_from_str(
        r#"
[runtime]
mode = "live"
real_user_shadow_smoke = false

[strategy_control]
source = "adopted"
operator_strategy_revision = "strategy-rev-12"

[strategies.full_set]
enabled = true

[strategies.neg_risk]
enabled = true

[strategies.neg_risk.rollout]
approved_scopes = ["family-a"]
ready_scopes = ["family-a"]

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
"#,
    )
    .unwrap();

    let validated = ValidatedConfig::new(raw).unwrap();
    let live = validated.for_app_live().unwrap();

    let rollout = live
        .negrisk_rollout()
        .expect("route-owned rollout should bridge into negrisk rollout view");
    assert_eq!(rollout.approved_families(), &["family-a".to_owned()]);
    assert_eq!(rollout.ready_families(), &["family-a".to_owned()]);
    assert_eq!(live.operator_strategy_revision(), Some("strategy-rev-12"));
}

#[test]
fn route_owned_neg_risk_rollout_overrides_legacy_rollout_when_both_are_present() {
    let raw = load_raw_config_from_str(
        r#"
[runtime]
mode = "live"
real_user_shadow_smoke = false

[strategy_control]
source = "adopted"
operator_strategy_revision = "strategy-rev-12"

[strategies.neg_risk]
enabled = true

[strategies.neg_risk.rollout]
approved_scopes = ["family-route-owned"]
ready_scopes = ["family-route-owned"]

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
approved_families = []
ready_families = []
"#,
    )
    .unwrap();

    let validated = ValidatedConfig::new(raw).unwrap();
    let live = validated.for_app_live().unwrap();

    let rollout = live
        .negrisk_rollout()
        .expect("route-owned rollout should take precedence over legacy rollout");
    assert_eq!(
        rollout.approved_families(),
        &["family-route-owned".to_owned()]
    );
    assert_eq!(rollout.ready_families(), &["family-route-owned".to_owned()]);
}

#[test]
fn signer_view_ignores_stale_legacy_rollout_when_route_owned_rollout_is_present() {
    let raw = load_raw_config_from_str(
        r#"
[runtime]
mode = "live"

[strategies.neg_risk]
enabled = true

[strategies.neg_risk.rollout]
approved_scopes = ["family-route-owned"]
ready_scopes = ["family-route-owned"]

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
approved_families = ["family-stale"]
ready_families = ["family-stale"]
"#,
    )
    .unwrap();

    let validated = ValidatedConfig::new(raw).unwrap();
    let live = validated
        .for_app_live()
        .expect("route-owned rollout should override stale legacy rollout during requiredness");

    let rollout = live
        .negrisk_rollout()
        .expect("route-owned rollout should remain visible through the validated view");
    assert_eq!(
        rollout.approved_families(),
        &["family-route-owned".to_owned()]
    );
    assert_eq!(rollout.ready_families(), &["family-route-owned".to_owned()]);
}

#[test]
fn legacy_explicit_strategy_config_detection_distinguishes_missing_targets_from_explicit_targets() {
    let missing_targets = load_raw_config_from_str(
        r#"
[runtime]
mode = "live"
real_user_shadow_smoke = true

[strategy_control]
source = "adopted"
operator_strategy_revision = "strategy-rev-12"

[polymarket.account]
address = "0x1111111111111111111111111111111111111111"
signature_type = "eoa"
wallet_route = "eoa"
api_key = "poly-api-key"
secret = "poly-secret"
passphrase = "poly-passphrase"

[polymarket.relayer_auth]
kind = "relayer_api_key"
api_key = "relay-key"
address = "0x1111111111111111111111111111111111111111"
"#,
    )
    .unwrap();
    let explicit_targets = load_raw_config_from_str(
        r#"
[runtime]
mode = "live"
real_user_shadow_smoke = true

[strategy_control]
source = "adopted"
operator_strategy_revision = "strategy-rev-12"

[polymarket.account]
address = "0x1111111111111111111111111111111111111111"
signature_type = "eoa"
wallet_route = "eoa"
api_key = "poly-api-key"
secret = "poly-secret"
passphrase = "poly-passphrase"

[polymarket.relayer_auth]
kind = "relayer_api_key"
api_key = "relay-key"
address = "0x1111111111111111111111111111111111111111"

[negrisk]
targets = []
"#,
    )
    .unwrap();

    let missing_targets_validated = ValidatedConfig::new(missing_targets).unwrap();
    let missing_targets_live = missing_targets_validated.for_app_live().unwrap();
    let explicit_targets_validated = ValidatedConfig::new(explicit_targets).unwrap();
    let explicit_targets_live = explicit_targets_validated.for_app_live().unwrap();

    assert!(!missing_targets_live.is_legacy_explicit_strategy_config());
    assert!(explicit_targets_live.is_legacy_explicit_strategy_config());
}

#[test]
fn validated_config_marks_legacy_explicit_targets_as_compatibility_mode() {
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

[polymarket.relayer_auth]
kind = "relayer_api_key"
api_key = "relay-key"
address = "0x1111111111111111111111111111111111111111"

[negrisk.target_source]
source = "adopted"
operator_target_revision = "targets-rev-9"

[negrisk.rollout]
approved_families = ["family-a"]
ready_families = ["family-a"]

[[negrisk.targets]]
family_id = "family-a"

[[negrisk.targets.members]]
condition_id = "condition-1"
token_id = "token-1"
price = "0.43"
quantity = "5"
"#,
    )
    .unwrap();

    let validated = ValidatedConfig::new(raw).unwrap();
    let live = validated.for_app_live().unwrap();

    assert!(live.is_legacy_explicit_strategy_config());
    assert_eq!(live.operator_strategy_revision(), None);
}

#[test]
fn paper_view_does_not_require_live_sections() {
    let raw = load_raw_config_from_str("[runtime]\nmode = \"paper\"\n").unwrap();
    let validated = ValidatedConfig::new(raw).unwrap();
    let live = validated.for_app_live().unwrap();
    assert!(live.is_paper());
}

#[test]
fn replay_view_accepts_new_operator_facing_schema_without_live_only_sections() {
    let raw = load_raw_config_from_path(&fixture_path("app-replay-ux.toml")).unwrap();
    assert!(raw.polymarket.is_none());
    assert!(raw.negrisk.is_none());

    let validated = ValidatedConfig::new(raw).unwrap();
    let replay = validated.for_app_replay().unwrap();
    assert_eq!(replay.mode(), RuntimeModeToml::Live);
    assert!(!replay.real_user_shadow_smoke());
}

#[test]
fn replay_view_does_not_require_live_signer_or_source() {
    let raw = load_raw_config_from_path(&fixture_path("app-replay.toml")).unwrap();
    let validated = ValidatedConfig::new(raw).unwrap();

    let replay = validated
        .for_app_replay()
        .expect("replay view should validate");

    assert_eq!(replay.mode(), RuntimeModeToml::Live);
    assert!(!replay.real_user_shadow_smoke());
}

#[test]
fn replay_view_accepts_malformed_live_only_sections() {
    let raw = load_raw_config_from_path(&fixture_path("app-replay-malformed-live.toml")).unwrap();
    let validated = ValidatedConfig::new(raw).unwrap();

    let replay = validated
        .for_app_replay()
        .expect("replay view should ignore malformed live-only sections");

    assert_eq!(replay.mode(), RuntimeModeToml::Live);
}

#[test]
fn replay_view_accepts_invalid_rollout_references() {
    let raw = load_raw_config_from_str(
        r#"
[runtime]
mode = "live"
real_user_shadow_smoke = false

[negrisk.rollout]
approved_families = ["family-a", "family-missing"]
ready_families = ["family-a"]

[[negrisk.targets]]
family_id = "family-a"

[[negrisk.targets.members]]
condition_id = "condition-1"
token_id = "token-1"
price = "0.43"
quantity = "5"
"#,
    )
    .unwrap();

    let validated = ValidatedConfig::new(raw).unwrap();
    let replay = validated
        .for_app_replay()
        .expect("replay view should ignore invalid rollout references");

    assert_eq!(replay.mode(), RuntimeModeToml::Live);
}

#[test]
fn live_view_rejects_invalid_rollout_references() {
    let raw = load_raw_config_from_str(
        r#"
[runtime]
mode = "live"
real_user_shadow_smoke = false

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
approved_families = ["family-a", "family-missing"]
ready_families = ["family-a"]

[[negrisk.targets]]
family_id = "family-a"

[[negrisk.targets.members]]
condition_id = "condition-1"
token_id = "token-1"
price = "0.43"
quantity = "5"
"#,
    )
    .unwrap();

    let validated = ValidatedConfig::new(raw).expect("global validation should allow rollout refs");
    let err = validated
        .for_app_live()
        .expect_err("live view should reject invalid rollout references");

    assert!(err
        .to_string()
        .contains("approved_families references missing family_id"));
}

#[test]
fn smoke_view_requires_live_signer() {
    let raw = load_raw_config_from_path(&fixture_path("app-live-smoke.toml")).unwrap();
    let err = ValidatedConfig::new(raw)
        .unwrap()
        .for_app_live()
        .expect_err("smoke fixture missing signer should fail");

    assert!(err.to_string().contains("polymarket.signer"));
}

#[test]
fn operator_facing_smoke_view_defaults_source_when_omitted() {
    let raw = load_raw_config_from_str(
        r#"
[runtime]
mode = "live"
real_user_shadow_smoke = true

[polymarket.account]
address = "0x1111111111111111111111111111111111111111"
signature_type = "eoa"
wallet_route = "eoa"
api_key = "poly-api-key"
secret = "poly-secret"
passphrase = "poly-passphrase"

[polymarket.relayer_auth]
kind = "relayer_api_key"
api_key = "relay-key"
address = "0x1111111111111111111111111111111111111111"

[negrisk.target_source]
source = "adopted"
operator_target_revision = "targets-rev-9"
"#,
    )
    .unwrap();

    let validated = ValidatedConfig::new(raw).unwrap();
    let live = validated.for_app_live().unwrap();

    assert!(!live.has_polymarket_source());

    let source = live
        .effective_polymarket_source()
        .expect("operator smoke config should use built-in source defaults");
    assert_eq!(source.clob_host(), "https://clob.polymarket.com");
    assert_eq!(source.data_api_host(), "https://gamma-api.polymarket.com");
    assert_eq!(source.metadata_refresh_interval_seconds(), 60);
}

#[test]
fn operator_facing_source_overrides_win_over_source() {
    let raw = load_raw_config_from_str(
        r#"
[runtime]
mode = "live"
real_user_shadow_smoke = true

[polymarket.account]
address = "0x1111111111111111111111111111111111111111"
signature_type = "eoa"
wallet_route = "eoa"
api_key = "poly-api-key"
secret = "poly-secret"
passphrase = "poly-passphrase"

[polymarket.relayer_auth]
kind = "relayer_api_key"
api_key = "relay-key"
address = "0x1111111111111111111111111111111111111111"

[polymarket.source]
clob_host = "https://clob.example.invalid"
data_api_host = "https://data-api.example.invalid"
relayer_host = "https://relayer.example.invalid"
market_ws_url = "wss://market.example.invalid/ws"
user_ws_url = "wss://user.example.invalid/ws"
heartbeat_interval_seconds = 11
relayer_poll_interval_seconds = 22
metadata_refresh_interval_seconds = 33

[polymarket.source_overrides]
clob_host = "https://clob.override.invalid"
data_api_host = "https://data-api.override.invalid"
relayer_host = "https://relayer.override.invalid"
market_ws_url = "wss://market.override.invalid/ws"
user_ws_url = "wss://user.override.invalid/ws"
heartbeat_interval_seconds = 44
relayer_poll_interval_seconds = 55
metadata_refresh_interval_seconds = 66

[negrisk.target_source]
source = "adopted"
operator_target_revision = "targets-rev-9"
"#,
    )
    .unwrap();

    let validated = ValidatedConfig::new(raw).unwrap();
    let live = validated.for_app_live().unwrap();

    assert!(live.has_polymarket_source());
    assert_eq!(
        live.polymarket_source()
            .expect("raw source should still resolve to explicit override")
            .clob_host(),
        "https://clob.override.invalid"
    );
    assert_eq!(
        live.effective_polymarket_source()
            .expect("effective source should resolve to explicit override")
            .clob_host(),
        "https://clob.override.invalid"
    );
}

#[test]
fn signer_based_legacy_live_view_still_requires_explicit_source() {
    let err = validated_view_err(
        r#"
[runtime]
mode = "live"
real_user_shadow_smoke = false

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
approved_families = []
ready_families = []
"#,
    );

    assert!(err.contains("polymarket.source"));
}

#[test]
fn invalid_toml_and_invalid_fields_fail_closed() {
    assert!(load_raw_config_from_str("runtime = [").is_err());
    assert!(config_err(
        "[runtime]
mode = \"invalid\"
"
    )
    .contains("runtime.mode"));
}

#[test]
fn paper_mode_rejects_real_user_shadow_smoke() {
    let err = validated_err(
        r#"
[runtime]
mode = "paper"
real_user_shadow_smoke = true
"#,
    );

    assert!(err.contains("real_user_shadow_smoke"));
}

#[test]
fn live_view_accepts_operator_facing_live_fixture() {
    let raw = load_raw_config_from_path(&fixture_path("app-live-ux-live.toml")).unwrap();
    let validated = ValidatedConfig::new(raw).unwrap();
    let live = validated
        .for_app_live()
        .expect("operator live fixture should validate");

    assert!(live.has_polymarket_account());
    assert!(live.has_target_source());
    assert_eq!(
        live.target_source()
            .expect("operator live fixture should include target source")
            .operator_target_revision(),
        Some("targets-rev-9")
    );
    assert!(!live.has_polymarket_source());
}

#[test]
fn smoke_view_accepts_operator_facing_live_fixture() {
    let raw = load_raw_config_from_path(&fixture_path("app-live-ux-smoke.toml")).unwrap();
    let validated = ValidatedConfig::new(raw).unwrap();
    let live = validated
        .for_app_live()
        .expect("operator smoke fixture should validate");

    assert!(live.real_user_shadow_smoke());
    assert!(live.has_polymarket_account());
    assert!(live.has_polymarket_source());
    assert!(!live.has_target_source());
    assert_eq!(live.operator_strategy_revision(), Some("targets-rev-9"));
    assert_eq!(
        live.polymarket_source()
            .expect("operator smoke fixture should include source settings")
            .clob_host(),
        "https://clob.polymarket.com"
    );
}

#[test]
fn live_view_accepts_fully_populated_live_fixture() {
    let raw = load_raw_config_from_path(&fixture_path("app-live-live.toml")).unwrap();
    let validated = ValidatedConfig::new(raw).unwrap();

    let live = validated.for_app_live().expect("live view should validate");
    assert_eq!(live.mode(), RuntimeModeToml::Live);
    assert!(!live.real_user_shadow_smoke());
    assert!(live.has_polymarket_account());
    assert!(live.has_polymarket_source());
    assert!(!live.has_polymarket_signer());
    assert!(!live.has_target_source());
    assert_eq!(live.operator_strategy_revision(), Some("strategy-rev-12"));
}

#[test]
fn live_view_exposes_consumer_scoped_wrappers() {
    let raw = load_raw_config_from_path(&fixture_path("app-live-live.toml")).unwrap();
    let validated = ValidatedConfig::new(raw).unwrap();
    let live = validated.for_app_live().expect("live view should validate");

    let source = live
        .polymarket_source()
        .expect("live fixture should include source");
    assert_eq!(source.clob_host(), "https://clob.polymarket.com");
    assert_eq!(source.heartbeat_interval_seconds(), 15);

    let account = live
        .account()
        .expect("live fixture should include account credentials");
    assert_eq!(account.signature_type_label(), "Eoa");
    assert_eq!(account.wallet_route_label(), "Eoa");

    let relayer_auth = live
        .polymarket_relayer_auth()
        .expect("live fixture should include relayer auth");
    assert_eq!(
        relayer_auth.kind(),
        config_schema::AppLivePolymarketRelayerAuthKind::RelayerApiKey
    );
    assert_eq!(relayer_auth.api_key(), "relay-key");

    assert!(live.negrisk_targets().iter().next().is_none());
    assert_eq!(live.operator_strategy_revision(), Some("strategy-rev-12"));
}

fn validated_err(extra: &str) -> String {
    let config = r#"
[runtime]
mode = "live"
real_user_shadow_smoke = false

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

[[negrisk.targets]]
family_id = "family-a"

[[negrisk.targets.members]]
condition_id = "condition-1"
token_id = "token-1"
price = "0.43"
quantity = "5"
"#
    .to_string();

    let config = if extra.contains("[runtime]") {
        extra.to_owned()
    } else {
        config + extra
    };

    let raw = load_raw_config_from_str(&config).unwrap();
    ValidatedConfig::new(raw).unwrap_err().to_string()
}

fn validated_view_err(config: &str) -> String {
    let raw = load_raw_config_from_str(config).unwrap();
    ValidatedConfig::new(raw)
        .unwrap()
        .for_app_live()
        .unwrap_err()
        .to_string()
}

fn config_err(config: &str) -> String {
    match load_raw_config_from_str(config) {
        Ok(raw) => ValidatedConfig::new(raw).unwrap_err().to_string(),
        Err(err) => err.to_string(),
    }
}

fn fixture_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}
