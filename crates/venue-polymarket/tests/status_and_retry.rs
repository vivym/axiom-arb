use domain::{OrderId, RuntimeMode, RuntimeOverlay, SignedOrderIdentity};
use venue_polymarket::{map_venue_status, RetryClass, RetryDecision};

#[test]
fn http_503_cancel_only_maps_to_no_new_risk_cancel_only() {
    let mapped = map_venue_status(503, Some("cancel-only"));

    assert_eq!(mapped.mode, RuntimeMode::NoNewRisk);
    assert_eq!(mapped.overlay, Some(RuntimeOverlay::CancelOnly));
}

#[test]
fn http_503_trading_disabled_halts_globally() {
    let mapped = map_venue_status(503, Some("trading-disabled"));

    assert_eq!(mapped.mode, RuntimeMode::GlobalHalt);
    assert_eq!(mapped.overlay, None);
}

#[test]
fn http_425_is_transport_retry_that_forces_reconciling() {
    let retry = RetryDecision::for_http_status(425, None, Some(&sample_signed_order()));

    assert_eq!(retry.class, RetryClass::Transport);
    assert!(retry.reuse_payload);
    assert!(retry.backoff);
    assert_eq!(retry.next_mode, Some(RuntimeMode::Reconciling));
    assert_eq!(retry.preserved_identity, Some(sample_signed_order()));
}

#[test]
fn transport_retry_preserves_signed_order_identity() {
    let retry = RetryDecision::for_transport_timeout(&sample_signed_order());

    assert!(retry.reuse_payload);
    assert_eq!(retry.preserved_identity, Some(sample_signed_order()));
}

#[test]
fn duplicate_signed_order_requires_reconcile_before_business_retry() {
    let retry =
        RetryDecision::for_duplicate_signed_order(sample_order_id(), &sample_signed_order());

    assert_eq!(retry.class, RetryClass::Business);
    assert!(!retry.reuse_payload);
    assert!(retry.reconcile_first);
    assert_eq!(retry.retry_of_order_id, Some(sample_order_id()));
    assert_eq!(retry.next_mode, Some(RuntimeMode::Reconciling));
}

fn sample_signed_order() -> SignedOrderIdentity {
    SignedOrderIdentity {
        signed_order_hash: "0xhash".to_owned(),
        salt: "123".to_owned(),
        nonce: "7".to_owned(),
        signature: "0xsig".to_owned(),
    }
}

fn sample_order_id() -> OrderId {
    OrderId::new("ord_123")
}
