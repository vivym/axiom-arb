use app_live::strategy_control::{
    resolve_strategy_control, CanonicalPersistenceState, ResolveStrategyControlError,
    ResolveStrategyControlInput, RuntimeProgressState,
};
use config_schema::{load_raw_config_from_str, AppLiveConfigView, ValidatedConfig};

#[test]
fn canonical_strategy_control_resolves_without_legacy_aliases() {
    let live = live_view(
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

[strategy_control]
source = "adopted"
operator_strategy_revision = "strategy-rev-12"
"#,
    );

    let resolved = resolve_strategy_control(ResolveStrategyControlInput {
        config: &live,
        canonical_persistence: Some(CanonicalPersistenceState::for_revision("strategy-rev-12")),
        runtime_progress: None,
    })
    .unwrap();

    assert_eq!(resolved.operator_strategy_revision, "strategy-rev-12");
    assert_eq!(resolved.active_operator_strategy_revision, None);
    assert!(!resolved.restart_required);
}

#[test]
fn explicit_targets_are_not_reported_as_steady_state_control_plane() {
    let live = live_view(
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

[[negrisk.targets]]
family_id = "family-a"

[[negrisk.targets.members]]
condition_id = "condition-1"
token_id = "token-1"
price = "0.43"
quantity = "5"
"#,
    );

    let result = resolve_strategy_control(ResolveStrategyControlInput {
        config: &live,
        canonical_persistence: None,
        runtime_progress: None,
    });

    assert!(matches!(
        result,
        Err(ResolveStrategyControlError::MigrationRequired(_))
    ));
}

#[test]
fn mixed_canonical_and_legacy_input_fails_closed() {
    let live = live_view(
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

[strategy_control]
source = "adopted"
operator_strategy_revision = "strategy-rev-12"

[negrisk.target_source]
source = "adopted"
operator_target_revision = "targets-rev-9"
"#,
    );

    let result = resolve_strategy_control(ResolveStrategyControlInput {
        config: &live,
        canonical_persistence: Some(CanonicalPersistenceState::for_revision("strategy-rev-12")),
        runtime_progress: None,
    });

    assert!(matches!(
        result,
        Err(ResolveStrategyControlError::InvalidConfig(_))
    ));
}

#[test]
fn empty_targets_array_is_invalid_legacy_input() {
    let live = live_view(
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

[negrisk]
targets = []
"#,
    );

    let result = resolve_strategy_control(ResolveStrategyControlInput {
        config: &live,
        canonical_persistence: None,
        runtime_progress: None,
    });

    assert!(matches!(
        result,
        Err(ResolveStrategyControlError::InvalidConfig(_))
    ));
}

#[test]
fn canonical_config_without_matching_canonical_persistence_fails_closed() {
    let live = live_view(
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

[strategy_control]
source = "adopted"
operator_strategy_revision = "strategy-rev-missing"
"#,
    );

    let result = resolve_strategy_control(ResolveStrategyControlInput {
        config: &live,
        canonical_persistence: Some(CanonicalPersistenceState::for_revision(
            "strategy-rev-other",
        )),
        runtime_progress: None,
    });

    assert!(matches!(
        result,
        Err(ResolveStrategyControlError::MissingCanonicalPersistence(_))
    ));
}

#[test]
fn malformed_legacy_target_source_is_invalid_config() {
    let live = live_view(
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
"#,
    );

    let result = resolve_strategy_control(ResolveStrategyControlInput {
        config: &live,
        canonical_persistence: None,
        runtime_progress: None,
    });

    assert!(matches!(
        result,
        Err(ResolveStrategyControlError::InvalidConfig(_))
    ));
}

#[test]
fn conflicting_runtime_progress_does_not_override_configured_strategy_revision() {
    let live = live_view(
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

[strategy_control]
source = "adopted"
operator_strategy_revision = "strategy-rev-12"
"#,
    );

    let resolved = resolve_strategy_control(ResolveStrategyControlInput {
        config: &live,
        canonical_persistence: Some(CanonicalPersistenceState::for_revision("strategy-rev-12")),
        runtime_progress: Some(RuntimeProgressState::with_strategy_revision(
            "strategy-rev-active",
        )),
    })
    .unwrap();

    assert_eq!(resolved.operator_strategy_revision, "strategy-rev-12");
    assert_eq!(
        resolved.active_operator_strategy_revision.as_deref(),
        Some("strategy-rev-active")
    );
    assert!(resolved.restart_required);
}

fn live_view(config: &str) -> AppLiveConfigView<'static> {
    let raw = load_raw_config_from_str(config).unwrap();
    let validated = ValidatedConfig::new(raw).unwrap();
    let leaked = Box::leak(Box::new(validated));
    leaked.for_app_live().unwrap()
}
