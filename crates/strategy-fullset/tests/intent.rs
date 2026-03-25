use domain::DecisionInput;
use state::FullSetView;

#[test]
fn fullset_strategy_emits_stable_default_scoped_intent_from_fullset_view() {
    let intents = strategy_fullset::build_intents(&sample_fullset_view());

    assert_eq!(intents.len(), 1);
    let DecisionInput::Strategy(intent) = &intents[0] else {
        panic!("expected strategy intent");
    };

    assert_eq!(intent.route, "full-set");
    assert_eq!(intent.scope, "default");
    assert_eq!(intent.source_snapshot_id, "snapshot-fullset-1");
    assert_eq!(intent.intent_id, "full-set:default:snapshot-fullset-1:7");
}

#[test]
fn fullset_strategy_intent_id_is_stable_for_identical_views() {
    let first = strategy_fullset::build_intents(&sample_fullset_view());
    let second = strategy_fullset::build_intents(&sample_fullset_view());

    assert_eq!(first, second);
}

fn sample_fullset_view() -> FullSetView {
    FullSetView {
        snapshot_id: "snapshot-fullset-1".to_owned(),
        state_version: 7,
        open_orders: vec!["order-1".to_owned()],
    }
}
