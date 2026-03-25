use observability::{field_keys, metric_dimensions, span_names};

#[test]
fn venue_producer_conventions_define_ws_and_heartbeat_fields() {
    assert_eq!(span_names::VENUE_WS_SESSION, "axiom.venue.ws.session");
    assert_eq!(span_names::VENUE_HEARTBEAT, "axiom.venue.heartbeat");

    assert_eq!(field_keys::CHANNEL, "channel");
    assert_eq!(field_keys::CONNECTION_ID, "connection_id");
    assert_eq!(field_keys::SESSION_STATUS, "session_status");
    assert_eq!(field_keys::HEARTBEAT_ID, "heartbeat_id");
    assert_eq!(field_keys::HEARTBEAT_STATUS, "heartbeat_status");
}

#[test]
fn observability_conventions_define_stable_span_names_and_field_keys() {
    assert_eq!(span_names::APP_BOOTSTRAP, "axiom.app.bootstrap");
    assert_eq!(span_names::REPLAY_RUN, "axiom.app_replay.run");
    assert_eq!(field_keys::SERVICE_NAME, "service.name");
    assert_eq!(field_keys::RUNTIME_MODE, "runtime_mode");
}

#[test]
fn metric_dimension_vocabularies_are_repo_owned_and_finite() {
    assert_eq!(
        metric_dimensions::Channel::User.as_pair(),
        ("channel", "user")
    );
    assert_eq!(
        metric_dimensions::HaltScope::Family.as_pair(),
        ("scope", "family")
    );
}

#[test]
fn runtime_observability_conventions_define_runtime_spans_fields_and_reconcile_reasons() {
    assert_eq!(
        span_names::APP_RUNTIME_RECONCILE,
        "axiom.app.runtime.reconcile"
    );
    assert_eq!(
        span_names::APP_RUNTIME_APPLY_INPUT,
        "axiom.app.runtime.apply_input"
    );
    assert_eq!(
        span_names::APP_RUNTIME_PUBLISH_SNAPSHOT,
        "axiom.app.runtime.publish_snapshot"
    );
    assert_eq!(
        span_names::APP_SUPERVISOR_RESUME,
        "axiom.app.supervisor.resume"
    );
    assert_eq!(span_names::APP_DISPATCH_FLUSH, "axiom.app.dispatch.flush");

    assert_eq!(field_keys::STATE_VERSION, "state_version");
    assert_eq!(field_keys::JOURNAL_SEQ, "journal_seq");
    assert_eq!(field_keys::SNAPSHOT_ID, "snapshot_id");
    assert_eq!(
        field_keys::PENDING_RECONCILE_COUNT,
        "pending_reconcile_count"
    );
    assert_eq!(field_keys::ATTENTION_REASON, "attention_reason");
    assert_eq!(field_keys::BACKLOG_COUNT, "backlog_count");
    assert_eq!(field_keys::APPLY_RESULT, "apply_result");

    assert_eq!(
        metric_dimensions::ReconcileReason::DuplicateSignedOrder.as_pair(),
        ("attention_reason", "duplicate_signed_order")
    );
    assert_eq!(
        metric_dimensions::ReconcileReason::IdentifierMismatch.as_pair(),
        ("attention_reason", "identifier_mismatch")
    );
    assert_eq!(
        metric_dimensions::ReconcileReason::MissingRemoteOrder.as_pair(),
        ("attention_reason", "missing_remote_order")
    );
    assert_eq!(
        metric_dimensions::ReconcileReason::UnexpectedRemoteOrder.as_pair(),
        ("attention_reason", "unexpected_remote_order")
    );
    assert_eq!(
        metric_dimensions::ReconcileReason::OrderStateMismatch.as_pair(),
        ("attention_reason", "order_state_mismatch")
    );
    assert_eq!(
        metric_dimensions::ReconcileReason::ApprovalMismatch.as_pair(),
        ("attention_reason", "approval_mismatch")
    );
    assert_eq!(
        metric_dimensions::ReconcileReason::ResolutionMismatch.as_pair(),
        ("attention_reason", "resolution_mismatch")
    );
    assert_eq!(
        metric_dimensions::ReconcileReason::RelayerTxMismatch.as_pair(),
        ("attention_reason", "relayer_tx_mismatch")
    );
    assert_eq!(
        metric_dimensions::ReconcileReason::InventoryMismatch.as_pair(),
        ("attention_reason", "inventory_mismatch")
    );
}
