use std::collections::{HashMap, HashSet};

use domain::{
    ApprovalKey, ApprovalState, ConditionId, InventoryBucket, Order, OrderId, ResolutionState,
    TokenId,
};
use rust_decimal::Decimal;

use crate::store::{RelayerTxSummary, StateStore};

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
    InventoryMismatch {
        token_id: TokenId,
        bucket: InventoryBucket,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RemoteSnapshot {
    pub open_orders: Vec<Order>,
    pub approvals: Vec<ApprovalState>,
    pub inventory: Vec<(TokenId, InventoryBucket, Decimal)>,
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

    attention.extend(detect_duplicate_signed_orders(snapshot));
    attention.extend(detect_identifier_mismatches(snapshot));
    attention.extend(compare_orders(store, snapshot));
    attention.extend(compare_approvals(store, snapshot));
    attention.extend(compare_relayer_txs(store, snapshot));
    attention.sort_by(attention_sort_key);

    attention
}

fn detect_duplicate_signed_orders(snapshot: &RemoteSnapshot) -> Vec<ReconcileAttention> {
    let mut hash_to_order_id = HashMap::<String, OrderId>::new();
    let mut attention = Vec::new();

    for order in &snapshot.open_orders {
        let Some(signed_order) = &order.signed_order else {
            continue;
        };

        match hash_to_order_id.entry(signed_order.signed_order_hash.clone()) {
            std::collections::hash_map::Entry::Occupied(existing)
                if existing.get() != &order.order_id =>
            {
                attention.push(ReconcileAttention::DuplicateSignedOrder {
                    order_id: order.order_id.clone(),
                    signed_order_hash: signed_order.signed_order_hash.clone(),
                });
            }
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(order.order_id.clone());
            }
            std::collections::hash_map::Entry::Occupied(_) => {}
        }
    }

    attention
}

fn detect_identifier_mismatches(snapshot: &RemoteSnapshot) -> Vec<ReconcileAttention> {
    let mut token_to_condition = HashMap::<TokenId, ConditionId>::new();
    let mut attention = Vec::new();

    for order in &snapshot.open_orders {
        match token_to_condition.entry(order.token_id.clone()) {
            std::collections::hash_map::Entry::Occupied(existing)
                if existing.get() != &order.condition_id =>
            {
                attention.push(ReconcileAttention::IdentifierMismatch {
                    token_id: order.token_id.clone(),
                    expected_condition_id: existing.get().clone(),
                    remote_condition_id: order.condition_id.clone(),
                });
            }
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(order.condition_id.clone());
            }
            std::collections::hash_map::Entry::Occupied(_) => {}
        }
    }

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
        (!same_order_identity(local, remote)).then(|| ReconcileAttention::OrderStateMismatch {
            order_id: order_id.clone(),
        })
    }));

    attention
}

fn compare_approvals(store: &StateStore, snapshot: &RemoteSnapshot) -> Vec<ReconcileAttention> {
    let local_keys = store.approvals().keys().cloned().collect::<HashSet<_>>();
    let remote_keys = snapshot
        .approvals
        .iter()
        .map(crate::store::approval_key)
        .collect::<HashSet<_>>();

    collect_symmetric_difference(local_keys, remote_keys)
        .into_iter()
        .map(|key| ReconcileAttention::ApprovalMismatch { key })
        .collect()
}

fn compare_relayer_txs(store: &StateStore, snapshot: &RemoteSnapshot) -> Vec<ReconcileAttention> {
    let local_ids = store.relayer_txs().keys().cloned().collect::<HashSet<_>>();
    let remote_ids = snapshot
        .relayer_txs
        .iter()
        .map(|tx| tx.tx_id.clone())
        .collect::<HashSet<_>>();

    remote_ids
        .difference(&local_ids)
        .cloned()
        .into_iter()
        .map(|tx_id| ReconcileAttention::RelayerTxMismatch { tx_id })
        .collect()
}

fn collect_symmetric_difference<T>(local: HashSet<T>, remote: HashSet<T>) -> Vec<T>
where
    T: Clone + Eq + std::hash::Hash,
{
    local.symmetric_difference(&remote).cloned().collect()
}

fn same_order_identity(local: &Order, remote: &Order) -> bool {
    local.market_id == remote.market_id
        && local.condition_id == remote.condition_id
        && local.token_id == remote.token_id
        && local.quantity == remote.quantity
        && local.price == remote.price
        && local.signed_order == remote.signed_order
}

fn attention_sort_key(left: &ReconcileAttention, right: &ReconcileAttention) -> std::cmp::Ordering {
    attention_key(left).cmp(&attention_key(right))
}

fn attention_key(attention: &ReconcileAttention) -> (u8, String, String, String) {
    match attention {
        ReconcileAttention::DuplicateSignedOrder {
            order_id,
            signed_order_hash,
        } => (
            0,
            signed_order_hash.clone(),
            order_id.as_str().to_owned(),
            String::new(),
        ),
        ReconcileAttention::IdentifierMismatch {
            token_id,
            expected_condition_id,
            remote_condition_id,
        } => (
            1,
            token_id.as_str().to_owned(),
            expected_condition_id.as_str().to_owned(),
            remote_condition_id.as_str().to_owned(),
        ),
        ReconcileAttention::MissingRemoteOrder { order_id } => (
            2,
            order_id.as_str().to_owned(),
            String::new(),
            String::new(),
        ),
        ReconcileAttention::UnexpectedRemoteOrder { order_id } => (
            3,
            order_id.as_str().to_owned(),
            String::new(),
            String::new(),
        ),
        ReconcileAttention::OrderStateMismatch { order_id } => (
            4,
            order_id.as_str().to_owned(),
            String::new(),
            String::new(),
        ),
        ReconcileAttention::ApprovalMismatch { key } => (
            5,
            key.token_id.as_str().to_owned(),
            key.owner_address.clone(),
            key.spender.clone(),
        ),
        ReconcileAttention::InventoryMismatch { token_id, bucket } => (
            6,
            token_id.as_str().to_owned(),
            format!("{bucket:?}"),
            String::new(),
        ),
        ReconcileAttention::ResolutionMismatch { condition_id } => (
            7,
            condition_id.as_str().to_owned(),
            String::new(),
            String::new(),
        ),
        ReconcileAttention::RelayerTxMismatch { tx_id } => {
            (8, tx_id.clone(), String::new(), String::new())
        }
    }
}
