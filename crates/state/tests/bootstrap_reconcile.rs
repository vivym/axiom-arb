use chrono::{TimeZone, Utc};
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
    assert_eq!(store.state_version(), 0);
    assert_eq!(store.last_applied_journal_seq(), None);
    assert_eq!(store.last_consumed_journal_seq(), None);
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
    assert_eq!(store.state_version(), 0);
    assert_eq!(store.last_applied_journal_seq(), None);
    assert_eq!(store.last_consumed_journal_seq(), None);
    assert!(store.first_reconcile_succeeded());
    assert!(store.allows_automatic_repair());
    assert_eq!(store.mode(), RuntimeMode::Healthy);
    assert_eq!(store.mode_overlay(), None);
}

#[test]
fn open_order_state_progression_reconciles_successfully_and_applies_remote_value() {
    let mut store = StateStore::new();
    let initial_order = sample_order("order-1", "hash-1");
    let progressed_order = Order {
        submission_state: SubmissionState::Submitted,
        venue_state: VenueOrderState::Matched,
        settlement_state: SettlementState::Confirmed,
        ..initial_order.clone()
    };

    store.record_local_order(initial_order.clone());

    let initial = store.reconcile(RemoteSnapshot {
        open_orders: vec![initial_order],
        ..RemoteSnapshot::empty()
    });
    assert!(initial.succeeded);

    let report = store.reconcile(RemoteSnapshot {
        open_orders: vec![progressed_order.clone()],
        ..RemoteSnapshot::empty()
    });

    assert!(report.succeeded);
    assert!(report.attention.is_empty());
    assert_eq!(
        store.open_orders().get(&OrderId::from("order-1")),
        Some(&progressed_order)
    );
}

#[test]
fn approval_progression_reconciles_successfully_and_applies_remote_value() {
    let mut store = StateStore::new();
    let initial_approval = sample_approval_at(
        "token-yes",
        ApprovalStatus::Pending,
        Utc.with_ymd_and_hms(2026, 3, 24, 10, 0, 0).unwrap(),
    );
    let progressed_approval = sample_approval_at(
        "token-yes",
        ApprovalStatus::Approved,
        Utc.with_ymd_and_hms(2026, 3, 24, 10, 5, 0).unwrap(),
    );

    store.record_local_approval(initial_approval.clone());

    let initial = store.reconcile(RemoteSnapshot {
        approvals: vec![initial_approval],
        ..RemoteSnapshot::empty()
    });
    assert!(initial.succeeded);

    let report = store.reconcile(RemoteSnapshot {
        approvals: vec![progressed_approval.clone()],
        ..RemoteSnapshot::empty()
    });

    assert!(report.succeeded);
    assert!(report.attention.is_empty());
    assert_eq!(
        store.approvals().get(&domain::ApprovalKey {
            token_id: TokenId::from("token-yes"),
            spender: "0xspender".to_owned(),
            owner_address: "0xowner".to_owned(),
        }),
        Some(&progressed_approval)
    );
}

#[test]
fn resolution_progression_reconciles_successfully() {
    let mut store = StateStore::new();
    let initial_resolution =
        sample_resolution_with_status("condition-a", ResolutionStatus::Unresolved);
    store.record_local_resolution(initial_resolution.clone());

    let initial = store.reconcile(RemoteSnapshot {
        resolution_states: vec![initial_resolution],
        ..RemoteSnapshot::empty()
    });
    assert!(initial.succeeded);

    let progressed_resolution =
        sample_resolution_with_status("condition-a", ResolutionStatus::Resolved);
    let report = store.reconcile(RemoteSnapshot {
        resolution_states: vec![progressed_resolution.clone()],
        ..RemoteSnapshot::empty()
    });

    assert!(report.succeeded);
    assert!(report.attention.is_empty());
    assert_eq!(
        store.resolution().get(&ConditionId::from("condition-a")),
        Some(&progressed_resolution)
    );
}

#[test]
fn relayer_tx_progression_reconciles_successfully_and_applies_remote_value() {
    let mut store = StateStore::new();
    let initial_tx = sample_relayer_tx_with_status("tx-1", "submitted");
    store.record_local_relayer_tx(initial_tx.clone());

    let initial = store.reconcile(RemoteSnapshot {
        relayer_txs: vec![initial_tx],
        ..RemoteSnapshot::empty()
    });
    assert!(initial.succeeded);

    let progressed_tx = sample_relayer_tx_with_status("tx-1", "confirmed");
    let report = store.reconcile(RemoteSnapshot {
        relayer_txs: vec![progressed_tx.clone()],
        ..RemoteSnapshot::empty()
    });

    assert!(report.succeeded);
    assert!(report.attention.is_empty());
    assert_eq!(store.relayer_txs().get("tx-1"), Some(&progressed_tx));
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
    assert_eq!(store.state_version(), 0);
    assert_eq!(store.last_applied_journal_seq(), None);
    assert_eq!(
        report.attention,
        vec![ReconcileAttention::DuplicateSignedOrder {
            order_id: OrderId::from("order-1"),
            signed_order_hash: "hash-1".to_owned(),
        }]
    );
}

#[test]
fn duplicate_signed_order_in_remote_snapshot_forces_attention_without_upstream_signal() {
    let mut store = StateStore::new();
    let order_a = sample_order("order-1", "hash-shared");
    let order_b = sample_order("order-2", "hash-shared");

    store.record_local_order(order_a.clone());
    store.record_local_order(order_b.clone());

    let report = store.reconcile(RemoteSnapshot {
        open_orders: vec![order_a, order_b],
        ..RemoteSnapshot::empty()
    });

    assert!(!report.succeeded);
    assert_eq!(store.mode(), RuntimeMode::Reconciling);
    assert_eq!(store.mode_overlay(), Some(RuntimeOverlay::CancelOnly));
    assert_eq!(store.state_version(), 0);
    assert_eq!(store.last_applied_journal_seq(), None);
    assert!(!store.first_reconcile_succeeded());
    assert!(report.attention.iter().any(|attention| {
        matches!(
            attention,
            ReconcileAttention::DuplicateSignedOrder {
                signed_order_hash,
                ..
            } if signed_order_hash == "hash-shared"
        )
    }));
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
fn identifier_mismatch_in_remote_snapshot_forces_attention_without_upstream_signal() {
    let mut store = StateStore::new();
    let expected = sample_order("order-1", "hash-1");
    let mismatched = sample_order_for_condition("order-2", "hash-2", "token-yes", "condition-b");

    store.record_local_order(expected.clone());
    store.record_local_order(mismatched.clone());

    let report = store.reconcile(RemoteSnapshot {
        open_orders: vec![expected, mismatched],
        ..RemoteSnapshot::empty()
    });

    assert!(!report.succeeded);
    assert_eq!(store.mode(), RuntimeMode::Reconciling);
    assert_eq!(store.mode_overlay(), Some(RuntimeOverlay::CancelOnly));
    assert!(report.attention.iter().any(|attention| {
        matches!(
            attention,
            ReconcileAttention::IdentifierMismatch {
                token_id,
                expected_condition_id,
                remote_condition_id,
            } if token_id == &TokenId::from("token-yes")
                && expected_condition_id == &ConditionId::from("condition-a")
                && remote_condition_id == &ConditionId::from("condition-b")
        )
    }));
}

#[test]
fn attention_order_is_stable_for_multiple_order_differences() {
    let mut store = StateStore::new();

    store.record_local_order(sample_order("order-b", "hash-b"));
    store.record_local_order(sample_order("order-a", "hash-a"));

    let report = store.reconcile(RemoteSnapshot {
        open_orders: vec![
            sample_order("order-d", "hash-d"),
            sample_order("order-c", "hash-c"),
        ],
        ..RemoteSnapshot::empty()
    });

    assert!(!report.succeeded);
    assert_eq!(
        report.attention,
        vec![
            ReconcileAttention::MissingRemoteOrder {
                order_id: OrderId::from("order-a"),
            },
            ReconcileAttention::MissingRemoteOrder {
                order_id: OrderId::from("order-b"),
            },
            ReconcileAttention::UnexpectedRemoteOrder {
                order_id: OrderId::from("order-c"),
            },
            ReconcileAttention::UnexpectedRemoteOrder {
                order_id: OrderId::from("order-d"),
            },
        ]
    );
}

#[test]
fn attention_order_is_deterministic_across_attention_sources() {
    let mut store = StateStore::new();

    store.record_local_order(sample_order("order-b", "hash-b"));
    store.record_local_order(sample_order("order-a", "hash-a"));

    let report = store.reconcile(
        RemoteSnapshot {
            open_orders: vec![
                sample_order("order-d", "hash-d"),
                sample_order("order-c", "hash-c"),
            ],
            ..RemoteSnapshot::empty()
        }
        .with_attention(ReconcileAttention::RelayerTxMismatch {
            tx_id: "tx-9".to_owned(),
        }),
    );

    assert!(!report.succeeded);
    assert_eq!(
        report.attention,
        vec![
            ReconcileAttention::MissingRemoteOrder {
                order_id: OrderId::from("order-a"),
            },
            ReconcileAttention::MissingRemoteOrder {
                order_id: OrderId::from("order-b"),
            },
            ReconcileAttention::UnexpectedRemoteOrder {
                order_id: OrderId::from("order-c"),
            },
            ReconcileAttention::UnexpectedRemoteOrder {
                order_id: OrderId::from("order-d"),
            },
            ReconcileAttention::RelayerTxMismatch {
                tx_id: "tx-9".to_owned(),
            },
        ]
    );
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

#[test]
fn approval_key_set_mismatch_forces_reconcile_attention() {
    let mut store = StateStore::new();
    store.record_local_approval(sample_approval_for_spender("token-yes", "0xspender-a"));

    let report = store.reconcile(RemoteSnapshot {
        approvals: vec![sample_approval_for_spender("token-yes", "0xspender-b")],
        ..RemoteSnapshot::empty()
    });

    assert!(!report.succeeded);
    assert_eq!(
        report.attention,
        vec![
            ReconcileAttention::ApprovalMismatch {
                key: domain::ApprovalKey {
                    token_id: TokenId::from("token-yes"),
                    spender: "0xspender-a".to_owned(),
                    owner_address: "0xowner".to_owned(),
                },
            },
            ReconcileAttention::ApprovalMismatch {
                key: domain::ApprovalKey {
                    token_id: TokenId::from("token-yes"),
                    spender: "0xspender-b".to_owned(),
                    owner_address: "0xowner".to_owned(),
                },
            },
        ]
    );
}

#[test]
fn inventory_bucket_migration_reconciles_to_authoritative_remote_snapshot() {
    let mut store = StateStore::new();
    store.record_local_inventory(
        TokenId::from("token-yes"),
        domain::InventoryBucket::Free,
        Decimal::new(5, 0),
    );

    let report = store.reconcile(RemoteSnapshot {
        inventory: vec![(
            TokenId::from("token-yes"),
            domain::InventoryBucket::ReservedForOrder,
            Decimal::new(5, 0),
        )],
        ..RemoteSnapshot::empty()
    });

    assert!(report.succeeded);
    assert!(report.attention.is_empty());
}

#[test]
fn resolution_key_change_reconciles_to_authoritative_remote_snapshot() {
    let mut store = StateStore::new();
    store.record_local_resolution(sample_resolution("condition-a"));

    let report = store.reconcile(RemoteSnapshot {
        resolution_states: vec![sample_resolution("condition-b")],
        ..RemoteSnapshot::empty()
    });

    assert!(report.succeeded);
    assert!(report.attention.is_empty());
    assert!(store
        .resolution()
        .contains_key(&ConditionId::from("condition-b")));
    assert!(!store
        .resolution()
        .contains_key(&ConditionId::from("condition-a")));
}

#[test]
fn relayer_tx_missing_from_recent_remote_window_does_not_force_attention() {
    let mut store = StateStore::new();
    store.record_local_relayer_tx(sample_relayer_tx("tx-a"));

    let report = store.reconcile(RemoteSnapshot {
        relayer_txs: Vec::new(),
        ..RemoteSnapshot::empty()
    });

    assert!(report.succeeded);
    assert!(report.attention.is_empty());
}

#[test]
fn unexpected_remote_relayer_tx_forces_reconcile_attention() {
    let mut store = StateStore::new();

    let report = store.reconcile(RemoteSnapshot {
        relayer_txs: vec![sample_relayer_tx("tx-b")],
        ..RemoteSnapshot::empty()
    });

    assert!(!report.succeeded);
    assert_eq!(
        report.attention,
        vec![ReconcileAttention::RelayerTxMismatch {
            tx_id: "tx-b".to_owned(),
        },]
    );
}

fn sample_order(order_id: &str, signed_order_hash: &str) -> Order {
    sample_order_for_condition(order_id, signed_order_hash, "token-yes", "condition-a")
}

fn sample_order_for_condition(
    order_id: &str,
    signed_order_hash: &str,
    token_id: &str,
    condition_id: &str,
) -> Order {
    Order {
        order_id: OrderId::from(order_id),
        market_id: MarketId::from("market-a"),
        condition_id: ConditionId::from(condition_id),
        token_id: TokenId::from(token_id),
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
    sample_approval_at(token_id, ApprovalStatus::Approved, Utc::now())
}

fn sample_approval_for_spender(token_id: &str, spender: &str) -> ApprovalState {
    ApprovalState {
        spender: spender.to_owned(),
        ..sample_approval(token_id)
    }
}

fn sample_approval_at(
    token_id: &str,
    approval_status: ApprovalStatus,
    last_checked_at: chrono::DateTime<Utc>,
) -> ApprovalState {
    ApprovalState {
        token_id: TokenId::from(token_id),
        spender: "0xspender".to_owned(),
        owner_address: "0xowner".to_owned(),
        funder_address: "0xfunder".to_owned(),
        wallet_route: WalletRoute::Eoa,
        signature_type: SignatureType::Eoa,
        allowance: Decimal::new(100, 0),
        required_min_allowance: Decimal::new(50, 0),
        last_checked_at,
        approval_status,
    }
}

fn sample_resolution(condition_id: &str) -> ResolutionState {
    sample_resolution_with_status(condition_id, ResolutionStatus::Resolved)
}

fn sample_resolution_with_status(
    condition_id: &str,
    resolution_status: ResolutionStatus,
) -> ResolutionState {
    ResolutionState {
        condition_id: ConditionId::from(condition_id),
        resolution_status,
        payout_vector: vec![Decimal::new(1, 0), Decimal::new(0, 0)],
        resolved_at: Some(Utc::now()),
        dispute_state: DisputeState::None,
        redeemable_at: Some(Utc::now()),
    }
}

fn sample_relayer_tx(tx_id: &str) -> RelayerTxSummary {
    sample_relayer_tx_with_status(tx_id, "confirmed")
}

fn sample_relayer_tx_with_status(tx_id: &str, status: &str) -> RelayerTxSummary {
    RelayerTxSummary {
        tx_id: tx_id.to_owned(),
        order_id: Some(OrderId::from("order-1")),
        status: status.to_owned(),
    }
}
