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
    pub relayer_transaction_id: Option<String>,
    pub nonce: Option<String>,
    pub tx_hash: Option<String>,
    pub status: CtfOperationStatus,
}

impl CtfOperation {
    pub fn new(
        kind: CtfOperationKind,
        condition_id: ConditionId,
        relayer_transaction_id: Option<String>,
        nonce: Option<String>,
        status: CtfOperationStatus,
    ) -> Self {
        Self {
            kind,
            condition_id,
            relayer_transaction_id,
            nonce,
            tx_hash: None,
            status,
        }
    }
}

pub type CtfOperationId = usize;

#[derive(Debug, Default)]
pub struct CtfTracker {
    operations: HashMap<CtfOperationId, CtfOperation>,
    relayer_index: HashMap<String, CtfOperationId>,
    next_operation_id: CtfOperationId,
}

impl CtfTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record(&mut self, operation: CtfOperation) -> CtfOperationId {
        let operation_id = self.next_operation_id;
        self.next_operation_id += 1;

        if let Some(relayer_transaction_id) = operation.relayer_transaction_id.clone() {
            self.relayer_index
                .insert(relayer_transaction_id, operation_id);
        }

        self.operations.insert(operation_id, operation);
        operation_id
    }

    pub fn attach_relayer_metadata(
        &mut self,
        operation_id: CtfOperationId,
        relayer_transaction_id: Option<String>,
        nonce: Option<String>,
    ) -> bool {
        let Some(operation) = self.operations.get_mut(&operation_id) else {
            return false;
        };

        if let Some(relayer_transaction_id) = relayer_transaction_id {
            self.relayer_index
                .insert(relayer_transaction_id.clone(), operation_id);
            operation.relayer_transaction_id = Some(relayer_transaction_id);
        }

        if let Some(nonce) = nonce {
            operation.nonce = Some(nonce);
        }

        true
    }

    pub fn update_status(
        &mut self,
        operation_id: CtfOperationId,
        status: CtfOperationStatus,
        tx_hash: Option<String>,
    ) -> bool {
        let Some(operation) = self.operations.get_mut(&operation_id) else {
            return false;
        };

        operation.status = status;

        if let Some(tx_hash) = tx_hash {
            operation.tx_hash = Some(tx_hash);
        }

        true
    }

    pub fn operation(&self, operation_id: CtfOperationId) -> Option<&CtfOperation> {
        self.operations.get(&operation_id)
    }

    pub fn operation_by_relayer_transaction_id(
        &self,
        relayer_transaction_id: &str,
    ) -> Option<&CtfOperation> {
        self.relayer_index
            .get(relayer_transaction_id)
            .and_then(|operation_id| self.operations.get(operation_id))
    }
}
