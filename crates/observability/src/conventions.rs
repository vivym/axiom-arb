pub mod span_names {
    pub const APP_BOOTSTRAP: &str = "axiom.app.bootstrap";
    pub const APP_BOOTSTRAP_COMPLETE: &str = "axiom.app.bootstrap.complete";
    pub const APP_DAEMON_RUN: &str = "axiom.app.daemon.run";
    pub const APP_RUNTIME_RECONCILE: &str = "axiom.app.runtime.reconcile";
    pub const APP_RUNTIME_APPLY_INPUT: &str = "axiom.app.runtime.apply_input";
    pub const APP_RUNTIME_PUBLISH_SNAPSHOT: &str = "axiom.app.runtime.publish_snapshot";
    pub const APP_SUPERVISOR_RESUME: &str = "axiom.app.supervisor.resume";
    pub const APP_DISPATCH_FLUSH: &str = "axiom.app.dispatch.flush";
    pub const EXECUTION_ATTEMPT: &str = "axiom.execution.attempt";
    pub const APP_RECOVERY_DIVERGENCE: &str = "axiom.app.recovery.divergence";
    pub const REPLAY_RUN: &str = "axiom.app_replay.run";
    pub const REPLAY_SUMMARY: &str = "axiom.app_replay.summary";
    pub const VENUE_WS_SESSION: &str = "axiom.venue.ws.session";
    pub const VENUE_HEARTBEAT: &str = "axiom.venue.heartbeat";
    pub const VENUE_RELAYER_POLL: &str = "axiom.venue.relayer.poll";
}

pub mod field_keys {
    pub const SERVICE_NAME: &str = "service.name";
    pub const APP_MODE: &str = "app_mode";
    pub const RUNTIME_MODE: &str = "runtime_mode";
    pub const BOOTSTRAP_STATUS: &str = "bootstrap_status";
    pub const EXECUTION_MODE: &str = "execution_mode";
    pub const ROUTE: &str = "route";
    pub const SCOPE: &str = "scope";
    pub const PLAN_ID: &str = "plan_id";
    pub const ATTEMPT_ID: &str = "attempt_id";
    pub const ATTEMPT_NO: &str = "attempt_no";
    pub const ATTEMPT_OUTCOME: &str = "attempt_outcome";
    pub const SINK_KIND: &str = "sink_kind";
    pub const DIVERGENCE_KIND: &str = "divergence_kind";
    pub const RELAYER_TX_COUNT: &str = "relayer_tx_count";
    pub const PENDING_TX_COUNT: &str = "pending_tx_count";
    pub const PENDING_AGE_SECONDS: &str = "pending_age_seconds";
    pub const PROCESSED_COUNT: &str = "processed_count";
    pub const LAST_JOURNAL_SEQ: &str = "last_journal_seq";
    pub const COMMITTED_JOURNAL_SEQ: &str = "committed_journal_seq";
    pub const STATE_VERSION: &str = "state_version";
    pub const JOURNAL_SEQ: &str = "journal_seq";
    pub const SNAPSHOT_ID: &str = "snapshot_id";
    pub const PENDING_RECONCILE_COUNT: &str = "pending_reconcile_count";
    pub const GLOBAL_POSTURE: &str = "global_posture";
    pub const INGRESS_BACKLOG: &str = "ingress_backlog";
    pub const FOLLOW_UP_BACKLOG: &str = "follow_up_backlog";
    pub const OPERATOR_TARGET_REVISION: &str = "operator_target_revision";
    pub const ATTENTION_REASON: &str = "attention_reason";
    pub const BACKLOG_COUNT: &str = "backlog_count";
    pub const APPLY_RESULT: &str = "apply_result";
    pub const CHANNEL: &str = "channel";
    pub const CONNECTION_ID: &str = "connection_id";
    pub const SESSION_STATUS: &str = "session_status";
    pub const HEARTBEAT_ID: &str = "heartbeat_id";
    pub const HEARTBEAT_STATUS: &str = "heartbeat_status";
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
                Self::Market => (field_keys::CHANNEL, "market"),
                Self::User => (field_keys::CHANNEL, "user"),
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
                Self::Global => (field_keys::SCOPE, "global"),
                Self::Family => (field_keys::SCOPE, "family"),
                Self::Market => (field_keys::SCOPE, "market"),
                Self::Strategy => (field_keys::SCOPE, "strategy"),
            }
        }
    }
}
