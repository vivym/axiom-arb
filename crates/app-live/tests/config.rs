use app_live::{load_neg_risk_live_targets, ConfigError};

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
    assert_eq!(config["family-a"].members.len(), 2);
    assert_eq!(config["family-a"].members[0].token_id, "token-1");
}

#[test]
fn missing_neg_risk_live_target_config_returns_empty_map() {
    let config = load_neg_risk_live_targets(None).unwrap();

    assert!(config.is_empty());
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
