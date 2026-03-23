use domain::{IdentifierMap, MarketRoute, RuntimeMode, RuntimeOverlay};

#[test]
fn identifier_map_resolves_token_condition_and_route() {
    let map = IdentifierMap::new(
        [("token_yes", "condition_a"), ("token_no", "condition_a")],
        [("condition_a", MarketRoute::Standard)],
    );

    assert_eq!(map.condition_for_token("token_yes").unwrap(), "condition_a");
    assert_eq!(
        map.route_for_condition("condition_a"),
        MarketRoute::Standard
    );
}

#[test]
fn bootstrapping_defaults_to_cancel_only_until_first_reconcile() {
    let mode = RuntimeMode::Bootstrapping.default_overlay();

    assert_eq!(mode, Some(RuntimeOverlay::CancelOnly));
}
