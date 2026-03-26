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
    pub submission_ref: Option<String>,
    pub pending_ref: Option<String>,
}

impl ExecutionReceipt {
    pub fn new(attempt_id: impl Into<String>, outcome: ExecutionAttemptOutcome) -> Self {
        Self {
            attempt_id: attempt_id.into(),
            outcome,
            submission_ref: None,
            pending_ref: None,
        }
    }

    pub fn with_submission_ref(mut self, submission_ref: impl Into<String>) -> Self {
        self.submission_ref = Some(submission_ref.into());
        self
    }

    pub fn with_pending_ref(mut self, pending_ref: impl Into<String>) -> Self {
        self.pending_ref = Some(pending_ref.into());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveSubmissionRecord {
    pub submission_ref: String,
    pub attempt_id: String,
    pub route: String,
    pub scope: String,
    pub provider: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LiveSubmitOutcome {
    Accepted {
        submission_record: LiveSubmissionRecord,
    },
    RejectedDefinitive {
        reason: String,
    },
    AcceptedButUnconfirmed {
        submission_record: Option<LiveSubmissionRecord>,
        pending_ref: String,
    },
    Ambiguous {
        pending_ref: String,
        reason: String,
    },
}

impl LiveSubmitOutcome {
    pub fn is_accepted(&self) -> bool {
        matches!(
            self,
            Self::Accepted { .. } | Self::AcceptedButUnconfirmed { .. }
        )
    }

    pub fn is_ambiguous(&self) -> bool {
        matches!(self, Self::Ambiguous { .. })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingReconcileWork {
    pub pending_ref: String,
    pub route: String,
    pub scope: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReconcileOutcome {
    ConfirmedAuthoritative { submission_ref: String },
    StillPending,
    NeedsRecovery { pending_ref: String, reason: String },
    FailedAmbiguous { pending_ref: String, reason: String },
    FailedDefinitive { reason: String },
}

impl ReconcileOutcome {
    pub fn is_confirmed(&self) -> bool {
        matches!(self, Self::ConfirmedAuthoritative { .. })
    }

    pub fn is_pending(&self) -> bool {
        matches!(self, Self::StillPending)
    }

    pub fn needs_recovery(&self) -> bool {
        matches!(self, Self::NeedsRecovery { .. })
    }
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
