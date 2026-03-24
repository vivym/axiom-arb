use domain::{ConditionId, OrderId, SignedOrderIdentity};
use execution::{
    ctf::{CtfOperation, CtfOperationKind, CtfOperationStatus, CtfTracker},
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
fn ctf_tracker_preserves_relayer_nonce_and_status_semantics() {
    let condition_id = ConditionId::from("condition-1");
    let mut tracker = CtfTracker::new();

    let operation_id = tracker.record(CtfOperation::new(
        CtfOperationKind::Redeem,
        condition_id.clone(),
        Some("relayer-tx-1".to_owned()),
        Some("7".to_owned()),
        CtfOperationStatus::Submitted,
    ));
    tracker.update_status(
        operation_id,
        CtfOperationStatus::Confirmed,
        Some("0xabc123".to_owned()),
    );

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
fn planned_ctf_operation_can_omit_relayer_metadata_until_later_update() {
    let condition_id = ConditionId::from("condition-2");
    let mut tracker = CtfTracker::new();

    let operation_id = tracker.record(CtfOperation::new(
        CtfOperationKind::Split,
        condition_id.clone(),
        None,
        None,
        CtfOperationStatus::Planned,
    ));

    let planned = tracker
        .operation(operation_id)
        .expect("planned operation should be tracked");
    assert_eq!(planned.relayer_transaction_id, None);
    assert_eq!(planned.nonce, None);
    assert_eq!(planned.status, CtfOperationStatus::Planned);

    tracker.attach_relayer_metadata(
        operation_id,
        Some("relayer-tx-2".to_owned()),
        Some("nonce-22".to_owned()),
    );
    tracker.update_status(operation_id, CtfOperationStatus::Submitted, None);

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

fn sample_identity(label: &str) -> SignedOrderIdentity {
    SignedOrderIdentity {
        signed_order_hash: format!("hash-{label}"),
        salt: format!("salt-{label}"),
        nonce: format!("nonce-{label}"),
        signature: format!("signature-{label}"),
    }
}
