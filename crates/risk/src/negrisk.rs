use domain::{DecisionVerdict, ExecutionMode};
use state::NegRiskView;

pub const ROUTE: &str = "neg-risk";

pub fn phase_one_effective_mode(mode: ExecutionMode) -> ExecutionMode {
    match mode {
        ExecutionMode::Live => ExecutionMode::Disabled,
        other => other,
    }
}

pub fn evaluate_negrisk_intent(view: &NegRiskView, mode: ExecutionMode) -> DecisionVerdict {
    if view.snapshot_id.is_empty() || view.family_ids().is_empty() {
        return DecisionVerdict::Rejected;
    }

    match mode {
        ExecutionMode::Shadow => DecisionVerdict::Approved,
        ExecutionMode::Disabled
        | ExecutionMode::Live
        | ExecutionMode::ReduceOnly
        | ExecutionMode::RecoveryOnly => DecisionVerdict::Rejected,
    }
}
