use domain::{DecisionVerdict, ExecutionMode};
use state::{NegRiskFamilyRolloutReadiness, NegRiskView};

pub const ROUTE: &str = "neg-risk";

pub fn phase_one_effective_mode(mode: ExecutionMode) -> ExecutionMode {
    match mode {
        ExecutionMode::Live => ExecutionMode::Disabled,
        other => other,
    }
}

pub fn evaluate_negrisk_intent(view: &NegRiskView, mode: ExecutionMode) -> DecisionVerdict {
    let Some(family_id) = view.family_ids().into_iter().next() else {
        return DecisionVerdict::Rejected;
    };

    evaluate_negrisk_family(view, &family_id, mode)
}

pub fn evaluate_negrisk_family(
    view: &NegRiskView,
    family_id: &str,
    mode: ExecutionMode,
) -> DecisionVerdict {
    if view.snapshot_id.is_empty() {
        return DecisionVerdict::Rejected;
    }

    let Some(family) = view
        .families
        .iter()
        .find(|family| family.family_id == family_id)
    else {
        return DecisionVerdict::Rejected;
    };

    match mode {
        ExecutionMode::Shadow => DecisionVerdict::Approved,
        ExecutionMode::Live if live_ready(family) => DecisionVerdict::Approved,
        ExecutionMode::Disabled
        | ExecutionMode::Live
        | ExecutionMode::ReduceOnly
        | ExecutionMode::RecoveryOnly => DecisionVerdict::Rejected,
    }
}

fn live_ready(family: &NegRiskFamilyRolloutReadiness) -> bool {
    family.shadow_parity_ready
        && family.recovery_ready
        && family.replay_drift_ready
        && family.fault_injection_ready
        && family.conversion_path_ready
        && family.halt_semantics_ready
}
