use app_live::strategy_control::live_route_registry;

#[test]
fn live_route_registry_exposes_fullset_and_negrisk_adapters() {
    let registry = live_route_registry();

    assert!(registry.adapter("full-set").is_some());
    assert!(registry.adapter("neg-risk").is_some());
}
