use std::collections::{HashMap, HashSet};

use domain::{ApprovalKey, ApprovalState, ConditionId, Order, OrderId, ResolutionState, TokenId};

use crate::store::{approval_key, RelayerTxSummary, StateStore};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReconcileAttention {
    DuplicateSignedOrder {
        order_id: OrderId,
        signed_order_hash: String,
    },
    IdentifierMismatch {
        token_id: TokenId,
        expected_condition_id: ConditionId,
        remote_condition_id: ConditionId,
    },
    MissingRemoteOrder {
        order_id: OrderId,
    },
    UnexpectedRemoteOrder {
        order_id: OrderId,
    },
    OrderStateMismatch {
        order_id: OrderId,
    },
    ApprovalMismatch {
        key: ApprovalKey,
    },
    ResolutionMismatch {
        condition_id: ConditionId,
    },
    RelayerTxMismatch {
        tx_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RemoteSnapshot {
    pub open_orders: Vec<Order>,
    pub approvals: Vec<ApprovalState>,
    pub resolution_states: Vec<ResolutionState>,
    pub relayer_txs: Vec<RelayerTxSummary>,
    pub attention: Vec<ReconcileAttention>,
}

impl RemoteSnapshot {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn with_attention(mut self, attention: ReconcileAttention) -> Self {
        self.attention.push(attention);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReconcileReport {
    pub succeeded: bool,
    pub promoted_from_bootstrap: bool,
    pub remote_applied: bool,
    pub attention: Vec<ReconcileAttention>,
}

pub(crate) fn reconcile_store(store: &mut StateStore, snapshot: RemoteSnapshot) -> ReconcileReport {
    let attention = collect_attention(store, &snapshot);

    if !attention.is_empty() {
        store.enter_reconciling();
        return ReconcileReport {
            succeeded: false,
            promoted_from_bootstrap: false,
            remote_applied: false,
            attention,
        };
    }

    let promoted_from_bootstrap = store.complete_reconcile(&snapshot);
    store.set_mode_if_reconciled();

    ReconcileReport {
        succeeded: true,
        promoted_from_bootstrap,
        remote_applied: true,
        attention: Vec::new(),
    }
}

fn collect_attention(store: &StateStore, snapshot: &RemoteSnapshot) -> Vec<ReconcileAttention> {
    let mut attention = snapshot.attention.clone();

    attention.extend(compare_orders(store, snapshot));
    attention.extend(compare_approvals(store, snapshot));
    attention.extend(compare_resolutions(store, snapshot));
    attention.extend(compare_relayer_txs(store, snapshot));

    attention
}

fn compare_orders(store: &StateStore, snapshot: &RemoteSnapshot) -> Vec<ReconcileAttention> {
    let remote_map = snapshot
        .open_orders
        .iter()
        .cloned()
        .map(|order| (order.order_id.clone(), order))
        .collect::<HashMap<_, _>>();

    let local_ids = store.open_orders().keys().cloned().collect::<HashSet<_>>();
    let remote_ids = remote_map.keys().cloned().collect::<HashSet<_>>();

    let mut attention = local_ids
        .difference(&remote_ids)
        .cloned()
        .map(|order_id| ReconcileAttention::MissingRemoteOrder { order_id })
        .collect::<Vec<_>>();

    attention.extend(
        remote_ids
            .difference(&local_ids)
            .cloned()
            .map(|order_id| ReconcileAttention::UnexpectedRemoteOrder { order_id }),
    );

    attention.extend(local_ids.intersection(&remote_ids).filter_map(|order_id| {
        let local = store.open_orders().get(order_id)?;
        let remote = remote_map.get(order_id)?;
        (local != remote).then(|| ReconcileAttention::OrderStateMismatch {
            order_id: order_id.clone(),
        })
    }));

    attention
}

fn compare_approvals(store: &StateStore, snapshot: &RemoteSnapshot) -> Vec<ReconcileAttention> {
    let remote_map = snapshot
        .approvals
        .iter()
        .cloned()
        .map(|approval| (approval_key(&approval), approval))
        .collect::<HashMap<_, _>>();

    compare_map(store.approvals(), &remote_map, |key| {
        ReconcileAttention::ApprovalMismatch { key }
    })
}

fn compare_resolutions(store: &StateStore, snapshot: &RemoteSnapshot) -> Vec<ReconcileAttention> {
    let remote_map = snapshot
        .resolution_states
        .iter()
        .cloned()
        .map(|resolution| (resolution.condition_id.clone(), resolution))
        .collect::<HashMap<_, _>>();

    compare_map(store.resolution(), &remote_map, |condition_id| {
        ReconcileAttention::ResolutionMismatch { condition_id }
    })
}

fn compare_relayer_txs(store: &StateStore, snapshot: &RemoteSnapshot) -> Vec<ReconcileAttention> {
    let remote_map = snapshot
        .relayer_txs
        .iter()
        .cloned()
        .map(|tx| (tx.tx_id.clone(), tx))
        .collect::<HashMap<_, _>>();

    compare_map(store.relayer_txs(), &remote_map, |tx_id| {
        ReconcileAttention::RelayerTxMismatch { tx_id }
    })
}

fn compare_map<K, V, F>(
    local: &HashMap<K, V>,
    remote: &HashMap<K, V>,
    into_attention: F,
) -> Vec<ReconcileAttention>
where
    K: Clone + Eq + std::hash::Hash,
    V: PartialEq,
    F: Fn(K) -> ReconcileAttention,
{
    let local_keys = local.keys().cloned().collect::<HashSet<_>>();
    let remote_keys = remote.keys().cloned().collect::<HashSet<_>>();

    let mut attention = local_keys
        .difference(&remote_keys)
        .cloned()
        .map(&into_attention)
        .collect::<Vec<_>>();

    attention.extend(
        remote_keys
            .difference(&local_keys)
            .cloned()
            .map(&into_attention),
    );

    attention.extend(local_keys.intersection(&remote_keys).filter_map(|key| {
        let local_value = local.get(key)?;
        let remote_value = remote.get(key)?;
        (local_value != remote_value).then(|| into_attention(key.clone()))
    }));

    attention
}
