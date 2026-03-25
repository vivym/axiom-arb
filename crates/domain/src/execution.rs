#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishedSnapshotRef {
    pub snapshot_id: String,
    pub state_version: u64,
    pub committed_journal_seq: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionRequest {
    pub request_id: String,
    pub decision_input_id: String,
    pub snapshot_id: String,
    pub route: String,
    pub scope: String,
    pub activation_mode: crate::ExecutionMode,
    pub matched_rule_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionPlanRef {
    pub plan_id: String,
    pub request_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionAttemptOutcome {
    Succeeded,
    ShadowRecorded,
    FailedDefinitive,
    FailedAmbiguous,
    RetryExhausted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionAttemptContext {
    pub attempt_id: String,
    pub snapshot_id: String,
    pub execution_mode: crate::ExecutionMode,
    pub route: String,
    pub scope: String,
    pub matched_rule_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionReceipt {
    pub attempt_id: String,
    pub outcome: ExecutionAttemptOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionAttempt {
    pub attempt_id: String,
    pub plan_id: String,
    pub snapshot_id: String,
    pub attempt_no: u32,
}

impl ExecutionAttempt {
    pub fn new(
        attempt_id: impl Into<String>,
        plan_id: impl Into<String>,
        snapshot_id: impl Into<String>,
        attempt_no: u32,
    ) -> Self {
        Self {
            attempt_id: attempt_id.into(),
            plan_id: plan_id.into(),
            snapshot_id: snapshot_id.into(),
            attempt_no,
        }
    }
}
