use std::{collections::BTreeMap, fs, path::PathBuf, process::Command};

use app_live::config::PolymarketSourceConfig;
use app_live::{
    load_real_user_shadow_smoke_config, ConfigError, LocalRelayerAuth, LocalSignerConfig,
    LocalSignerIdentity, NegRiskFamilyLiveTarget, NegRiskLiveTargetSet, NegRiskMemberLiveTarget,
    PolymarketGatewayCredentials, RealUserShadowSmokeConfig,
};
use config_schema::{load_raw_config_from_str, ValidatedConfig};
use rust_decimal::Decimal;

#[test]
fn parses_neg_risk_live_target_config_from_validated_view() {
    let config = NegRiskLiveTargetSet::try_from(&live_view(NEG_RISK_TARGETS_A)).unwrap();

    assert_eq!(config.targets()["family-a"].members.len(), 2);
    assert_eq!(config.targets()["family-a"].members[0].token_id, "token-2");
}

#[test]
fn missing_neg_risk_live_target_config_returns_empty_map() {
    let config =
        NegRiskLiveTargetSet::try_from(&paper_view("[runtime]\nmode = \"paper\"\n")).unwrap();

    assert!(config.is_empty());
}

#[test]
fn live_target_config_reports_stable_revision_for_startup_set() {
    let config_a = NegRiskLiveTargetSet::try_from(&live_view(NEG_RISK_TARGETS_A)).unwrap();
    let config_b = NegRiskLiveTargetSet::try_from(&live_view(NEG_RISK_TARGETS_B)).unwrap();

    assert_eq!(config_a.revision(), config_b.revision());
    assert!(config_a.revision().starts_with("sha256:"));
    assert_eq!(
        config_a.targets().keys().cloned().collect::<Vec<_>>(),
        vec!["family-a".to_owned(), "family-b".to_owned()]
    );
    assert_eq!(
        config_a.targets()["family-a"].members[0].token_id,
        "token-2"
    );
}

#[test]
fn duplicate_neg_risk_family_ids_are_rejected_by_validation() {
    let error = validated_err(
        r#"
[runtime]
mode = "live"

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

    assert!(error.contains("duplicate negrisk.targets.family_id"));
}

#[test]
fn live_view_rejects_invalid_rollout_references() {
    let raw = load_raw_config_from_str(
        r#"
[runtime]
mode = "live"
real_user_shadow_smoke = false

[strategy_control]
source = "adopted"
operator_strategy_revision = "strategy-rev-12"

[polymarket.source]
clob_host = "https://clob.polymarket.com"
data_api_host = "https://gamma-api.polymarket.com"
relayer_host = "https://relayer-v2.polymarket.com"
market_ws_url = "wss://ws-subscriptions-clob.polymarket.com/ws/market"
user_ws_url = "wss://ws-subscriptions-clob.polymarket.com/ws/user"
heartbeat_interval_seconds = 15
relayer_poll_interval_seconds = 5
metadata_refresh_interval_seconds = 60

[polymarket.account]
address = "0x1111111111111111111111111111111111111111"
funder_address = "0x2222222222222222222222222222222222222222"
signature_type = "eoa"
wallet_route = "eoa"
api_key = "poly-api-key-1"
secret = "poly-secret-1"
passphrase = "poly-passphrase-1"

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
    let error = validated
        .for_app_live()
        .expect_err("live view should reject invalid rollout references");

    assert!(error
        .to_string()
        .contains("approved_families references missing family_id"));
}

#[test]
fn pure_neutral_adopted_strategy_control_shape_loads_in_app_live_view() {
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

[polymarket.account]
address = "0x1111111111111111111111111111111111111111"
funder_address = "0x2222222222222222222222222222222222222222"
signature_type = "eoa"
wallet_route = "eoa"
api_key = "poly-api-key-1"
secret = "poly-secret-1"
passphrase = "poly-passphrase-1"

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
        .expect("pure neutral rollout should bridge into live view");
    assert_eq!(rollout.approved_families(), &["family-a".to_owned()]);
    assert_eq!(rollout.ready_families(), &["family-a".to_owned()]);
    assert!(live.has_adopted_strategy_source());
    assert_eq!(live.operator_strategy_revision(), Some("strategy-rev-12"));
    assert!(!live.is_legacy_explicit_strategy_config());
}

#[test]
fn derives_local_signer_config_from_account_backed_live_view() {
    let config = LocalSignerConfig::try_from(&live_view("")).unwrap();

    assert_eq!(
        config.signer,
        LocalSignerIdentity {
            address: "0x1111111111111111111111111111111111111111".to_owned(),
            funder_address: "0x2222222222222222222222222222222222222222".to_owned(),
            signature_type: "Eoa".to_owned(),
            wallet_route: "Eoa".to_owned(),
        }
    );
    assert_eq!(config.l2_auth.api_key, "poly-api-key-1");
    assert_eq!(config.l2_auth.passphrase, "poly-passphrase-1");
    assert!(!config.l2_auth.timestamp.is_empty());
    assert!(!config.l2_auth.signature.is_empty());
    assert_eq!(
        config.relayer_auth,
        LocalRelayerAuth::BuilderApiKey {
            api_key: "builder-api-key-1".to_owned(),
            timestamp: "1700000001".to_owned(),
            passphrase: "builder-passphrase-1".to_owned(),
            signature: "builder-signature-1".to_owned(),
        }
    );
}

#[test]
fn operator_facing_account_derives_local_signer_config() {
    let config = LocalSignerConfig::try_from(&operator_live_view()).unwrap();

    assert_eq!(
        config.signer,
        LocalSignerIdentity {
            address: "0x1111111111111111111111111111111111111111".to_owned(),
            funder_address: "0x2222222222222222222222222222222222222222".to_owned(),
            signature_type: "Eoa".to_owned(),
            wallet_route: "Eoa".to_owned(),
        }
    );
    assert_eq!(config.l2_auth.api_key, "poly-api-key");
    assert_eq!(config.l2_auth.passphrase, "poly-passphrase");
    assert!(!config.l2_auth.timestamp.is_empty());
    assert!(!config.l2_auth.signature.is_empty());
    assert_eq!(
        config.relayer_auth,
        LocalRelayerAuth::RelayerApiKey {
            api_key: "relay-key".to_owned(),
            address: "0x1111111111111111111111111111111111111111".to_owned(),
        }
    );
}

#[test]
fn operator_facing_account_derives_builder_relayer_auth() {
    let config = LocalSignerConfig::try_from(&operator_live_view_with_builder_auth()).unwrap();

    match config.relayer_auth {
        LocalRelayerAuth::BuilderApiKey {
            api_key,
            timestamp,
            passphrase,
            signature,
        } => {
            assert_eq!(api_key, "builder-api-key");
            assert_eq!(passphrase, "builder-passphrase");
            assert!(!timestamp.is_empty());
            assert!(!signature.is_empty());
        }
        other => panic!("expected builder auth, got {other:?}"),
    }
}

#[test]
fn live_view_rejects_polymarket_signer_after_cutover() {
    let error = validated_view_err(
        r#"
[runtime]
mode = "live"
real_user_shadow_smoke = false

[strategy_control]
source = "adopted"
operator_strategy_revision = "strategy-rev-12"

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
    );

    assert!(error.contains("polymarket.signer is no longer supported"));
}

#[test]
fn operator_facing_account_builds_gateway_credentials() {
    let credentials = PolymarketGatewayCredentials::try_from(&operator_live_view()).unwrap();

    assert_eq!(
        credentials.address,
        "0x1111111111111111111111111111111111111111"
    );
    assert_eq!(
        credentials.funder_address,
        "0x2222222222222222222222222222222222222222"
    );
    assert_eq!(credentials.signature_type, "Eoa");
    assert_eq!(credentials.wallet_route, "Eoa");
    assert_eq!(credentials.api_key, "poly-api-key");
    assert_eq!(credentials.secret, "poly-secret");
    assert_eq!(credentials.passphrase, "poly-passphrase");
}

#[test]
fn venue_polymarket_public_lib_rejects_removed_root_reexports() {
    let venue_polymarket_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../venue-polymarket");
    let temp_dir = tempfile::tempdir().expect("temp dir");
    fs::create_dir(temp_dir.path().join("src")).expect("test src dir");
    fs::write(
        temp_dir.path().join("Cargo.toml"),
        format!(
            r#"[package]
name = "venue-polymarket-root-export-probe"
version = "0.1.0"
edition = "2021"

[dependencies]
venue-polymarket = {{ path = "{}" }}
"#,
            venue_polymarket_root.display()
        ),
    )
    .expect("probe manifest");
    fs::write(
        temp_dir.path().join("src/main.rs"),
        r#"
use venue_polymarket::{L2AuthHeaders, PolymarketRestClient, PolymarketWsClient, SignerContext};

fn main() {}
"#,
    )
    .expect("probe main");

    let output = Command::new(env!("CARGO"))
        .arg("check")
        .arg("--quiet")
        .arg("--manifest-path")
        .arg(temp_dir.path().join("Cargo.toml"))
        .output()
        .expect("cargo check should run");

    assert!(!output.status.success(), "probe unexpectedly compiled");
    let stderr = String::from_utf8_lossy(&output.stderr);
    for legacy in [
        "PolymarketRestClient",
        "PolymarketWsClient",
        "L2AuthHeaders",
        "SignerContext",
    ] {
        assert!(
            stderr.contains(legacy),
            "expected compile failure for removed root export {legacy}; stderr was: {stderr}"
        );
    }
}

#[test]
fn from_targets_with_revision_preserves_external_revision() {
    let config = NegRiskLiveTargetSet::from_targets_with_revision(
        "targets-rev-9",
        BTreeMap::from([(
            "family-a".to_owned(),
            NegRiskFamilyLiveTarget {
                family_id: "family-a".to_owned(),
                members: vec![NegRiskMemberLiveTarget {
                    condition_id: "condition-1".to_owned(),
                    token_id: "token-1".to_owned(),
                    price: Decimal::new(43, 2),
                    quantity: Decimal::new(5, 0),
                }],
            },
        )]),
    );

    assert_eq!(config.revision(), "targets-rev-9");
    assert_eq!(config.targets()["family-a"].members.len(), 1);
}

#[test]
fn missing_local_signer_config_returns_error() {
    let error =
        LocalSignerConfig::try_from(&paper_view("[runtime]\nmode = \"paper\"\n")).unwrap_err();

    assert!(matches!(error, ConfigError::MissingLocalSignerConfig));
    assert!(error.to_string().contains("missing local signer config"));
}

#[test]
fn account_fields_trim_whitespace_before_validation() {
    let config = format!("{BASE_LIVE_CONFIG}\n{DEFAULT_ROLLOUT}").replace(
        "address = \"0x1111111111111111111111111111111111111111\"",
        "address = \"   \"",
    );
    let error = validated_err(&config);

    assert!(error.to_string().contains("polymarket.account.address"));
}

#[test]
fn relayer_auth_fields_trim_whitespace_before_validation() {
    let config = format!("{BASE_LIVE_CONFIG}\n{DEFAULT_ROLLOUT}")
        .replace("timestamp = \"1700000001\"", "timestamp = \"   \"")
        .replace("signature = \"builder-signature-1\"", "signature = \"   \"");
    let raw = load_raw_config_from_str(&config).unwrap();
    let validated = ValidatedConfig::new(raw).unwrap();

    let error = validated
        .for_app_live()
        .expect_err("whitespace-only relayer auth fields should be rejected");

    assert!(error
        .to_string()
        .contains("polymarket.relayer_auth.timestamp"));
}

#[test]
fn parses_polymarket_source_config_from_validated_view() {
    let config = PolymarketSourceConfig::try_from(&live_view("")).unwrap();

    assert_eq!(config.clob_host.as_str(), "https://clob.polymarket.com/");
    assert_eq!(
        config.data_api_host.as_str(),
        "https://gamma-api.polymarket.com/"
    );
    assert_eq!(
        config.relayer_host.as_str(),
        "https://relayer-v2.polymarket.com/"
    );
    assert_eq!(
        config.market_ws_url.as_str(),
        "wss://ws-subscriptions-clob.polymarket.com/ws/market"
    );
    assert_eq!(
        config.user_ws_url.as_str(),
        "wss://ws-subscriptions-clob.polymarket.com/ws/user"
    );
    assert_eq!(config.heartbeat_interval_seconds, 15);
    assert_eq!(config.relayer_poll_interval_seconds, 5);
    assert_eq!(config.metadata_refresh_interval_seconds, 60);
}

#[test]
fn operator_facing_live_config_without_source_uses_default_polymarket_source() {
    let config = PolymarketSourceConfig::try_from(&operator_live_view_without_source()).unwrap();

    assert_eq!(config.clob_host.as_str(), "https://clob.polymarket.com/");
    assert_eq!(
        config.data_api_host.as_str(),
        "https://gamma-api.polymarket.com/"
    );
    assert_eq!(
        config.relayer_host.as_str(),
        "https://relayer-v2.polymarket.com/"
    );
    assert_eq!(
        config.market_ws_url.as_str(),
        "wss://ws-subscriptions-clob.polymarket.com/ws/market"
    );
    assert_eq!(
        config.user_ws_url.as_str(),
        "wss://ws-subscriptions-clob.polymarket.com/ws/user"
    );
    assert_eq!(config.heartbeat_interval_seconds, 15);
    assert_eq!(config.relayer_poll_interval_seconds, 5);
    assert_eq!(config.metadata_refresh_interval_seconds, 60);
}

#[test]
fn operator_facing_smoke_fixture_without_source_uses_default_polymarket_source() {
    let config = PolymarketSourceConfig::try_from(&smoke_view_without_source()).unwrap();

    assert_eq!(config.clob_host.as_str(), "https://clob.polymarket.com/");
    assert_eq!(
        config.data_api_host.as_str(),
        "https://gamma-api.polymarket.com/"
    );
    assert_eq!(
        config.relayer_host.as_str(),
        "https://relayer-v2.polymarket.com/"
    );
    assert_eq!(
        config.market_ws_url.as_str(),
        "wss://ws-subscriptions-clob.polymarket.com/ws/market"
    );
    assert_eq!(
        config.user_ws_url.as_str(),
        "wss://ws-subscriptions-clob.polymarket.com/ws/user"
    );
    assert_eq!(config.heartbeat_interval_seconds, 15);
    assert_eq!(config.relayer_poll_interval_seconds, 5);
    assert_eq!(config.metadata_refresh_interval_seconds, 60);
}

#[test]
fn source_overrides_win_over_source_when_both_are_present() {
    let config = PolymarketSourceConfig::try_from(&live_view_with_source_overrides()).unwrap();

    assert_eq!(
        config.clob_host.as_str(),
        "https://override-clob.polymarket.com/"
    );
    assert_eq!(
        config.data_api_host.as_str(),
        "https://override-data-api.polymarket.com/"
    );
    assert_eq!(
        config.relayer_host.as_str(),
        "https://override-relayer.polymarket.com/"
    );
    assert_eq!(
        config.market_ws_url.as_str(),
        "wss://override-ws.polymarket.com/ws/market"
    );
    assert_eq!(
        config.user_ws_url.as_str(),
        "wss://override-ws.polymarket.com/ws/user"
    );
    assert_eq!(config.heartbeat_interval_seconds, 22);
    assert_eq!(config.relayer_poll_interval_seconds, 11);
    assert_eq!(config.metadata_refresh_interval_seconds, 99);
}

#[test]
fn parses_optional_polymarket_http_proxy_from_validated_view() {
    let config = PolymarketSourceConfig::try_from(&live_view(
        r#"
[polymarket.http]
proxy_url = "http://127.0.0.1:7897"
"#,
    ))
    .unwrap();

    assert_eq!(
        config.outbound_proxy_url.as_ref().map(|url| url.as_str()),
        Some("http://127.0.0.1:7897/")
    );
}

#[test]
fn gateway_credentials_path_is_unchanged_when_source_is_omitted() {
    let credentials =
        PolymarketGatewayCredentials::try_from(&operator_live_view_without_source()).unwrap();

    assert_eq!(
        credentials.address,
        "0x1111111111111111111111111111111111111111"
    );
    assert_eq!(
        credentials.funder_address,
        "0x2222222222222222222222222222222222222222"
    );
    assert_eq!(credentials.signature_type, "Eoa");
    assert_eq!(credentials.wallet_route, "Eoa");
    assert_eq!(credentials.api_key, "poly-api-key");
    assert_eq!(credentials.secret, "poly-secret");
    assert_eq!(credentials.passphrase, "poly-passphrase");
}

#[test]
fn rejects_polymarket_source_config_with_non_http_hosts() {
    let error = source_config_err(
        r#"
[polymarket.source]
clob_host = "ftp://clob.polymarket.com"
data_api_host = "https://gamma-api.polymarket.com"
relayer_host = "https://relayer-v2.polymarket.com"
market_ws_url = "wss://ws-subscriptions-clob.polymarket.com/ws/market"
user_ws_url = "wss://ws-subscriptions-clob.polymarket.com/ws/user"
heartbeat_interval_seconds = 15
relayer_poll_interval_seconds = 5
metadata_refresh_interval_seconds = 60
"#,
    );

    assert!(error.contains("clob_host"));
}

#[test]
fn rejects_polymarket_source_config_with_non_ws_urls() {
    let error = source_config_err(
        r#"
[polymarket.source]
clob_host = "https://clob.polymarket.com"
data_api_host = "https://gamma-api.polymarket.com"
relayer_host = "https://relayer-v2.polymarket.com"
market_ws_url = "https://ws-subscriptions-clob.polymarket.com/ws/market"
user_ws_url = "wss://ws-subscriptions-clob.polymarket.com/ws/user"
heartbeat_interval_seconds = 15
relayer_poll_interval_seconds = 5
metadata_refresh_interval_seconds = 60
"#,
    );

    assert!(error.contains("market_ws_url"));
}

#[test]
fn rejects_polymarket_source_config_with_zero_cadence() {
    let error = source_config_err(
        r#"
[polymarket.source]
clob_host = "https://clob.polymarket.com"
data_api_host = "https://gamma-api.polymarket.com"
relayer_host = "https://relayer-v2.polymarket.com"
market_ws_url = "wss://ws-subscriptions-clob.polymarket.com/ws/market"
user_ws_url = "wss://ws-subscriptions-clob.polymarket.com/ws/user"
heartbeat_interval_seconds = 0
relayer_poll_interval_seconds = 5
metadata_refresh_interval_seconds = 60
"#,
    );

    assert!(error.contains("heartbeat_interval_seconds"));
}

#[test]
fn rejects_polymarket_source_config_with_non_root_host_path() {
    let error = source_config_err(
        r#"
[polymarket.source]
clob_host = "https://clob.polymarket.com/api"
data_api_host = "https://gamma-api.polymarket.com"
relayer_host = "https://relayer-v2.polymarket.com"
market_ws_url = "wss://ws-subscriptions-clob.polymarket.com/ws/market"
user_ws_url = "wss://ws-subscriptions-clob.polymarket.com/ws/user"
heartbeat_interval_seconds = 15
relayer_poll_interval_seconds = 5
metadata_refresh_interval_seconds = 60
"#,
    );

    assert!(error.contains("clob_host"));
}

#[test]
fn rejects_polymarket_source_config_with_host_query_or_fragment() {
    let query_error = source_config_err(
        r#"
[polymarket.source]
clob_host = "https://clob.polymarket.com?foo=bar"
data_api_host = "https://gamma-api.polymarket.com"
relayer_host = "https://relayer-v2.polymarket.com"
market_ws_url = "wss://ws-subscriptions-clob.polymarket.com/ws/market"
user_ws_url = "wss://ws-subscriptions-clob.polymarket.com/ws/user"
heartbeat_interval_seconds = 15
relayer_poll_interval_seconds = 5
metadata_refresh_interval_seconds = 60
"#,
    );
    let fragment_error = source_config_err(
        r#"
[polymarket.source]
clob_host = "https://clob.polymarket.com"
data_api_host = "https://gamma-api.polymarket.com#fragment"
relayer_host = "https://relayer-v2.polymarket.com"
market_ws_url = "wss://ws-subscriptions-clob.polymarket.com/ws/market"
user_ws_url = "wss://ws-subscriptions-clob.polymarket.com/ws/user"
heartbeat_interval_seconds = 15
relayer_poll_interval_seconds = 5
metadata_refresh_interval_seconds = 60
"#,
    );

    assert!(query_error.contains("clob_host"));
    assert!(fragment_error.contains("data_api_host"));
}

#[test]
fn parses_real_user_shadow_smoke_guard_when_enabled() {
    let smoke = load_real_user_shadow_smoke_config(&smoke_view())
        .unwrap()
        .expect("smoke should be enabled");

    assert_eq!(
        smoke,
        RealUserShadowSmokeConfig {
            enabled: true,
            source_config: PolymarketSourceConfig::try_from(&smoke_view()).unwrap(),
        }
    );
}

#[test]
fn non_enabled_real_user_shadow_smoke_is_ignored() {
    assert_eq!(
        load_real_user_shadow_smoke_config(&live_view("")).unwrap(),
        None
    );
}

#[test]
fn paper_mode_rejects_real_user_shadow_smoke_when_enabled() {
    let error = validated_err(
        r#"
[runtime]
mode = "paper"
real_user_shadow_smoke = true
"#,
    );

    assert!(error.contains("real_user_shadow_smoke"));
}

fn paper_view(extra: &str) -> config_schema::AppLiveConfigView<'static> {
    let raw = Box::leak(Box::new(
        load_raw_config_from_str(extra).expect("config should parse"),
    ));
    let validated = Box::leak(Box::new(
        ValidatedConfig::new(raw.clone()).expect("config should validate"),
    ));

    validated.for_app_live().expect("view should validate")
}

fn live_view(extra: &str) -> config_schema::AppLiveConfigView<'static> {
    let negrisk = if extra.contains("[negrisk.rollout]") {
        extra.to_owned()
    } else {
        format!("{DEFAULT_ROLLOUT}\n{extra}")
    };
    let raw = Box::leak(Box::new(
        load_raw_config_from_str(&format!("{BASE_LIVE_CONFIG}\n{negrisk}"))
            .expect("config should parse"),
    ));
    let validated = Box::leak(Box::new(
        ValidatedConfig::new(raw.clone()).expect("config should validate"),
    ));

    validated.for_app_live().expect("live view should validate")
}

fn smoke_view() -> config_schema::AppLiveConfigView<'static> {
    let raw = Box::leak(Box::new(
        load_raw_config_from_str(BASE_SMOKE_CONFIG).expect("config should parse"),
    ));
    let validated = Box::leak(Box::new(
        ValidatedConfig::new(raw.clone()).expect("config should validate"),
    ));

    validated
        .for_app_live()
        .expect("smoke view should validate")
}

fn smoke_view_without_source() -> config_schema::AppLiveConfigView<'static> {
    let raw = Box::leak(Box::new(
        load_raw_config_from_str(
            r#"
[runtime]
mode = "live"
real_user_shadow_smoke = true

[strategy_control]
source = "adopted"
operator_strategy_revision = "targets-rev-9"

[polymarket.relayer_auth]
kind = "builder_api_key"
api_key = "builder-api-key-1"
timestamp = "1700000001"
passphrase = "builder-passphrase-1"
signature = "builder-signature-1"

[polymarket.account]
address = "0x1111111111111111111111111111111111111111"
funder_address = "0x2222222222222222222222222222222222222222"
signature_type = "eoa"
wallet_route = "eoa"
api_key = "poly-api-key-1"
secret = "poly-secret-1"
passphrase = "poly-passphrase-1"

[negrisk.rollout]
approved_families = []
ready_families = []
"#,
        )
        .expect("config should parse"),
    ));
    let validated = Box::leak(Box::new(
        ValidatedConfig::new(raw.clone()).expect("config should validate"),
    ));

    validated
        .for_app_live()
        .expect("smoke view should validate")
}

fn operator_live_view() -> config_schema::AppLiveConfigView<'static> {
    operator_live_view_from(
        r#"
[polymarket.relayer_auth]
kind = "relayer_api_key"
api_key = "relay-key"
address = "0x1111111111111111111111111111111111111111"
"#,
    )
}

fn operator_live_view_with_builder_auth() -> config_schema::AppLiveConfigView<'static> {
    operator_live_view_from(
        r#"
[polymarket.relayer_auth]
kind = "builder_api_key"
api_key = "builder-api-key"
secret = "builder-secret"
passphrase = "builder-passphrase"
"#,
    )
}

fn operator_live_view_without_source() -> config_schema::AppLiveConfigView<'static> {
    let raw = Box::leak(Box::new(
        load_raw_config_from_str(
            r#"
[runtime]
mode = "live"
real_user_shadow_smoke = false

[polymarket.account]
address = "0x1111111111111111111111111111111111111111"
funder_address = "0x2222222222222222222222222222222222222222"
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
        .expect("config should parse"),
    ));
    let validated = Box::leak(Box::new(
        ValidatedConfig::new(raw.clone()).expect("config should validate"),
    ));

    validated.for_app_live().expect("live view should validate")
}

fn live_view_with_source_overrides() -> config_schema::AppLiveConfigView<'static> {
    let raw = Box::leak(Box::new(
        load_raw_config_from_str(
            r#"
[runtime]
mode = "live"
real_user_shadow_smoke = false

[polymarket.source_overrides]
clob_host = "https://override-clob.polymarket.com"
data_api_host = "https://override-data-api.polymarket.com"
relayer_host = "https://override-relayer.polymarket.com"
market_ws_url = "wss://override-ws.polymarket.com/ws/market"
user_ws_url = "wss://override-ws.polymarket.com/ws/user"
heartbeat_interval_seconds = 22
relayer_poll_interval_seconds = 11
metadata_refresh_interval_seconds = 99

[polymarket.source]
clob_host = "https://clob.polymarket.com"
data_api_host = "https://gamma-api.polymarket.com"
relayer_host = "https://relayer-v2.polymarket.com"
market_ws_url = "wss://ws-subscriptions-clob.polymarket.com/ws/market"
user_ws_url = "wss://ws-subscriptions-clob.polymarket.com/ws/user"
heartbeat_interval_seconds = 15
relayer_poll_interval_seconds = 5
metadata_refresh_interval_seconds = 60

[polymarket.account]
address = "0x1111111111111111111111111111111111111111"
funder_address = "0x2222222222222222222222222222222222222222"
signature_type = "eoa"
wallet_route = "eoa"
api_key = "poly-api-key-1"
secret = "poly-secret-1"
passphrase = "poly-passphrase-1"

[polymarket.relayer_auth]
kind = "builder_api_key"
api_key = "builder-api-key-1"
timestamp = "1700000001"
passphrase = "builder-passphrase-1"
signature = "builder-signature-1"

[negrisk.target_source]
source = "adopted"
operator_target_revision = "targets-rev-9"

[negrisk.rollout]
approved_families = []
ready_families = []
"#,
        )
        .expect("config should parse"),
    ));
    let validated = Box::leak(Box::new(
        ValidatedConfig::new(raw.clone()).expect("config should validate"),
    ));

    validated.for_app_live().expect("live view should validate")
}

fn operator_live_view_from(relayer_auth: &str) -> config_schema::AppLiveConfigView<'static> {
    let raw = Box::leak(Box::new(
        load_raw_config_from_str(&format!(
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

[polymarket.account]
address = "0x1111111111111111111111111111111111111111"
funder_address = "0x2222222222222222222222222222222222222222"
signature_type = "eoa"
wallet_route = "eoa"
api_key = "poly-api-key"
secret = "poly-secret"
passphrase = "poly-passphrase"

{relayer_auth}

[negrisk.target_source]
source = "adopted"
operator_target_revision = "targets-rev-9"
"#,
        ))
        .expect("config should parse"),
    ));
    let validated = Box::leak(Box::new(
        ValidatedConfig::new(raw.clone()).expect("config should validate"),
    ));

    validated.for_app_live().expect("live view should validate")
}

fn validated_err(text: &str) -> String {
    match load_raw_config_from_str(text) {
        Ok(raw) => match ValidatedConfig::new(raw) {
            Ok(validated) => validated.for_app_live().unwrap_err().to_string(),
            Err(error) => error.to_string(),
        },
        Err(error) => error.to_string(),
    }
}

fn validated_view_err(text: &str) -> String {
    let raw = load_raw_config_from_str(text).expect("config should parse");
    let validated = ValidatedConfig::new(raw).expect("config should validate globally");
    validated
        .for_app_live()
        .expect_err("live view should reject config")
        .to_string()
}

fn source_config_err(source_table: &str) -> String {
    let raw = load_raw_config_from_str(&format!("{BASE_LIVE_WITH_RAW_SOURCE}\n{source_table}"))
        .expect("config should parse");
    match ValidatedConfig::new(raw) {
        Ok(validated) => PolymarketSourceConfig::try_from(
            &validated.for_app_live().expect("live view should validate"),
        )
        .unwrap_err()
        .to_string(),
        Err(error) => error.to_string(),
    }
}

const BASE_LIVE_CONFIG: &str = r#"
[runtime]
mode = "live"
real_user_shadow_smoke = false

[strategy_control]
source = "adopted"
operator_strategy_revision = "strategy-rev-12"

[polymarket.source]
clob_host = "https://clob.polymarket.com"
data_api_host = "https://gamma-api.polymarket.com"
relayer_host = "https://relayer-v2.polymarket.com"
market_ws_url = "wss://ws-subscriptions-clob.polymarket.com/ws/market"
user_ws_url = "wss://ws-subscriptions-clob.polymarket.com/ws/user"
heartbeat_interval_seconds = 15
relayer_poll_interval_seconds = 5
metadata_refresh_interval_seconds = 60

[polymarket.account]
address = "0x1111111111111111111111111111111111111111"
funder_address = "0x2222222222222222222222222222222222222222"
signature_type = "eoa"
wallet_route = "eoa"
api_key = "poly-api-key-1"
secret = "poly-secret-1"
passphrase = "poly-passphrase-1"

[polymarket.relayer_auth]
kind = "builder_api_key"
api_key = "builder-api-key-1"
timestamp = "1700000001"
passphrase = "builder-passphrase-1"
signature = "builder-signature-1"
"#;

const BASE_LIVE_WITH_RAW_SOURCE: &str = r#"
[runtime]
mode = "live"
real_user_shadow_smoke = false

[strategy_control]
source = "adopted"
operator_strategy_revision = "strategy-rev-12"

[polymarket.account]
address = "0x1111111111111111111111111111111111111111"
funder_address = "0x2222222222222222222222222222222222222222"
signature_type = "eoa"
wallet_route = "eoa"
api_key = "poly-api-key-1"
secret = "poly-secret-1"
passphrase = "poly-passphrase-1"

[polymarket.relayer_auth]
kind = "builder_api_key"
api_key = "builder-api-key-1"
timestamp = "1700000001"
passphrase = "builder-passphrase-1"
signature = "builder-signature-1"

[negrisk.rollout]
approved_families = []
ready_families = []
"#;

const BASE_SMOKE_CONFIG: &str = r#"
[runtime]
mode = "live"
real_user_shadow_smoke = true

[strategy_control]
source = "adopted"
operator_strategy_revision = "targets-rev-9"

[polymarket.source]
clob_host = "https://clob.polymarket.com"
data_api_host = "https://gamma-api.polymarket.com"
relayer_host = "https://relayer-v2.polymarket.com"
market_ws_url = "wss://ws-subscriptions-clob.polymarket.com/ws/market"
user_ws_url = "wss://ws-subscriptions-clob.polymarket.com/ws/user"
heartbeat_interval_seconds = 15
relayer_poll_interval_seconds = 5
metadata_refresh_interval_seconds = 60

[polymarket.account]
address = "0x1111111111111111111111111111111111111111"
funder_address = "0x2222222222222222222222222222222222222222"
signature_type = "eoa"
wallet_route = "eoa"
api_key = "poly-api-key-1"
secret = "poly-secret-1"
passphrase = "poly-passphrase-1"

[polymarket.relayer_auth]
kind = "builder_api_key"
api_key = "builder-api-key-1"
timestamp = "1700000001"
passphrase = "builder-passphrase-1"
signature = "builder-signature-1"

[negrisk.rollout]
approved_families = []
ready_families = []
"#;

const DEFAULT_ROLLOUT: &str = r#"
[negrisk.rollout]
approved_families = []
ready_families = []
"#;

const NEG_RISK_TARGETS_A: &str = r#"
[negrisk.rollout]
approved_families = ["family-a", "family-b"]
ready_families = ["family-a", "family-b"]

[[negrisk.targets]]
family_id = "family-b"

[[negrisk.targets.members]]
condition_id = "condition-2"
token_id = "token-2"
price = "0.4100"
quantity = "5.0"

[[negrisk.targets]]
family_id = "family-a"

[[negrisk.targets.members]]
condition_id = "condition-2"
token_id = "token-2"
price = "0.410"
quantity = "5"

[[negrisk.targets.members]]
condition_id = "condition-1"
token_id = "token-1"
price = "0.4300"
quantity = "5.00"
"#;

const NEG_RISK_TARGETS_B: &str = r#"
[negrisk.rollout]
approved_families = ["family-a", "family-b"]
ready_families = ["family-a", "family-b"]

[[negrisk.targets]]
family_id = "family-a"

[[negrisk.targets.members]]
condition_id = "condition-1"
token_id = "token-1"
price = "0.43"
quantity = "5"

[[negrisk.targets.members]]
condition_id = "condition-2"
token_id = "token-2"
price = "0.41"
quantity = "5.0"

[[negrisk.targets]]
family_id = "family-b"

[[negrisk.targets.members]]
condition_id = "condition-2"
token_id = "token-2"
price = "0.41"
quantity = "5"
"#;
