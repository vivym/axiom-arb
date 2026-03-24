use domain::{DecisionInput, IntentCandidate};
use state::FullSetView;

pub fn build_intents(view: &FullSetView) -> Vec<DecisionInput> {
    vec![DecisionInput::Strategy(IntentCandidate::new(
        "fullset-intent-1",
        &view.snapshot_id,
        "full-set",
    ))]
}
