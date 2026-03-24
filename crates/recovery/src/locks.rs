#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecoveryScopeLock {
    Market(String),
    Condition(String),
    Family(String),
    InventorySet(String),
    ExecutionPath(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RecoveryScopeKind {
    Market,
    Condition,
    Family,
    InventorySet,
    ExecutionPath,
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

    pub fn blocks_expansion(&self, candidate: &RecoveryScopeLock) -> bool {
        self.scope_kind() == candidate.scope_kind()
            && scope_blocks(self.scope_id(), candidate.scope_id())
    }

    fn scope_kind(&self) -> RecoveryScopeKind {
        match self {
            Self::Market(_) => RecoveryScopeKind::Market,
            Self::Condition(_) => RecoveryScopeKind::Condition,
            Self::Family(_) => RecoveryScopeKind::Family,
            Self::InventorySet(_) => RecoveryScopeKind::InventorySet,
            Self::ExecutionPath(_) => RecoveryScopeKind::ExecutionPath,
        }
    }

    fn scope_id(&self) -> &str {
        match self {
            Self::Market(scope_id)
            | Self::Condition(scope_id)
            | Self::Family(scope_id)
            | Self::InventorySet(scope_id)
            | Self::ExecutionPath(scope_id) => scope_id,
        }
    }
}

fn scope_blocks(parent: &str, candidate: &str) -> bool {
    if candidate == parent {
        return true;
    }

    candidate
        .strip_prefix(parent)
        .is_some_and(|remainder| remainder.starts_with(':'))
}
