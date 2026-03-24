use std::collections::HashMap;

use domain::{
    ApprovalKey, ApprovalState, ConditionId, InventoryBucket, Order, OrderId, ResolutionState,
    RuntimeMode, RuntimeOverlay, RuntimePolicy, TokenId,
};
use rust_decimal::Decimal;

use crate::{
    bootstrap::{
        allows_automatic_repair, bootstrap_policy, reconcile_attention_policy, reconciled_policy,
    },
    reconcile::{reconcile_store, ReconcileReport, RemoteSnapshot},
};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct InventoryEntry {
    pub token_id: TokenId,
    pub bucket: InventoryBucket,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelayerTxSummary {
    pub tx_id: String,
    pub order_id: Option<OrderId>,
    pub status: String,
}

#[derive(Debug, Clone)]
pub struct StateStore {
    runtime_mode: RuntimeMode,
    overlay: Option<RuntimeOverlay>,
    first_reconcile_succeeded: bool,
    open_orders: HashMap<OrderId, Order>,
    approvals: HashMap<ApprovalKey, ApprovalState>,
    inventory: HashMap<InventoryEntry, Decimal>,
    resolution: HashMap<ConditionId, ResolutionState>,
    relayer_txs: HashMap<String, RelayerTxSummary>,
}

impl StateStore {
    pub fn new() -> Self {
        let policy = bootstrap_policy();

        Self {
            runtime_mode: policy.mode,
            overlay: policy.overlay,
            first_reconcile_succeeded: false,
            open_orders: HashMap::new(),
            approvals: HashMap::new(),
            inventory: HashMap::new(),
            resolution: HashMap::new(),
            relayer_txs: HashMap::new(),
        }
    }

    pub fn mark_bootstrapping(&mut self) {
        if self.first_reconcile_succeeded {
            return;
        }

        self.apply_policy(bootstrap_policy());
    }

    pub fn reconcile(&mut self, snapshot: RemoteSnapshot) -> ReconcileReport {
        reconcile_store(self, snapshot)
    }

    pub fn mode(&self) -> RuntimeMode {
        self.runtime_mode
    }

    pub fn overlay(&self) -> Option<RuntimeOverlay> {
        self.overlay
    }

    pub fn mode_overlay(&self) -> Option<RuntimeOverlay> {
        self.overlay.or_else(|| self.runtime_mode.default_overlay())
    }

    pub fn runtime_policy(&self) -> RuntimePolicy {
        RuntimePolicy {
            mode: self.runtime_mode,
            overlay: self.mode_overlay(),
        }
    }

    pub fn first_reconcile_succeeded(&self) -> bool {
        self.first_reconcile_succeeded
    }

    pub fn allows_automatic_repair(&self) -> bool {
        allows_automatic_repair(self.first_reconcile_succeeded, self.runtime_mode)
    }

    pub fn open_orders(&self) -> &HashMap<OrderId, Order> {
        &self.open_orders
    }

    pub fn approvals(&self) -> &HashMap<ApprovalKey, ApprovalState> {
        &self.approvals
    }

    pub fn resolution(&self) -> &HashMap<ConditionId, ResolutionState> {
        &self.resolution
    }

    pub fn relayer_txs(&self) -> &HashMap<String, RelayerTxSummary> {
        &self.relayer_txs
    }

    pub fn record_local_order(&mut self, order: Order) {
        self.open_orders.insert(order.order_id.clone(), order);
    }

    pub fn record_local_approval(&mut self, approval: ApprovalState) {
        self.approvals.insert(approval_key(&approval), approval);
    }

    pub fn record_local_inventory(
        &mut self,
        token_id: TokenId,
        bucket: InventoryBucket,
        quantity: Decimal,
    ) {
        self.inventory
            .insert(InventoryEntry { token_id, bucket }, quantity);
    }

    pub fn record_local_resolution(&mut self, resolution: ResolutionState) {
        self.resolution
            .insert(resolution.condition_id.clone(), resolution);
    }

    pub fn record_local_relayer_tx(&mut self, tx: RelayerTxSummary) {
        self.relayer_txs.insert(tx.tx_id.clone(), tx);
    }

    pub(crate) fn enter_reconciling(&mut self) {
        self.apply_policy(reconcile_attention_policy());
    }

    pub(crate) fn complete_reconcile(&mut self, snapshot: &RemoteSnapshot) -> bool {
        self.open_orders = snapshot
            .open_orders
            .iter()
            .cloned()
            .map(|order| (order.order_id.clone(), order))
            .collect();
        self.approvals = snapshot
            .approvals
            .iter()
            .cloned()
            .map(|approval| (approval_key(&approval), approval))
            .collect();
        self.inventory = snapshot
            .inventory
            .iter()
            .cloned()
            .map(|(token_id, bucket, quantity)| (InventoryEntry { token_id, bucket }, quantity))
            .collect();
        self.resolution = snapshot
            .resolution_states
            .iter()
            .cloned()
            .map(|resolution| (resolution.condition_id.clone(), resolution))
            .collect();
        self.relayer_txs = snapshot
            .relayer_txs
            .iter()
            .cloned()
            .map(|tx| (tx.tx_id.clone(), tx))
            .collect();

        let promoted_from_bootstrap = !self.first_reconcile_succeeded;

        self.first_reconcile_succeeded = true;
        self.apply_policy(reconciled_policy());

        promoted_from_bootstrap
    }

    pub(crate) fn set_mode_if_reconciled(&mut self) {
        if self.first_reconcile_succeeded {
            self.apply_policy(reconciled_policy());
        }
    }

    fn apply_policy(&mut self, policy: RuntimePolicy) {
        self.runtime_mode = policy.mode;
        self.overlay = policy.overlay;
    }
}

impl Default for StateStore {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) fn approval_key(approval: &ApprovalState) -> ApprovalKey {
    ApprovalKey {
        token_id: approval.token_id.clone(),
        spender: approval.spender.clone(),
        owner_address: approval.owner_address.clone(),
    }
}

#[cfg(test)]
mod tests {
    use domain::{InventoryBucket, TokenId};
    use rust_decimal::Decimal;

    use super::StateStore;
    use crate::RemoteSnapshot;

    #[test]
    fn complete_reconcile_applies_remote_inventory_to_store() {
        let mut store = StateStore::new();
        store.record_local_inventory(
            TokenId::from("token-yes"),
            InventoryBucket::Free,
            Decimal::new(1, 0),
        );

        store.complete_reconcile(&RemoteSnapshot {
            inventory: vec![(
                TokenId::from("token-yes"),
                InventoryBucket::Free,
                Decimal::new(7, 0),
            )],
            ..RemoteSnapshot::empty()
        });

        assert_eq!(store.inventory.len(), 1);
        assert_eq!(
            store.inventory.get(&super::InventoryEntry {
                token_id: TokenId::from("token-yes"),
                bucket: InventoryBucket::Free,
            }),
            Some(&Decimal::new(7, 0))
        );
    }

    #[test]
    fn reconcile_allows_inventory_progression_and_applies_remote_quantity() {
        let mut store = StateStore::new();

        let initial = store.reconcile(RemoteSnapshot {
            inventory: vec![(
                TokenId::from("token-yes"),
                InventoryBucket::Free,
                Decimal::new(5, 0),
            )],
            ..RemoteSnapshot::empty()
        });

        assert!(initial.succeeded);

        let progressed = store.reconcile(RemoteSnapshot {
            inventory: vec![(
                TokenId::from("token-yes"),
                InventoryBucket::Free,
                Decimal::new(4, 0),
            )],
            ..RemoteSnapshot::empty()
        });

        assert!(progressed.succeeded);
        assert_eq!(
            store.inventory.get(&super::InventoryEntry {
                token_id: TokenId::from("token-yes"),
                bucket: InventoryBucket::Free,
            }),
            Some(&Decimal::new(4, 0))
        );
    }
}
