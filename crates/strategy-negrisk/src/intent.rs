use domain::{DecisionInput, IntentCandidate};
use state::NegRiskView;

pub fn build_intents(view: &NegRiskView) -> Vec<DecisionInput> {
    let mut family_ids = view.family_ids();
    if family_ids.is_empty() {
        return Vec::new();
    }

    family_ids.sort();
    family_ids.dedup();

    family_ids
        .into_iter()
        .map(|family_id| {
            DecisionInput::Strategy(IntentCandidate::new(
                stable_intent_id(
                    "neg-risk",
                    &family_id,
                    &view.snapshot_id,
                    view.state_version,
                ),
                &view.snapshot_id,
                family_id,
            ))
        })
        .collect()
}

fn stable_intent_id(route: &str, scope: &str, snapshot_id: &str, state_version: u64) -> String {
    format!("{route}:{scope}:{snapshot_id}:{state_version}")
}
