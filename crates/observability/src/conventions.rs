pub mod span_names {
    pub const APP_BOOTSTRAP: &str = "axiom.app.bootstrap";
    pub const APP_BOOTSTRAP_COMPLETE: &str = "axiom.app.bootstrap.complete";
    pub const APP_RUNTIME_RECONCILE: &str = "axiom.app_runtime.reconcile";
    pub const APP_RUNTIME_APPLY_INPUT: &str = "axiom.app_runtime.apply_input";
    pub const APP_RUNTIME_PUBLISH_SNAPSHOT: &str = "axiom.app_runtime.publish_snapshot";
    pub const APP_SUPERVISOR_RESUME: &str = "axiom.app_supervisor.resume";
    pub const APP_DISPATCH_FLUSH: &str = "axiom.app_dispatch.flush";
    pub const REPLAY_RUN: &str = "axiom.app_replay.run";
    pub const REPLAY_SUMMARY: &str = "axiom.app_replay.summary";
}

pub mod field_keys {
    pub const SERVICE_NAME: &str = "service.name";
    pub const APP_MODE: &str = "app_mode";
    pub const RUNTIME_MODE: &str = "runtime_mode";
    pub const BOOTSTRAP_STATUS: &str = "bootstrap_status";
    pub const PROCESSED_COUNT: &str = "processed_count";
    pub const LAST_JOURNAL_SEQ: &str = "last_journal_seq";
    pub const STATE_VERSION: &str = "state_version";
    pub const JOURNAL_SEQ: &str = "journal_seq";
    pub const SNAPSHOT_ID: &str = "snapshot_id";
    pub const PENDING_RECONCILE_COUNT: &str = "pending_reconcile_count";
    pub const ATTENTION_REASON: &str = "attention_reason";
    pub const BACKLOG_COUNT: &str = "backlog_count";
    pub const APPLY_RESULT: &str = "apply_result";
}

pub mod metric_dimensions {
    use crate::field_keys;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Channel {
        Market,
        User,
    }

    impl Channel {
        pub const fn as_pair(self) -> (&'static str, &'static str) {
            match self {
                Self::Market => ("channel", "market"),
                Self::User => ("channel", "user"),
            }
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum ReconcileReason {
        DuplicateSignedOrder,
        IdentifierMismatch,
        MissingRemoteOrder,
        UnexpectedRemoteOrder,
        OrderStateMismatch,
        ApprovalMismatch,
        ResolutionMismatch,
        RelayerTxMismatch,
        InventoryMismatch,
    }

    impl ReconcileReason {
        pub const fn as_pair(self) -> (&'static str, &'static str) {
            match self {
                Self::DuplicateSignedOrder => {
                    (field_keys::ATTENTION_REASON, "duplicate_signed_order")
                }
                Self::IdentifierMismatch => (field_keys::ATTENTION_REASON, "identifier_mismatch"),
                Self::MissingRemoteOrder => (field_keys::ATTENTION_REASON, "missing_remote_order"),
                Self::UnexpectedRemoteOrder => {
                    (field_keys::ATTENTION_REASON, "unexpected_remote_order")
                }
                Self::OrderStateMismatch => (field_keys::ATTENTION_REASON, "order_state_mismatch"),
                Self::ApprovalMismatch => (field_keys::ATTENTION_REASON, "approval_mismatch"),
                Self::ResolutionMismatch => (field_keys::ATTENTION_REASON, "resolution_mismatch"),
                Self::RelayerTxMismatch => (field_keys::ATTENTION_REASON, "relayer_tx_mismatch"),
                Self::InventoryMismatch => (field_keys::ATTENTION_REASON, "inventory_mismatch"),
            }
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum HaltScope {
        Global,
        Family,
        Market,
        Strategy,
    }

    impl HaltScope {
        pub const fn as_pair(self) -> (&'static str, &'static str) {
            match self {
                Self::Global => ("scope", "global"),
                Self::Family => ("scope", "family"),
                Self::Market => ("scope", "market"),
                Self::Strategy => ("scope", "strategy"),
            }
        }
    }
}
