use domain::{DecisionInput, IntentCandidate};
use state::FullSetView;

pub fn build_intents(view: &FullSetView) -> Vec<DecisionInput> {
    let scope = "default";

    vec![DecisionInput::Strategy(IntentCandidate::new(
        stable_intent_id("full-set", scope, &view.snapshot_id, view.state_version),
        &view.snapshot_id,
        "full-set",
        scope,
    ))]
}

fn stable_intent_id(route: &str, scope: &str, snapshot_id: &str, state_version: u64) -> String {
    format!("{route}:{scope}:{snapshot_id}:{state_version}")
}
