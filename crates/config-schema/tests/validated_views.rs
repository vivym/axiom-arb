use std::path::{Path, PathBuf};

use config_schema::{
    load_raw_config_from_path, load_raw_config_from_str, RuntimeModeToml, ValidatedConfig,
};

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
fn smoke_view_requires_live_signer() {
    let raw = load_raw_config_from_path(&fixture_path("app-live-smoke.toml")).unwrap();
    let err = ValidatedConfig::new(raw)
        .unwrap()
        .for_app_live()
        .expect_err("smoke fixture missing signer should fail");

    assert!(err.to_string().contains("polymarket.signer"));
}

#[test]
fn smoke_view_requires_live_source() {
    let err = validated_view_err(
        r#"
[runtime]
mode = "live"
real_user_shadow_smoke = true

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
    assert!(config_err("[runtime]\nmode = \"invalid\"\n").contains("runtime.mode"));
    assert!(config_err(
        r#"
[runtime]
mode = "live"

[polymarket.source]
clob_host = "https://clob.polymarket.com"
data_api_host = "https://data-api.polymarket.com"
relayer_host = "https://relayer-v2.polymarket.com"
market_ws_url = "wss://ws-subscriptions-clob.polymarket.com/ws/market"
user_ws_url = "wss://ws-subscriptions-clob.polymarket.com/ws/user"
heartbeat_interval_seconds = 0
relayer_poll_interval_seconds = 5
metadata_refresh_interval_seconds = 60
"#
    )
    .contains("heartbeat_interval_seconds"));
    assert!(config_err(
        r#"
[runtime]
mode = "live"

[polymarket.signer]
address = "0x1111111111111111111111111111111111111111"
funder_address = "0x2222222222222222222222222222222222222222"
signature_type = "eoa"
wallet_route = "safe"
api_key = "poly-api-key-1"
passphrase = "poly-passphrase-1"
timestamp = "1700000000"
signature = "poly-signature-1"
"#
    )
    .contains("wallet_route"));
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
fn live_view_accepts_fully_populated_live_fixture() {
    let raw = load_raw_config_from_path(&fixture_path("app-live-live.toml")).unwrap();
    let validated = ValidatedConfig::new(raw).unwrap();

    let live = validated.for_app_live().expect("live view should validate");
    assert_eq!(live.mode(), RuntimeModeToml::Live);
    assert!(!live.real_user_shadow_smoke());
    assert!(live.has_polymarket_source());
    assert!(live.has_polymarket_signer());
}

#[test]
fn approved_and_ready_families_must_exist_in_targets() {
    let err = validated_err(
        r#"
[runtime]
mode = "live"

[polymarket.source]
clob_host = "https://clob.polymarket.com"
data_api_host = "https://data-api.polymarket.com"
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
approved_families = ["family-missing"]
ready_families = ["family-a"]

[[negrisk.targets]]
family_id = "family-a"

[[negrisk.targets.members]]
condition_id = "condition-1"
token_id = "token-1"
price = "0.43"
quantity = "5"
"#,
    );

    assert!(err.contains("approved_families"));
}

#[test]
fn duplicate_family_ids_are_rejected() {
    let err = validated_err(
        r#"
[runtime]
mode = "live"

[polymarket.source]
clob_host = "https://clob.polymarket.com"
data_api_host = "https://data-api.polymarket.com"
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

[[negrisk.targets]]
family_id = "family-a"

[[negrisk.targets.members]]
condition_id = "condition-2"
token_id = "token-2"
price = "0.41"
quantity = "5"
"#,
    );

    assert!(err.contains("family_id"));
}

#[test]
fn duplicate_or_malformed_target_members_are_rejected() {
    let malformed = validated_err(
        r#"
[runtime]
mode = "live"

[polymarket.source]
clob_host = "https://clob.polymarket.com"
data_api_host = "https://data-api.polymarket.com"
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
condition_id = ""
token_id = "token-1"
price = "0.43"
quantity = "5"
"#,
    );
    assert!(malformed.contains("condition_id"));

    let duplicate = validated_err(
        r#"
[runtime]
mode = "live"

[polymarket.source]
clob_host = "https://clob.polymarket.com"
data_api_host = "https://data-api.polymarket.com"
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

[[negrisk.targets.members]]
condition_id = "condition-1"
token_id = "token-1"
price = "0.43"
quantity = "5"
"#,
    );
    assert!(duplicate.contains("token_id"));
}

fn validated_err(extra: &str) -> String {
    let config = format!(
        r#"
[runtime]
mode = "live"
real_user_shadow_smoke = false

[polymarket.source]
clob_host = "https://clob.polymarket.com"
data_api_host = "https://data-api.polymarket.com"
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
    );

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
