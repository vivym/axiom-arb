use chrono::Utc;
use domain::{
    ApprovalState, ApprovalStatus, ConditionId, DisputeState, MarketId, Order, OrderId,
    ResolutionState, ResolutionStatus, RuntimeMode, RuntimeOverlay, SettlementState, SignatureType,
    SignedOrderIdentity, SubmissionState, TokenId, VenueOrderState, WalletRoute,
};
use rust_decimal::Decimal;
use state::{ReconcileAttention, RelayerTxSummary, RemoteSnapshot, StateStore};

#[test]
fn startup_remains_cancel_only_until_first_reconcile_succeeds() {
    let store = StateStore::new();

    assert_eq!(store.mode(), RuntimeMode::Bootstrapping);
    assert_eq!(store.mode_overlay(), Some(RuntimeOverlay::CancelOnly));
    assert!(!store.first_reconcile_succeeded());
    assert!(!store.allows_automatic_repair());
}

#[test]
fn first_reconcile_successfully_leaves_bootstrap_cancel_only() {
    let mut store = StateStore::new();
    let order = sample_order("order-1", "hash-1");
    let approval = sample_approval("token-yes");
    let resolution = sample_resolution("condition-a");
    let relayer_tx = sample_relayer_tx("tx-1");

    store.record_local_order(order.clone());
    store.record_local_approval(approval.clone());
    store.record_local_resolution(resolution.clone());
    store.record_local_relayer_tx(relayer_tx.clone());

    let report = store.reconcile(RemoteSnapshot {
        open_orders: vec![order],
        approvals: vec![approval],
        resolution_states: vec![resolution],
        relayer_txs: vec![relayer_tx],
        ..RemoteSnapshot::empty()
    });

    assert!(report.succeeded);
    assert!(report.attention.is_empty());
    assert!(report.promoted_from_bootstrap);
    assert!(store.first_reconcile_succeeded());
    assert!(store.allows_automatic_repair());
    assert_eq!(store.mode(), RuntimeMode::Healthy);
    assert_eq!(store.mode_overlay(), None);
}

#[test]
fn duplicate_signed_order_detection_forces_reconcile_attention() {
    let mut store = StateStore::new();
    let order = sample_order("order-1", "hash-1");
    let approval = sample_approval("token-yes");
    let resolution = sample_resolution("condition-a");
    let relayer_tx = sample_relayer_tx("tx-1");

    store.record_local_order(order.clone());
    store.record_local_approval(approval.clone());
    store.record_local_resolution(resolution.clone());
    store.record_local_relayer_tx(relayer_tx.clone());

    let initial = store.reconcile(RemoteSnapshot {
        open_orders: vec![order.clone()],
        approvals: vec![approval.clone()],
        resolution_states: vec![resolution.clone()],
        relayer_txs: vec![relayer_tx.clone()],
        ..RemoteSnapshot::empty()
    });
    assert!(initial.succeeded);

    let report = store.reconcile(
        RemoteSnapshot {
            open_orders: vec![order.clone()],
            approvals: vec![approval],
            resolution_states: vec![resolution],
            relayer_txs: vec![relayer_tx],
            ..RemoteSnapshot::empty()
        }
        .with_attention(ReconcileAttention::DuplicateSignedOrder {
            order_id: order.order_id.clone(),
            signed_order_hash: order
                .signed_order
                .as_ref()
                .expect("sample order has signed identity")
                .signed_order_hash
                .clone(),
        }),
    );

    assert!(!report.succeeded);
    assert_eq!(store.mode(), RuntimeMode::Reconciling);
    assert_eq!(store.mode_overlay(), Some(RuntimeOverlay::CancelOnly));
    assert_eq!(
        report.attention,
        vec![ReconcileAttention::DuplicateSignedOrder {
            order_id: OrderId::from("order-1"),
            signed_order_hash: "hash-1".to_owned(),
        }]
    );
}

#[test]
fn identifier_mismatch_forces_reconcile_attention() {
    let mut store = StateStore::new();

    let report = store.reconcile(RemoteSnapshot::empty().with_attention(
        ReconcileAttention::IdentifierMismatch {
            token_id: TokenId::from("token-yes"),
            expected_condition_id: ConditionId::from("condition-a"),
            remote_condition_id: ConditionId::from("condition-b"),
        },
    ));

    assert!(!report.succeeded);
    assert_eq!(store.mode(), RuntimeMode::Reconciling);
    assert_eq!(store.mode_overlay(), Some(RuntimeOverlay::CancelOnly));
    assert!(!store.first_reconcile_succeeded());
}

#[test]
fn unresolved_divergence_keeps_restrictive_mode_and_preserves_local_view() {
    let mut store = StateStore::new();
    let local_order = sample_order("order-1", "hash-1");
    let local_approval = sample_approval("token-yes");

    store.record_local_order(local_order.clone());
    store.record_local_approval(local_approval.clone());

    let report = store.reconcile(RemoteSnapshot {
        approvals: vec![local_approval],
        ..RemoteSnapshot::empty()
    });

    assert!(!report.succeeded);
    assert_eq!(store.mode(), RuntimeMode::Reconciling);
    assert_eq!(store.mode_overlay(), Some(RuntimeOverlay::CancelOnly));
    assert!(!store.first_reconcile_succeeded());
    assert_eq!(
        store.open_orders().get(&OrderId::from("order-1")),
        Some(&local_order)
    );
    assert!(matches!(
        &report.attention[..],
        [ReconcileAttention::MissingRemoteOrder { order_id }] if *order_id == OrderId::from("order-1")
    ));
}

fn sample_order(order_id: &str, signed_order_hash: &str) -> Order {
    Order {
        order_id: OrderId::from(order_id),
        market_id: MarketId::from("market-a"),
        condition_id: ConditionId::from("condition-a"),
        token_id: TokenId::from("token-yes"),
        quantity: Decimal::new(5, 0),
        price: Decimal::new(55, 2),
        submission_state: SubmissionState::Acked,
        venue_state: VenueOrderState::Live,
        settlement_state: SettlementState::Matched,
        signed_order: Some(SignedOrderIdentity {
            signed_order_hash: signed_order_hash.to_owned(),
            salt: format!("salt-{order_id}"),
            nonce: format!("nonce-{order_id}"),
            signature: format!("sig-{order_id}"),
        }),
    }
}

fn sample_approval(token_id: &str) -> ApprovalState {
    ApprovalState {
        token_id: TokenId::from(token_id),
        spender: "0xspender".to_owned(),
        owner_address: "0xowner".to_owned(),
        funder_address: "0xfunder".to_owned(),
        wallet_route: WalletRoute::Eoa,
        signature_type: SignatureType::Eoa,
        allowance: Decimal::new(100, 0),
        required_min_allowance: Decimal::new(50, 0),
        last_checked_at: Utc::now(),
        approval_status: ApprovalStatus::Approved,
    }
}

fn sample_resolution(condition_id: &str) -> ResolutionState {
    ResolutionState {
        condition_id: ConditionId::from(condition_id),
        resolution_status: ResolutionStatus::Resolved,
        payout_vector: vec![Decimal::new(1, 0), Decimal::new(0, 0)],
        resolved_at: Some(Utc::now()),
        dispute_state: DisputeState::None,
        redeemable_at: Some(Utc::now()),
    }
}

fn sample_relayer_tx(tx_id: &str) -> RelayerTxSummary {
    RelayerTxSummary {
        tx_id: tx_id.to_owned(),
        order_id: Some(OrderId::from("order-1")),
        status: "confirmed".to_owned(),
    }
}
