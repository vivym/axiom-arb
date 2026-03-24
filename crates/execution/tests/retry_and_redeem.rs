use domain::{ConditionId, OrderId, SignedOrderIdentity};
use execution::{
    ctf::{CtfOperation, CtfOperationKind, CtfOperationStatus, CtfTracker},
    orders::{RetryKind, SignedOrderEnvelope},
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
        .business_retry(OrderId::from("order-2"), sample_identity("replacement"))
        .expect("business retry should require a fresh identity");

    assert_eq!(retry.retry_kind, RetryKind::Business);
    assert_eq!(retry.order_id, OrderId::from("order-2"));
    assert_ne!(retry.identity, order.identity);
    assert_eq!(retry.retry_of_order_id, Some(OrderId::from("order-1")));
}

#[test]
fn business_retry_rejects_reusing_the_same_signed_order_identity() {
    let order = SignedOrderEnvelope::new(OrderId::from("order-1"), sample_identity("original"));

    let err = order
        .business_retry(OrderId::from("order-2"), sample_identity("original"))
        .expect_err("business retry must not reuse the same signed-order identity");

    assert_eq!(
        err,
        execution::orders::BusinessRetryError::IdentityUnchanged
    );
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

    tracker.record(CtfOperation::new(
        CtfOperationKind::Redeem,
        condition_id.clone(),
        "relayer-tx-1",
        7,
        CtfOperationStatus::Submitted,
    ));
    tracker.update_status(
        "relayer-tx-1",
        CtfOperationStatus::Confirmed,
        Some("0xabc123".to_owned()),
    );

    let tracked = tracker
        .operation("relayer-tx-1")
        .expect("ctf operation should be tracked");

    assert_eq!(tracked.kind, CtfOperationKind::Redeem);
    assert_eq!(tracked.condition_id, condition_id);
    assert_eq!(tracked.relayer_transaction_id, "relayer-tx-1");
    assert_eq!(tracked.nonce, 7);
    assert_eq!(tracked.tx_hash.as_deref(), Some("0xabc123"));
    assert_eq!(tracked.status, CtfOperationStatus::Confirmed);
}

fn sample_identity(label: &str) -> SignedOrderIdentity {
    SignedOrderIdentity {
        signed_order_hash: format!("hash-{label}"),
        salt: format!("salt-{label}"),
        nonce: format!("nonce-{label}"),
        signature: format!("signature-{label}"),
    }
}
