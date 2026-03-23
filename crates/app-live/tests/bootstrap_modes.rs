use domain::{
    MarketId, Order, OrderId, SettlementState, SubmissionState, TokenId, VenueOrderState,
};
use domain::{RuntimeMode, RuntimeOverlay};
use rust_decimal::Decimal;
use state::{RemoteSnapshot, StateStore};

#[derive(Debug, Default)]
struct TestAppRuntime {
    store: StateStore,
}

impl TestAppRuntime {
    fn new() -> Self {
        Self {
            store: StateStore::new(),
        }
    }

    fn mode(&self) -> RuntimeMode {
        self.store.mode()
    }

    fn overlay(&self) -> Option<RuntimeOverlay> {
        self.store.mode_overlay()
    }

    fn reconcile(&mut self, snapshot: RemoteSnapshot) -> state::ReconcileReport {
        self.store.reconcile(snapshot)
    }
}

#[test]
fn app_stays_in_bootstrap_cancel_only_without_successful_reconcile() {
    let runtime = TestAppRuntime::new();

    assert_eq!(runtime.mode(), RuntimeMode::Bootstrapping);
    assert_eq!(runtime.overlay(), Some(RuntimeOverlay::CancelOnly));
}

#[test]
fn app_leaves_bootstrap_only_after_successful_reconcile() {
    let mut runtime = TestAppRuntime::new();

    let report = runtime.reconcile(RemoteSnapshot::empty());
    assert!(report.succeeded);
    assert_eq!(runtime.mode(), RuntimeMode::Healthy);
    assert_eq!(runtime.overlay(), None);
}

#[test]
fn app_failed_reconcile_keeps_cancel_only_until_first_success() {
    let mut runtime = TestAppRuntime::new();

    runtime.store.record_local_order(Order {
        order_id: OrderId::from("order-1"),
        market_id: MarketId::from("market-a"),
        condition_id: "condition-a".into(),
        token_id: TokenId::from("token-yes"),
        quantity: Decimal::new(1, 0),
        price: Decimal::new(50, 2),
        submission_state: SubmissionState::Acked,
        venue_state: VenueOrderState::Live,
        settlement_state: SettlementState::Matched,
        signed_order: None,
    });

    let report = runtime.reconcile(RemoteSnapshot::empty());

    assert!(!report.succeeded);
    assert_eq!(runtime.mode(), RuntimeMode::Reconciling);
    assert_eq!(runtime.overlay(), Some(RuntimeOverlay::CancelOnly));
}
