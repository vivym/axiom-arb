use domain::DecisionInput;
use state::FullSetView;

#[test]
fn fullset_strategy_emits_intent_from_fullset_view() {
    let intents = strategy_fullset::build_intents(&sample_fullset_view());

    assert_eq!(intents.len(), 1);
    assert!(matches!(intents[0], DecisionInput::Strategy(_)));
}

fn sample_fullset_view() -> FullSetView {
    FullSetView {
        snapshot_id: "snapshot-fullset-1".to_owned(),
        state_version: 7,
        open_orders: vec!["order-1".to_owned()],
    }
}
