use state::NegRiskView;

#[test]
fn negrisk_strategy_is_silent_when_projection_is_not_ready() {
    let intents = strategy_negrisk::build_intents(&sample_unready_negrisk_view());

    assert!(intents.is_empty());
}

fn sample_unready_negrisk_view() -> NegRiskView {
    NegRiskView {
        snapshot_id: "snapshot-negrisk-1".to_owned(),
        state_version: 11,
        family_ids: Vec::new(),
    }
}
