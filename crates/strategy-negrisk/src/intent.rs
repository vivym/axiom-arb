use domain::{DecisionInput, IntentCandidate};
use state::NegRiskView;

pub fn build_intents(view: &NegRiskView) -> Vec<DecisionInput> {
    if view.family_ids.is_empty() {
        return Vec::new();
    }

    vec![DecisionInput::Strategy(IntentCandidate::new(
        "negrisk-intent-1",
        &view.snapshot_id,
        "neg-risk",
    ))]
}
