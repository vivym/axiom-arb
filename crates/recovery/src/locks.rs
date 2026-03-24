#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecoveryScopeLock {
    Market(String),
    Condition(String),
    Family(String),
    InventorySet(String),
    ExecutionPath(String),
}

impl RecoveryScopeLock {
    pub fn market(scope_id: impl Into<String>) -> Self {
        Self::Market(scope_id.into())
    }

    pub fn condition(scope_id: impl Into<String>) -> Self {
        Self::Condition(scope_id.into())
    }

    pub fn family(scope_id: impl Into<String>) -> Self {
        Self::Family(scope_id.into())
    }

    pub fn inventory_set(scope_id: impl Into<String>) -> Self {
        Self::InventorySet(scope_id.into())
    }

    pub fn execution_path(scope_id: impl Into<String>) -> Self {
        Self::ExecutionPath(scope_id.into())
    }

    pub fn blocks_expansion(&self, scope_id: &str) -> bool {
        match self {
            Self::Market(current)
            | Self::Condition(current)
            | Self::Family(current)
            | Self::InventorySet(current)
            | Self::ExecutionPath(current) => current == scope_id,
        }
    }
}
