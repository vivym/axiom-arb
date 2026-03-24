use std::collections::HashMap;

use domain::ConditionId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CtfOperationKind {
    Split,
    Merge,
    Redeem,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CtfOperationStatus {
    Planned,
    Submitted,
    Confirmed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CtfOperation {
    pub kind: CtfOperationKind,
    pub condition_id: ConditionId,
    pub relayer_transaction_id: String,
    pub nonce: u64,
    pub tx_hash: Option<String>,
    pub status: CtfOperationStatus,
}

impl CtfOperation {
    pub fn new(
        kind: CtfOperationKind,
        condition_id: ConditionId,
        relayer_transaction_id: impl Into<String>,
        nonce: u64,
        status: CtfOperationStatus,
    ) -> Self {
        Self {
            kind,
            condition_id,
            relayer_transaction_id: relayer_transaction_id.into(),
            nonce,
            tx_hash: None,
            status,
        }
    }
}

#[derive(Debug, Default)]
pub struct CtfTracker {
    operations: HashMap<String, CtfOperation>,
}

impl CtfTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record(&mut self, operation: CtfOperation) {
        self.operations
            .insert(operation.relayer_transaction_id.clone(), operation);
    }

    pub fn update_status(
        &mut self,
        relayer_transaction_id: &str,
        status: CtfOperationStatus,
        tx_hash: Option<String>,
    ) -> bool {
        let Some(operation) = self.operations.get_mut(relayer_transaction_id) else {
            return false;
        };

        operation.status = status;

        if let Some(tx_hash) = tx_hash {
            operation.tx_hash = Some(tx_hash);
        }

        true
    }

    pub fn operation(&self, relayer_transaction_id: &str) -> Option<&CtfOperation> {
        self.operations.get(relayer_transaction_id)
    }
}
