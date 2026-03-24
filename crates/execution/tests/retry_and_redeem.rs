use domain::{ConditionId, OrderId, SignedOrderIdentity};
use execution::{
    attempt::ExecutionAttemptFactory,
    ctf::{CtfOperation, CtfOperationKind, CtfOperationStatus, CtfTracker, CtfTrackerError},
    orders::{BusinessRetryError, RetryKind, SignedOrderEnvelope},
    plans::ExecutionPlan,
};

#[test]
fn transport_retry_reuses_the_same_signed_order_identity() {
    let order = SignedOrderEnvelope::new(OrderId::from("order-1"), sample_identity("shared"));

    let retry = order.transport_retry();

    assert_eq!(retry.retry_kind, RetryKind::Transport);
    assert_eq!(retry.order_id, order.order_id);
    assert_eq!(retry.identity, order.identity);
    assert_eq!(retry.retry_of_order_id, None);
}

#[test]
fn order_envelope_carries_the_attempt_context_used_to_create_it() {
    let plan = ExecutionPlan::RedeemResolved {
        condition_id: ConditionId::from("condition-12"),
    };
    let mut factory = ExecutionAttemptFactory::default();
    let request = domain::ExecutionRequest {
        request_id: "request-12".to_owned(),
        decision_input_id: "decision-12".to_owned(),
        snapshot_id: "snapshot-12".to_owned(),
    };
    let (attempt, context) = factory.next_for_plan(&plan, &request, domain::ExecutionMode::Shadow);

    let order = SignedOrderEnvelope::new(OrderId::from("order-12"), sample_identity("attempt"))
        .with_attempt_context(&context);

    assert_eq!(order.attempt_id(), Some(attempt.attempt_id.as_str()));
    assert_eq!(
        order.transport_retry().attempt_id(),
        Some(attempt.attempt_id.as_str())
    );
}

#[test]
fn business_retry_generates_new_identity_and_links_retry_of_order_id() {
    let order = SignedOrderEnvelope::new(OrderId::from("order-1"), sample_identity("original"));

    let retry = order
        .business_retry(
            OrderId::from("order-2"),
            "nonce-replacement".to_owned(),
            sample_identity("replacement"),
        )
        .expect("business retry should require a fresh nonce and identity");

    assert_eq!(retry.retry_kind, RetryKind::Business);
    assert_eq!(retry.order_id, OrderId::from("order-2"));
    assert_ne!(retry.identity, order.identity);
    assert_eq!(retry.identity.nonce, "nonce-replacement");
    assert_eq!(retry.retry_of_order_id, Some(OrderId::from("order-1")));
}

#[test]
fn business_retry_rejects_reusing_the_same_signed_order_identity() {
    let order = SignedOrderEnvelope::new(OrderId::from("order-1"), sample_identity("original"));

    let err = order
        .business_retry(
            OrderId::from("order-2"),
            "nonce-original".to_owned(),
            sample_identity("original"),
        )
        .expect_err("business retry must not reuse the same signed-order identity");

    assert_eq!(err, BusinessRetryError::NonceUnchanged);
}

#[test]
fn business_retry_rejects_reusing_original_nonce_even_if_other_fields_change() {
    let order = SignedOrderEnvelope::new(OrderId::from("order-1"), sample_identity("original"));

    let err = order
        .business_retry(
            OrderId::from("order-2"),
            "nonce-original".to_owned(),
            SignedOrderIdentity {
                signed_order_hash: "hash-repriced".to_owned(),
                salt: "salt-repriced".to_owned(),
                nonce: "nonce-original".to_owned(),
                signature: "signature-repriced".to_owned(),
            },
        )
        .expect_err("business retry must require a new nonce");

    assert_eq!(err, BusinessRetryError::NonceUnchanged);
}

#[test]
fn business_retry_of_business_retry_keeps_lineage_pointing_to_the_original_order() {
    let original = SignedOrderEnvelope::new(OrderId::from("order-1"), sample_identity("original"));
    let first_retry = original
        .business_retry(
            OrderId::from("order-2"),
            "nonce-retry-1".to_owned(),
            sample_identity("retry-1"),
        )
        .expect("first retry should succeed");

    let second_retry = first_retry
        .business_retry(
            OrderId::from("order-3"),
            "nonce-retry-2".to_owned(),
            sample_identity("retry-2"),
        )
        .expect("second retry should succeed");

    assert_eq!(
        first_retry.retry_of_order_id,
        Some(OrderId::from("order-1"))
    );
    assert_eq!(
        second_retry.retry_of_order_id,
        Some(OrderId::from("order-1"))
    );
}

#[test]
fn business_retry_rejects_reusing_the_original_order_id_across_retry_lineage() {
    let original = SignedOrderEnvelope::new(OrderId::from("order-1"), sample_identity("original"));
    let first_retry = original
        .business_retry(
            OrderId::from("order-2"),
            "nonce-retry-1".to_owned(),
            sample_identity("retry-1"),
        )
        .expect("first retry should succeed");

    let err = first_retry
        .business_retry(
            OrderId::from("order-1"),
            "nonce-retry-2".to_owned(),
            sample_identity("retry-2"),
        )
        .expect_err("business retry must not reuse the lineage root order id");

    assert_eq!(err, BusinessRetryError::OrderIdReused);
}

#[test]
fn business_retry_rejects_reusing_an_intermediate_retry_order_id() {
    let original = SignedOrderEnvelope::new(OrderId::from("order-1"), sample_identity("original"));
    let first_retry = original
        .business_retry(
            OrderId::from("order-2"),
            "nonce-retry-1".to_owned(),
            sample_identity("retry-1"),
        )
        .expect("first retry should succeed");
    let second_retry = first_retry
        .business_retry(
            OrderId::from("order-3"),
            "nonce-retry-2".to_owned(),
            sample_identity("retry-2"),
        )
        .expect("second retry should succeed");

    let err = second_retry
        .business_retry(
            OrderId::from("order-2"),
            "nonce-retry-3".to_owned(),
            sample_identity("retry-3"),
        )
        .expect_err("business retry must not reuse any order id already used in the lineage");

    assert_eq!(err, BusinessRetryError::OrderIdReused);
}

#[test]
fn redeem_resolved_is_condition_scoped_and_amountless() {
    let condition_id = ConditionId::from("condition-1");
    let plan = ExecutionPlan::RedeemResolved {
        condition_id: condition_id.clone(),
    };

    assert_eq!(plan.condition_id(), Some(&condition_id));
    assert!(plan.is_amountless());
    assert!(matches!(
        plan,
        ExecutionPlan::RedeemResolved { condition_id: plan_condition_id }
            if plan_condition_id == condition_id
    ));
}

#[test]
fn execution_attempt_factory_binds_attempt_identity_to_the_plan_and_context() {
    let plan = ExecutionPlan::RedeemResolved {
        condition_id: ConditionId::from("condition-4"),
    };
    let mut factory = ExecutionAttemptFactory::default();
    let request = domain::ExecutionRequest {
        request_id: "request-4".to_owned(),
        decision_input_id: "decision-4".to_owned(),
        snapshot_id: "snapshot-4".to_owned(),
    };

    let (attempt, context) = factory.next_for_plan(&plan, &request, domain::ExecutionMode::Shadow);

    assert_eq!(
        attempt.plan_id,
        format!("{}:{}", request.request_id, plan.plan_id())
    );
    assert_eq!(attempt.snapshot_id, "snapshot-4");
    assert_eq!(context.attempt_id, attempt.attempt_id);
    assert_eq!(context.execution_mode, domain::ExecutionMode::Shadow);
}

#[test]
fn execution_attempt_factory_resets_attempt_numbers_per_request_bound_plan_identity() {
    let mut factory = ExecutionAttemptFactory::default();
    let request_a = domain::ExecutionRequest {
        request_id: "request-a".to_owned(),
        decision_input_id: "decision-a".to_owned(),
        snapshot_id: "snapshot-a".to_owned(),
    };
    let request_b = domain::ExecutionRequest {
        request_id: "request-b".to_owned(),
        decision_input_id: "decision-b".to_owned(),
        snapshot_id: "snapshot-b".to_owned(),
    };
    let plan_a = ExecutionPlan::RedeemResolved {
        condition_id: ConditionId::from("condition-a"),
    };
    let plan_b = ExecutionPlan::CancelStale {
        order_id: OrderId::from("order-b"),
    };

    let (attempt_a1, _) = factory.next_for_plan(&plan_a, &request_a, domain::ExecutionMode::Live);
    let (attempt_a2, _) = factory.next_for_plan(&plan_a, &request_a, domain::ExecutionMode::Live);
    let (attempt_request_b1, _) =
        factory.next_for_plan(&plan_a, &request_b, domain::ExecutionMode::Live);
    let (attempt_plan_b1, _) =
        factory.next_for_plan(&plan_b, &request_b, domain::ExecutionMode::Live);

    assert_eq!(attempt_a1.attempt_no, 1);
    assert_eq!(attempt_a2.attempt_no, 2);
    assert_eq!(attempt_request_b1.attempt_no, 1);
    assert_eq!(attempt_plan_b1.attempt_no, 1);
}

#[test]
fn execution_attempt_factory_keeps_same_business_plan_independent_across_requests() {
    let mut factory = ExecutionAttemptFactory::default();
    let plan = ExecutionPlan::RedeemResolved {
        condition_id: ConditionId::from("condition-cross-request"),
    };
    let request_a = domain::ExecutionRequest {
        request_id: "request-cross-a".to_owned(),
        decision_input_id: "decision-cross-a".to_owned(),
        snapshot_id: "snapshot-cross-a".to_owned(),
    };
    let request_b = domain::ExecutionRequest {
        request_id: "request-cross-b".to_owned(),
        decision_input_id: "decision-cross-b".to_owned(),
        snapshot_id: "snapshot-cross-b".to_owned(),
    };

    let (attempt_a, _) = factory.next_for_plan(&plan, &request_a, domain::ExecutionMode::Live);
    let (attempt_b, _) = factory.next_for_plan(&plan, &request_b, domain::ExecutionMode::Live);

    assert_eq!(attempt_a.attempt_no, 1);
    assert_eq!(attempt_b.attempt_no, 1);
    assert_ne!(attempt_a.plan_id, attempt_b.plan_id);
    assert_ne!(attempt_a.attempt_id, attempt_b.attempt_id);
}

#[test]
fn ctf_tracker_preserves_relayer_nonce_and_status_semantics() {
    let condition_id = ConditionId::from("condition-1");
    let mut tracker = CtfTracker::new();

    let operation_id = tracker
        .record(CtfOperation::new(
            CtfOperationKind::Redeem,
            condition_id.clone(),
            Some("relayer-tx-1".to_owned()),
            Some("7".to_owned()),
            CtfOperationStatus::Submitted,
        ))
        .expect("submitted operation with relayer metadata should be valid");
    tracker
        .update_status(
            operation_id,
            CtfOperationStatus::Confirmed,
            Some("0xabc123".to_owned()),
        )
        .expect("confirmed operation with tx hash should be valid");

    let tracked = tracker
        .operation(operation_id)
        .expect("ctf operation should be tracked");

    assert_eq!(tracked.kind, CtfOperationKind::Redeem);
    assert_eq!(tracked.condition_id, condition_id);
    assert_eq!(
        tracked.relayer_transaction_id.as_deref(),
        Some("relayer-tx-1")
    );
    assert_eq!(tracked.nonce.as_deref(), Some("7"));
    assert_eq!(tracked.tx_hash.as_deref(), Some("0xabc123"));
    assert_eq!(tracked.status, CtfOperationStatus::Confirmed);
}

#[test]
fn ctf_operation_carries_the_attempt_context_used_to_create_it() {
    let plan = ExecutionPlan::RedeemResolved {
        condition_id: ConditionId::from("condition-13"),
    };
    let mut factory = ExecutionAttemptFactory::default();
    let request = domain::ExecutionRequest {
        request_id: "request-13".to_owned(),
        decision_input_id: "decision-13".to_owned(),
        snapshot_id: "snapshot-13".to_owned(),
    };
    let (attempt, context) = factory.next_for_plan(&plan, &request, domain::ExecutionMode::Live);

    let operation = CtfOperation::new(
        CtfOperationKind::Merge,
        ConditionId::from("condition-13"),
        None,
        None,
        CtfOperationStatus::Planned,
    )
    .with_attempt_context(&context);

    assert_eq!(operation.attempt_id(), Some(attempt.attempt_id.as_str()));
}

#[test]
fn planned_ctf_operation_can_omit_relayer_metadata_until_later_update() {
    let condition_id = ConditionId::from("condition-2");
    let mut tracker = CtfTracker::new();

    let operation_id = tracker
        .record(CtfOperation::new(
            CtfOperationKind::Split,
            condition_id.clone(),
            None,
            None,
            CtfOperationStatus::Planned,
        ))
        .expect("planned operation without relayer metadata should be valid");

    let planned = tracker
        .operation(operation_id)
        .expect("planned operation should be tracked");
    assert_eq!(planned.relayer_transaction_id, None);
    assert_eq!(planned.nonce, None);
    assert_eq!(planned.status, CtfOperationStatus::Planned);

    tracker
        .attach_relayer_metadata(
            operation_id,
            Some("relayer-tx-2".to_owned()),
            Some("nonce-22".to_owned()),
        )
        .expect("attaching relayer metadata should succeed");
    tracker
        .update_status(operation_id, CtfOperationStatus::Submitted, None)
        .expect("submitted operation with relayer metadata should be valid");

    let submitted = tracker
        .operation(operation_id)
        .expect("submitted operation should be tracked");
    assert_eq!(
        submitted.relayer_transaction_id.as_deref(),
        Some("relayer-tx-2")
    );
    assert_eq!(submitted.nonce.as_deref(), Some("nonce-22"));
    assert_eq!(submitted.status, CtfOperationStatus::Submitted);
}

#[test]
fn submitted_operation_without_relayer_metadata_is_rejected() {
    let mut tracker = CtfTracker::new();

    let err = tracker
        .record(CtfOperation::new(
            CtfOperationKind::Split,
            ConditionId::from("condition-3"),
            None,
            None,
            CtfOperationStatus::Submitted,
        ))
        .expect_err("submitted operation must already have relayer metadata");

    assert_eq!(err, CtfTrackerError::SubmittedRequiresRelayerMetadata);
}

#[test]
fn confirmed_operation_without_tx_hash_is_rejected() {
    let mut tracker = CtfTracker::new();
    let operation_id = tracker
        .record(CtfOperation::new(
            CtfOperationKind::Redeem,
            ConditionId::from("condition-4"),
            Some("relayer-tx-4".to_owned()),
            Some("nonce-4".to_owned()),
            CtfOperationStatus::Submitted,
        ))
        .expect("submitted operation should be valid");

    let err = tracker
        .update_status(operation_id, CtfOperationStatus::Confirmed, None)
        .expect_err("confirmed operation must have a tx hash");

    assert_eq!(err, CtfTrackerError::ConfirmedRequiresTxHash);
}

#[test]
fn ctf_tracker_rejects_transitioning_back_to_planned() {
    let mut tracker = CtfTracker::new();
    let operation_id = tracker
        .record(CtfOperation::new(
            CtfOperationKind::Merge,
            ConditionId::from("condition-5"),
            Some("relayer-tx-5".to_owned()),
            Some("nonce-5".to_owned()),
            CtfOperationStatus::Submitted,
        ))
        .expect("submitted operation should be valid");

    let err = tracker
        .update_status(operation_id, CtfOperationStatus::Planned, None)
        .expect_err("cannot move a submitted operation back to planned");

    assert_eq!(
        err,
        CtfTrackerError::IllegalStatusTransition {
            from: CtfOperationStatus::Submitted,
            to: CtfOperationStatus::Planned,
        }
    );
}

#[test]
fn duplicate_relayer_transaction_id_is_rejected() {
    let mut tracker = CtfTracker::new();

    tracker
        .record(CtfOperation::new(
            CtfOperationKind::Split,
            ConditionId::from("condition-6"),
            Some("relayer-tx-dup".to_owned()),
            Some("nonce-6".to_owned()),
            CtfOperationStatus::Submitted,
        ))
        .expect("first relayer transaction id should be accepted");

    let err = tracker
        .record(CtfOperation::new(
            CtfOperationKind::Redeem,
            ConditionId::from("condition-7"),
            Some("relayer-tx-dup".to_owned()),
            Some("nonce-7".to_owned()),
            CtfOperationStatus::Submitted,
        ))
        .expect_err("duplicate relayer transaction id must be rejected");

    assert_eq!(
        err,
        CtfTrackerError::RelayerTransactionIdConflict {
            relayer_transaction_id: "relayer-tx-dup".to_owned(),
        }
    );
}

#[test]
fn relayer_id_cannot_be_rebound_and_old_index_remains_clean() {
    let mut tracker = CtfTracker::new();
    let operation_id = tracker
        .record(CtfOperation::new(
            CtfOperationKind::Split,
            ConditionId::from("condition-8"),
            Some("relayer-tx-old".to_owned()),
            Some("nonce-8".to_owned()),
            CtfOperationStatus::Planned,
        ))
        .expect("initial relayer transaction id should be accepted");

    let err = tracker
        .attach_relayer_metadata(
            operation_id,
            Some("relayer-tx-new".to_owned()),
            Some("nonce-8b".to_owned()),
        )
        .expect_err("rebinding an existing relayer transaction id should be rejected");

    assert_eq!(err, CtfTrackerError::RelayerTransactionIdAlreadyBound);
    assert_eq!(
        tracker
            .operation_by_relayer_transaction_id("relayer-tx-old")
            .map(|operation| operation.condition_id.as_str()),
        Some("condition-8")
    );
    assert_eq!(
        tracker.operation_by_relayer_transaction_id("relayer-tx-new"),
        None
    );
}

#[test]
fn submitted_operation_cannot_change_an_already_bound_nonce() {
    let mut tracker = CtfTracker::new();
    let operation_id = tracker
        .record(CtfOperation::new(
            CtfOperationKind::Split,
            ConditionId::from("condition-9"),
            Some("relayer-tx-9".to_owned()),
            Some("nonce-9".to_owned()),
            CtfOperationStatus::Submitted,
        ))
        .expect("submitted operation should be valid");

    let err = tracker
        .attach_relayer_metadata(operation_id, None, Some("nonce-9b".to_owned()))
        .expect_err("submitted operation must not allow nonce mutation");

    assert_eq!(
        err,
        CtfTrackerError::RelayerMetadataFrozen {
            status: CtfOperationStatus::Submitted,
        }
    );
}

#[test]
fn confirmed_operation_cannot_attach_metadata_after_confirmation() {
    let mut tracker = CtfTracker::new();
    let operation_id = tracker
        .record(CtfOperation::new(
            CtfOperationKind::Redeem,
            ConditionId::from("condition-10"),
            Some("relayer-tx-10".to_owned()),
            Some("nonce-10".to_owned()),
            CtfOperationStatus::Submitted,
        ))
        .expect("submitted operation should be valid");

    tracker
        .update_status(
            operation_id,
            CtfOperationStatus::Confirmed,
            Some("0xtx10".to_owned()),
        )
        .expect("confirmation should succeed");

    let err = tracker
        .attach_relayer_metadata(operation_id, None, Some("nonce-10".to_owned()))
        .expect_err("confirmed operation must not allow metadata attachment");

    assert_eq!(
        err,
        CtfTrackerError::RelayerMetadataFrozen {
            status: CtfOperationStatus::Confirmed,
        }
    );
}

#[test]
fn planned_operation_can_reaffirm_the_same_nonce_without_mutating_state() {
    let mut tracker = CtfTracker::new();
    let operation_id = tracker
        .record(CtfOperation::new(
            CtfOperationKind::Split,
            ConditionId::from("condition-11"),
            Some("relayer-tx-11".to_owned()),
            Some("nonce-11".to_owned()),
            CtfOperationStatus::Planned,
        ))
        .expect("planned operation should be valid");

    tracker
        .attach_relayer_metadata(operation_id, None, Some("nonce-11".to_owned()))
        .expect("reasserting the same nonce in planned state should be a no-op");

    let tracked = tracker
        .operation(operation_id)
        .expect("operation should remain tracked");
    assert_eq!(tracked.nonce.as_deref(), Some("nonce-11"));
    assert_eq!(tracked.status, CtfOperationStatus::Planned);
}

fn sample_identity(label: &str) -> SignedOrderIdentity {
    SignedOrderIdentity {
        signed_order_hash: format!("hash-{label}"),
        salt: format!("salt-{label}"),
        nonce: format!("nonce-{label}"),
        signature: format!("signature-{label}"),
    }
}
