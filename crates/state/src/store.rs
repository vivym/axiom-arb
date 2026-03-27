use std::collections::{BTreeMap, HashMap};

use domain::{
    ApprovalKey, ApprovalState, ConditionId, DiscoverySourceAnchor, FamilyDiscoveryRecord,
    InventoryBucket, Order, OrderId, ResolutionState, RuntimeMode, RuntimeOverlay, RuntimePolicy,
    StateConfidence, TokenId,
};
use rust_decimal::Decimal;

use crate::{
    bootstrap::{
        allows_automatic_repair, bootstrap_policy, reconcile_attention_policy, reconciled_policy,
    },
    facts::{FactKey, PendingReconcileAnchor, PendingRef, RuntimeAttentionAnchor},
    reconcile::{reconcile_store, ReconcileReport, RemoteSnapshot},
};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct InventoryEntry {
    pub token_id: TokenId,
    pub bucket: InventoryBucket,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InventorySnapshotRow {
    pub token_id: TokenId,
    pub bucket: InventoryBucket,
    pub quantity: Decimal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelayerTxSummary {
    pub tx_id: String,
    pub order_id: Option<OrderId>,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FullSetAnchor {
    pub state_version: u64,
    pub committed_journal_seq: i64,
    pub open_orders: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PendingFamilyBackfill {
    cursor: String,
    completed_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone)]
pub struct StateStore {
    state_version: u64,
    last_consumed_journal_seq: Option<i64>,
    last_applied_journal_seq: Option<i64>,
    fullset_anchor: Option<FullSetAnchor>,
    runtime_mode: RuntimeMode,
    overlay: Option<RuntimeOverlay>,
    first_reconcile_succeeded: bool,
    applied_fact_journal: BTreeMap<FactKey, i64>,
    consumed_journal: BTreeMap<i64, FactKey>,
    family_discovery_records: BTreeMap<String, FamilyDiscoveryRecord>,
    pending_family_backfills: BTreeMap<String, PendingFamilyBackfill>,
    pending_reconcile_anchors: BTreeMap<String, PendingReconcileAnchor>,
    runtime_attention_anchors: BTreeMap<String, RuntimeAttentionAnchor>,
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
            state_version: 0,
            last_consumed_journal_seq: None,
            last_applied_journal_seq: None,
            fullset_anchor: None,
            runtime_mode: policy.mode,
            overlay: policy.overlay,
            first_reconcile_succeeded: false,
            applied_fact_journal: BTreeMap::new(),
            consumed_journal: BTreeMap::new(),
            family_discovery_records: BTreeMap::new(),
            pending_family_backfills: BTreeMap::new(),
            pending_reconcile_anchors: BTreeMap::new(),
            runtime_attention_anchors: BTreeMap::new(),
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

    pub fn restore_committed_anchor(
        &mut self,
        committed_state_version: u64,
        committed_journal_seq: i64,
    ) {
        self.state_version = committed_state_version;
        self.last_consumed_journal_seq = Some(committed_journal_seq);
        self.last_applied_journal_seq = Some(committed_journal_seq);
        self.rebuild_fullset_anchor(committed_journal_seq);
        self.first_reconcile_succeeded = true;
        self.apply_restore_posture();
    }

    pub fn restore_pending_reconcile_anchor(&mut self, anchor: PendingReconcileAnchor) {
        self.record_pending_reconcile(anchor);
        self.first_reconcile_succeeded = true;
        self.apply_restore_posture();
    }

    pub fn mark_reconciled_after_restore(&mut self, baseline_journal_seq: i64) {
        if self.last_applied_journal_seq.is_none() {
            self.last_consumed_journal_seq = Some(baseline_journal_seq);
            self.last_applied_journal_seq = Some(baseline_journal_seq);
            self.rebuild_fullset_anchor(baseline_journal_seq);
        }

        self.pending_reconcile_anchors.clear();
        self.first_reconcile_succeeded = true;
        self.apply_policy(reconciled_policy());
    }

    pub fn restore_reconciled_policy(&mut self) {
        self.first_reconcile_succeeded = true;
        self.apply_policy(reconciled_policy());
    }

    pub fn mark_reconcile_required(&mut self) {
        self.first_reconcile_succeeded = true;
        self.enter_reconciling();
    }

    pub fn clear_pending_reconcile_after_restore(&mut self) {
        self.pending_reconcile_anchors.clear();
        if self.first_reconcile_succeeded {
            self.apply_restore_posture();
        }
    }

    pub fn mode(&self) -> RuntimeMode {
        self.runtime_mode
    }

    pub fn state_version(&self) -> u64 {
        self.state_version
    }

    pub fn last_applied_journal_seq(&self) -> Option<i64> {
        self.last_applied_journal_seq
    }

    pub fn last_consumed_journal_seq(&self) -> Option<i64> {
        self.last_consumed_journal_seq
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

    pub fn pending_reconcile_count(&self) -> usize {
        self.pending_reconcile_anchors.len()
    }

    pub fn family_discovery_records(&self) -> Vec<FamilyDiscoveryRecord> {
        self.family_discovery_records
            .values()
            .cloned()
            .collect::<Vec<_>>()
    }

    pub fn pending_reconcile_anchors(&self) -> Vec<PendingReconcileAnchor> {
        self.pending_reconcile_anchors
            .values()
            .cloned()
            .collect::<Vec<_>>()
    }

    pub fn runtime_attention_anchors(&self) -> Vec<RuntimeAttentionAnchor> {
        self.runtime_attention_anchors
            .values()
            .cloned()
            .collect::<Vec<_>>()
    }

    pub fn has_runtime_attention(&self, scope_id: &str, attention_kind: &str) -> bool {
        self.runtime_attention_anchors
            .values()
            .any(|anchor| anchor.scope_id == scope_id && anchor.attention_kind == attention_kind)
    }

    pub fn state_confidence(&self, scope: &str) -> StateConfidence {
        if self
            .pending_reconcile_anchors
            .values()
            .any(|anchor| anchor.family_id == scope)
        {
            StateConfidence::Uncertain
        } else {
            StateConfidence::Certain
        }
    }

    pub fn scope_confidence(&self, scope: &str) -> StateConfidence {
        self.state_confidence(scope)
    }

    pub fn open_orders(&self) -> &HashMap<OrderId, Order> {
        &self.open_orders
    }

    fn current_open_order_ids(&self) -> Vec<String> {
        let mut open_orders = self
            .open_orders
            .keys()
            .map(|order_id| order_id.as_str().to_owned())
            .collect::<Vec<_>>();
        open_orders.sort();
        open_orders
    }

    pub(crate) fn anchored_fullset(&self) -> Option<&FullSetAnchor> {
        let committed_journal_seq = self.last_applied_journal_seq?;
        let anchor = self.fullset_anchor.as_ref()?;

        (anchor.state_version == self.state_version
            && anchor.committed_journal_seq == committed_journal_seq)
            .then_some(anchor)
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

    pub fn inventory_snapshot(&self) -> Vec<InventorySnapshotRow> {
        let mut rows = self
            .inventory
            .iter()
            .map(|(entry, quantity)| InventorySnapshotRow {
                token_id: entry.token_id.clone(),
                bucket: entry.bucket,
                quantity: *quantity,
            })
            .collect::<Vec<_>>();

        rows.sort_by(|left, right| {
            left.token_id
                .as_str()
                .cmp(right.token_id.as_str())
                .then_with(|| bucket_sort_key(left.bucket).cmp(&bucket_sort_key(right.bucket)))
        });

        rows
    }

    pub fn record_local_order(&mut self, order: Order) {
        self.fullset_anchor = None;
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
        self.fullset_anchor = None;
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
        if let Some(committed_journal_seq) = self.last_applied_journal_seq {
            self.rebuild_fullset_anchor(committed_journal_seq);
        }

        let promoted_from_bootstrap = !self.first_reconcile_succeeded;

        self.first_reconcile_succeeded = true;
        self.apply_restore_posture();

        promoted_from_bootstrap
    }

    pub(crate) fn set_mode_if_reconciled(&mut self) {
        self.apply_restore_posture();
    }

    fn apply_policy(&mut self, policy: RuntimePolicy) {
        self.runtime_mode = policy.mode;
        self.overlay = policy.overlay;
    }

    fn apply_restore_posture(&mut self) {
        if !self.first_reconcile_succeeded {
            return;
        }

        if self.has_pending_reconcile_anchors() {
            self.enter_reconciling();
        } else {
            self.apply_policy(reconciled_policy());
        }
    }

    fn has_pending_reconcile_anchors(&self) -> bool {
        !self.pending_reconcile_anchors.is_empty()
    }

    pub(crate) fn duplicate_journal_seq(&self, fact_key: &FactKey) -> Option<i64> {
        self.applied_fact_journal.get(fact_key).copied()
    }

    pub(crate) fn consume_journal_seq(
        &mut self,
        journal_seq: i64,
        fact_key: &FactKey,
    ) -> JournalConsumption {
        if let Some(existing_fact) = self.consumed_journal.get(&journal_seq) {
            return if existing_fact == fact_key {
                JournalConsumption::AlreadyBoundToSameFact
            } else {
                JournalConsumption::AlreadyBoundToDifferentFact
            };
        }

        if self
            .last_consumed_journal_seq
            .is_some_and(|last_consumed_journal_seq| journal_seq <= last_consumed_journal_seq)
        {
            return JournalConsumption::OutOfOrder;
        }

        self.last_consumed_journal_seq = Some(journal_seq);
        self.consumed_journal.insert(journal_seq, fact_key.clone());

        JournalConsumption::Consumed
    }

    pub(crate) fn record_applied_fact(&mut self, journal_seq: i64, fact_key: FactKey) -> u64 {
        self.state_version += 1;
        self.last_applied_journal_seq = Some(journal_seq);
        self.applied_fact_journal.insert(fact_key, journal_seq);
        self.rebuild_fullset_anchor(journal_seq);
        self.apply_restore_posture();
        self.state_version
    }

    pub(crate) fn record_pending_reconcile(&mut self, anchor: PendingReconcileAnchor) {
        self.pending_reconcile_anchors
            .insert(anchor.pending_ref.clone(), anchor);
    }

    pub(crate) fn record_family_discovery(&mut self, mut record: FamilyDiscoveryRecord) {
        if let Some(existing) = self.family_discovery_records.get(record.family_id.as_str()) {
            record.backfill_cursor = existing.backfill_cursor.clone();
            record.backfill_completed_at = existing.backfill_completed_at;
        }
        if let Some(pending_backfill) = self
            .pending_family_backfills
            .remove(record.family_id.as_str())
        {
            record.record_backfill(pending_backfill.cursor, pending_backfill.completed_at);
        }

        self.family_discovery_records
            .insert(record.family_id.as_str().to_owned(), record);
    }

    pub(crate) fn record_family_backfill(
        &mut self,
        family_id: &str,
        cursor: impl Into<String>,
        source: DiscoverySourceAnchor,
        observed_at: chrono::DateTime<chrono::Utc>,
        completed_at: Option<chrono::DateTime<chrono::Utc>>,
    ) {
        let cursor = cursor.into();

        if let Some(record) = self.family_discovery_records.get_mut(family_id) {
            record.record_backfill(cursor, completed_at);
            return;
        }

        let _ = (source, observed_at);
        self.pending_family_backfills.insert(
            family_id.to_owned(),
            PendingFamilyBackfill {
                cursor,
                completed_at,
            },
        );
    }

    pub(crate) fn record_runtime_attention(&mut self, anchor: RuntimeAttentionAnchor) {
        self.runtime_attention_anchors
            .insert(anchor.attention_ref.clone(), anchor);
    }

    pub(crate) fn clear_pending_reconcile(
        &mut self,
        pending_ref: &PendingRef,
    ) -> Option<PendingReconcileAnchor> {
        let cleared = self.pending_reconcile_anchors.remove(&pending_ref.0);
        self.apply_restore_posture();
        cleared
    }

    fn rebuild_fullset_anchor(&mut self, committed_journal_seq: i64) {
        self.fullset_anchor = Some(FullSetAnchor {
            state_version: self.state_version,
            committed_journal_seq,
            open_orders: self.current_open_order_ids(),
        });
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum JournalConsumption {
    Consumed,
    AlreadyBoundToSameFact,
    AlreadyBoundToDifferentFact,
    OutOfOrder,
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

fn bucket_sort_key(bucket: InventoryBucket) -> usize {
    match bucket {
        InventoryBucket::Free => 0,
        InventoryBucket::ReservedForOrder => 1,
        InventoryBucket::MatchedUnsettled => 2,
        InventoryBucket::PendingCtfIn => 3,
        InventoryBucket::PendingCtfOut => 4,
        InventoryBucket::Redeemable => 5,
        InventoryBucket::Quarantined => 6,
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
        store.record_local_inventory(
            TokenId::from("token-yes"),
            InventoryBucket::Free,
            Decimal::new(5, 0),
        );

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
