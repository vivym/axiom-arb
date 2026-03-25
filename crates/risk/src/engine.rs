use domain::{ActivationDecision, DecisionInput, DecisionVerdict, ExecutionMode};

pub fn evaluate_decision(
    input: &DecisionInput,
    activation: &ActivationDecision,
) -> DecisionVerdict {
    if matches!(activation.mode, ExecutionMode::Disabled) {
        return DecisionVerdict::Rejected;
    }

    if let DecisionInput::Strategy(_intent) = input {
        if matches!(
            activation.mode,
            ExecutionMode::ReduceOnly | ExecutionMode::RecoveryOnly
        ) {
            return DecisionVerdict::Rejected;
        }
    }

    DecisionVerdict::Approved
}
