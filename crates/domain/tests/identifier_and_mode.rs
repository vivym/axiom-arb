use domain::{
    AccountTradingStatus, ConditionId, IdentifierMap, IdentifierMapError, MarketRoute, RuntimeMode,
    RuntimeOverlay, RuntimePolicy, TokenId, VenueTradingStatus,
};

#[test]
fn identifier_map_resolves_token_condition_and_route() {
    let map = IdentifierMap::new(
        [("token_yes", "condition_a"), ("token_no", "condition_a")],
        [("condition_a", MarketRoute::Standard)],
    )
    .expect("identifier map should build");

    assert_eq!(
        map.condition_for_token(&TokenId::from("token_yes")),
        Some(&ConditionId::from("condition_a"))
    );
    assert_eq!(
        map.route_for_condition(&ConditionId::from("condition_a")),
        Some(MarketRoute::Standard)
    );
}

#[test]
fn bootstrapping_defaults_to_cancel_only_until_first_reconcile() {
    let mode = RuntimeMode::Bootstrapping.default_overlay();

    assert_eq!(mode, Some(RuntimeOverlay::CancelOnly));
}

#[test]
fn identifier_map_rejects_conflicting_duplicate_token_mapping() {
    let err = IdentifierMap::new(
        [("token_yes", "condition_a"), ("token_yes", "condition_b")],
        [("condition_a", MarketRoute::Standard)],
    )
    .expect_err("conflicting token mapping should be rejected");

    assert_eq!(
        err,
        IdentifierMapError::ConflictingTokenCondition {
            token_id: TokenId::from("token_yes"),
            existing_condition_id: ConditionId::from("condition_a"),
            new_condition_id: ConditionId::from("condition_b"),
        }
    );
}

#[test]
fn identifier_map_rejects_conflicting_duplicate_route_mapping() {
    let err = IdentifierMap::new(
        [("token_yes", "condition_a")],
        [
            ("condition_a", MarketRoute::Standard),
            ("condition_a", MarketRoute::NegRisk),
        ],
    )
    .expect_err("conflicting route mapping should be rejected");

    assert_eq!(
        err,
        IdentifierMapError::ConflictingConditionRoute {
            condition_id: ConditionId::from("condition_a"),
            existing_route: MarketRoute::Standard,
            new_route: MarketRoute::NegRisk,
        }
    );
}

#[test]
fn identifier_map_returns_none_when_route_metadata_is_missing() {
    let map = IdentifierMap::new(
        [("token_yes", "condition_a")],
        Vec::<(ConditionId, MarketRoute)>::new(),
    )
    .expect("identifier map should build without routes");

    assert_eq!(
        map.route_for_condition(&ConditionId::from("condition_a")),
        None
    );
}

#[test]
fn venue_cancel_only_overlays_existing_policy() {
    let constrained = RuntimePolicy {
        mode: RuntimeMode::Reconciling,
        overlay: None,
    }
    .constrained_by(VenueTradingStatus::CancelOnly, AccountTradingStatus::Normal);

    assert_eq!(
        constrained,
        RuntimePolicy {
            mode: RuntimeMode::NoNewRisk,
            overlay: Some(RuntimeOverlay::CancelOnly),
        }
    );
}

#[test]
fn account_close_only_applies_reduce_only_overlay() {
    let constrained = RuntimePolicy {
        mode: RuntimeMode::Degraded,
        overlay: Some(RuntimeOverlay::InventoryOnly),
    }
    .constrained_by(
        VenueTradingStatus::TradingEnabled,
        AccountTradingStatus::CloseOnly,
    );

    assert_eq!(
        constrained,
        RuntimePolicy {
            mode: RuntimeMode::NoNewRisk,
            overlay: Some(RuntimeOverlay::ReduceOnly),
        }
    );
}

#[test]
fn trading_disabled_forces_global_halt() {
    let constrained = RuntimePolicy {
        mode: RuntimeMode::Healthy,
        overlay: None,
    }
    .constrained_by(
        VenueTradingStatus::TradingDisabled,
        AccountTradingStatus::Normal,
    );

    assert_eq!(
        constrained,
        RuntimePolicy {
            mode: RuntimeMode::GlobalHalt,
            overlay: None,
        }
    );
}

#[test]
fn geoblocked_or_banned_forces_global_halt() {
    for status in [
        AccountTradingStatus::Geoblocked,
        AccountTradingStatus::Banned,
    ] {
        let constrained = RuntimePolicy {
            mode: RuntimeMode::Bootstrapping,
            overlay: Some(RuntimeOverlay::CancelOnly),
        }
        .constrained_by(VenueTradingStatus::TradingEnabled, status);

        assert_eq!(
            constrained,
            RuntimePolicy {
                mode: RuntimeMode::GlobalHalt,
                overlay: None,
            }
        );
    }
}
