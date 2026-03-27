use app_live::{
    load_local_signer_config, load_neg_risk_live_targets, ConfigError, LocalL2AuthHeaders,
    LocalRelayerAuth, LocalSignerConfig, LocalSignerIdentity,
};
use app_live::config::load_polymarket_source_config;

#[test]
fn parses_neg_risk_live_target_config_from_env_json() {
    let json = r#"
    [
      {
        "family_id": "family-a",
        "members": [
          { "condition_id": "condition-1", "token_id": "token-1", "price": "0.43", "quantity": "5" },
          { "condition_id": "condition-2", "token_id": "token-2", "price": "0.41", "quantity": "5" }
        ]
      }
    ]
    "#;

    let config = load_neg_risk_live_targets(Some(json)).unwrap();
    assert_eq!(config.targets()["family-a"].members.len(), 2);
    assert_eq!(config.targets()["family-a"].members[0].token_id, "token-1");
}

#[test]
fn missing_neg_risk_live_target_config_returns_empty_map() {
    let config = load_neg_risk_live_targets(None).unwrap();

    assert!(config.is_empty());
}

#[test]
fn live_target_config_reports_stable_revision_for_startup_set() {
    let json_a = r#"
    [
      {
        "family_id": "family-b",
        "members": [
          { "condition_id": "condition-2", "token_id": "token-2", "price": "0.4100", "quantity": "5.0" }
        ]
      },
      {
        "family_id": "family-a",
        "members": [
          { "condition_id": "condition-2", "token_id": "token-2", "price": "0.410", "quantity": "5" },
          { "condition_id": "condition-1", "token_id": "token-1", "price": "0.4300", "quantity": "5.00" }
        ]
      }
    ]
    "#;
    let json_b = r#"
    [
      {
        "family_id": "family-a",
        "members": [
          { "condition_id": "condition-1", "token_id": "token-1", "price": "0.43", "quantity": "5" },
          { "condition_id": "condition-2", "token_id": "token-2", "price": "0.41", "quantity": "5.0" }
        ]
      },
      {
        "family_id": "family-b",
        "members": [
          { "condition_id": "condition-2", "token_id": "token-2", "price": "0.41", "quantity": "5" }
        ]
      }
    ]
    "#;

    let config_a = load_neg_risk_live_targets(Some(json_a)).unwrap();
    let config_b = load_neg_risk_live_targets(Some(json_b)).unwrap();

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
fn blank_neg_risk_live_target_config_is_invalid() {
    let error = load_neg_risk_live_targets(Some("")).unwrap_err();

    assert!(matches!(error, ConfigError::InvalidJson { .. }));
}

#[test]
fn rejects_invalid_neg_risk_live_target_config_json() {
    let error = load_neg_risk_live_targets(Some("{")).unwrap_err();

    assert!(matches!(error, ConfigError::InvalidJson { .. }));
    assert!(error
        .to_string()
        .contains("invalid neg-risk live target config"));
}

#[test]
fn rejects_duplicate_neg_risk_family_ids() {
    let json = r#"
    [
      {
        "family_id": "family-a",
        "members": [
          { "condition_id": "condition-1", "token_id": "token-1", "price": "0.43", "quantity": "5" }
        ]
      },
      {
        "family_id": "family-a",
        "members": [
          { "condition_id": "condition-2", "token_id": "token-2", "price": "0.41", "quantity": "5" }
        ]
      }
    ]
    "#;

    let error = load_neg_risk_live_targets(Some(json)).unwrap_err();

    assert_eq!(
        error,
        ConfigError::DuplicateFamilyId {
            family_id: "family-a".to_owned()
        }
    );
}

#[test]
fn parses_local_signer_config_from_env_json() {
    let json = r#"
    {
      "signer": {
        "address": "0x1111111111111111111111111111111111111111",
        "funder_address": "0x2222222222222222222222222222222222222222",
        "signature_type": "Eoa",
        "wallet_route": "Eoa"
      },
      "l2_auth": {
        "api_key": "poly-api-key-1",
        "passphrase": "poly-passphrase-1",
        "timestamp": "1700000000",
        "signature": "poly-signature-1"
      },
      "relayer_auth": {
        "kind": "builder_api_key",
        "api_key": "builder-api-key-1",
        "timestamp": "1700000001",
        "passphrase": "builder-passphrase-1",
        "signature": "builder-signature-1"
      }
    }
    "#;

    let config = load_local_signer_config(Some(json)).unwrap();
    assert_eq!(
        config,
        LocalSignerConfig {
            signer: LocalSignerIdentity {
                address: "0x1111111111111111111111111111111111111111".to_owned(),
                funder_address: "0x2222222222222222222222222222222222222222".to_owned(),
                signature_type: "Eoa".to_owned(),
                wallet_route: "Eoa".to_owned(),
            },
            l2_auth: LocalL2AuthHeaders {
                api_key: "poly-api-key-1".to_owned(),
                passphrase: "poly-passphrase-1".to_owned(),
                timestamp: "1700000000".to_owned(),
                signature: "poly-signature-1".to_owned(),
            },
            relayer_auth: LocalRelayerAuth::BuilderApiKey {
                api_key: "builder-api-key-1".to_owned(),
                timestamp: "1700000001".to_owned(),
                passphrase: "builder-passphrase-1".to_owned(),
                signature: "builder-signature-1".to_owned(),
            },
        }
    );
}

#[test]
fn missing_local_signer_config_returns_error() {
    let error = load_local_signer_config(None).unwrap_err();

    assert!(matches!(error, ConfigError::MissingLocalSignerConfig));
    assert!(error.to_string().contains("missing local signer config"));
}

#[test]
fn rejects_invalid_local_signer_config_json() {
    let error = load_local_signer_config(Some("{")).unwrap_err();

    assert!(matches!(
        error,
        ConfigError::InvalidLocalSignerConfig { .. }
    ));
    assert!(error.to_string().contains("invalid local signer config"));
}

#[test]
fn parses_polymarket_source_config_from_env_json() {
    let json = r#"
    {
      "clob_host": "https://clob.polymarket.com",
      "data_api_host": "https://data-api.polymarket.com",
      "relayer_host": "https://relayer-v2.polymarket.com",
      "market_ws_url": "wss://ws-subscriptions.polymarket.com/market",
      "user_ws_url": "wss://ws-subscriptions.polymarket.com/user",
      "heartbeat_interval_seconds": 15,
      "relayer_poll_interval_seconds": 5,
      "metadata_refresh_interval_seconds": 60
    }
    "#;

    let config = load_polymarket_source_config(Some(json)).unwrap();

    assert_eq!(config.clob_host.as_str(), "https://clob.polymarket.com/");
    assert_eq!(
        config.data_api_host.as_str(),
        "https://data-api.polymarket.com/"
    );
    assert_eq!(
        config.relayer_host.as_str(),
        "https://relayer-v2.polymarket.com/"
    );
    assert_eq!(
        config.market_ws_url.as_str(),
        "wss://ws-subscriptions.polymarket.com/market"
    );
    assert_eq!(
        config.user_ws_url.as_str(),
        "wss://ws-subscriptions.polymarket.com/user"
    );
    assert_eq!(config.heartbeat_interval_seconds, 15);
    assert_eq!(config.relayer_poll_interval_seconds, 5);
    assert_eq!(config.metadata_refresh_interval_seconds, 60);
}
