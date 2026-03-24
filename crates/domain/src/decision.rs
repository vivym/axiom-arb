#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntentCandidate {
    pub intent_id: String,
    pub source_snapshot_id: String,
    pub scope: String,
}

impl IntentCandidate {
    pub fn new(
        intent_id: impl Into<String>,
        source_snapshot_id: impl Into<String>,
        scope: impl Into<String>,
    ) -> Self {
        Self {
            intent_id: intent_id.into(),
            source_snapshot_id: source_snapshot_id.into(),
            scope: scope.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryIntent {
    pub recovery_id: String,
    pub source_snapshot_id: String,
    pub scope: String,
}

impl RecoveryIntent {
    pub fn new(
        recovery_id: impl Into<String>,
        source_snapshot_id: impl Into<String>,
        scope: impl Into<String>,
    ) -> Self {
        Self {
            recovery_id: recovery_id.into(),
            source_snapshot_id: source_snapshot_id.into(),
            scope: scope.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecisionInput {
    Strategy(IntentCandidate),
    Recovery(RecoveryIntent),
}

impl DecisionInput {
    pub fn decision_input_id(&self) -> &str {
        match self {
            Self::Strategy(intent) => &intent.intent_id,
            Self::Recovery(intent) => &intent.recovery_id,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionMode {
    Disabled,
    Shadow,
    Live,
    ReduceOnly,
    RecoveryOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecisionVerdict {
    Approved,
    Rejected,
    Deferred,
    ReconcileRequired,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivationDecision {
    pub mode: ExecutionMode,
    pub scope: String,
    pub reason: String,
    pub policy_version: String,
    pub matched_rule_id: Option<String>,
}

impl ActivationDecision {
    pub fn new(
        mode: ExecutionMode,
        scope: impl Into<String>,
        reason: impl Into<String>,
        policy_version: impl Into<String>,
        matched_rule_id: Option<impl Into<String>>,
    ) -> Self {
        Self {
            mode,
            scope: scope.into(),
            reason: reason.into(),
            policy_version: policy_version.into(),
            matched_rule_id: matched_rule_id.map(Into::into),
        }
    }

    pub fn shadow(
        scope: impl Into<String>,
        policy_version: impl Into<String>,
        matched_rule_id: Option<impl Into<String>>,
    ) -> Self {
        Self::new(
            ExecutionMode::Shadow,
            scope,
            "",
            policy_version,
            matched_rule_id,
        )
    }
}
