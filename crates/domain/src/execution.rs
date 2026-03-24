#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishedSnapshotRef {
    pub snapshot_id: String,
    pub state_version: u64,
    pub committed_journal_seq: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionRequest;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionReceipt;

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
