use domain::DecisionInput;
use state::{NegRiskFamilyRolloutReadiness, NegRiskView};

#[test]
fn negrisk_strategy_is_silent_when_projection_is_not_ready() {
    let intents = strategy_negrisk::build_intents(&sample_unready_negrisk_view());

    assert!(intents.is_empty());
}

#[test]
fn negrisk_strategy_emits_one_stable_intent_per_family_scope() {
    let intents = strategy_negrisk::build_intents(&sample_ready_negrisk_view());

    assert_eq!(intents.len(), 2);
    let DecisionInput::Strategy(first) = &intents[0] else {
        panic!("expected strategy intent");
    };
    let DecisionInput::Strategy(second) = &intents[1] else {
        panic!("expected strategy intent");
    };

    assert_eq!(first.route, "neg-risk");
    assert_eq!(first.scope, "family-a");
    assert_eq!(first.intent_id, "neg-risk:family-a:snapshot-negrisk-2:12");
    assert_eq!(second.route, "neg-risk");
    assert_eq!(second.scope, "family-b");
    assert_eq!(second.intent_id, "neg-risk:family-b:snapshot-negrisk-2:12");
}

#[test]
fn negrisk_strategy_intents_are_stable_and_deduped() {
    let first = strategy_negrisk::build_intents(&sample_ready_negrisk_view());
    let second = strategy_negrisk::build_intents(&sample_ready_negrisk_view());

    assert_eq!(first, second);
}

fn sample_unready_negrisk_view() -> NegRiskView {
    NegRiskView {
        snapshot_id: "snapshot-negrisk-1".to_owned(),
        state_version: 11,
        families: Vec::new(),
    }
}

fn sample_ready_negrisk_view() -> NegRiskView {
    NegRiskView {
        snapshot_id: "snapshot-negrisk-2".to_owned(),
        state_version: 12,
        families: vec![
            sample_family("family-b"),
            sample_family("family-a"),
            sample_family("family-b"),
        ],
    }
}

fn sample_family(family_id: &str) -> NegRiskFamilyRolloutReadiness {
    NegRiskFamilyRolloutReadiness {
        family_id: family_id.to_owned(),
        shadow_parity_ready: false,
        recovery_ready: false,
        replay_drift_ready: false,
        fault_injection_ready: false,
        conversion_path_ready: false,
        halt_semantics_ready: false,
    }
}
