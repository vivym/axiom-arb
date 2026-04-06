use app_live::strategy_control::live_route_registry;

#[test]
fn live_route_registry_exposes_fullset_and_negrisk_adapters() {
    let registry = live_route_registry();

    assert!(registry.adapter("full-set").is_some());
    assert!(registry.adapter("neg-risk").is_some());
}

#[test]
fn live_route_registry_exposes_route_owned_scope_semantics() {
    let registry = live_route_registry();
    let full_set = registry
        .adapter("full-set")
        .expect("full-set adapter should be registered");
    let neg_risk = registry
        .adapter("neg-risk")
        .expect("neg-risk adapter should be registered");

    assert!(full_set.supports_scope("default"));
    assert!(!full_set.supports_scope("family-a"));

    assert!(neg_risk.supports_scope("default"));
    assert!(neg_risk.supports_scope("family-a"));
    assert!(!neg_risk.supports_scope(""));
}
