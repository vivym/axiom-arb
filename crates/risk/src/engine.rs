use domain::{ActivationDecision, DecisionInput, DecisionVerdict, ExecutionMode};

pub fn evaluate_decision(
    input: &DecisionInput,
    activation: &ActivationDecision,
) -> DecisionVerdict {
    if matches!(activation.mode, ExecutionMode::Disabled) {
        return DecisionVerdict::Rejected;
    }

    if matches!(activation.mode, ExecutionMode::RecoveryOnly)
        && matches!(input, DecisionInput::Strategy(_))
    {
        return DecisionVerdict::Rejected;
    }

    DecisionVerdict::Approved
}
