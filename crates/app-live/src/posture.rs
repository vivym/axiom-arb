#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupervisorPosture {
    Healthy,
    DegradedIngress,
    DegradedDispatch,
    GlobalHalt,
}

impl SupervisorPosture {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Healthy => "healthy",
            Self::DegradedIngress => "degraded_ingress",
            Self::DegradedDispatch => "degraded_dispatch",
            Self::GlobalHalt => "global_halt",
        }
    }

    pub const fn is_global(self) -> bool {
        true
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeRestrictionKind {
    ReconcilingOnly,
    RecoveryOnly,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeRestriction {
    scope_id: String,
    kind: ScopeRestrictionKind,
}

impl ScopeRestriction {
    pub fn reconciling_only(scope_id: impl Into<String>) -> Self {
        Self {
            scope_id: scope_id.into(),
            kind: ScopeRestrictionKind::ReconcilingOnly,
        }
    }

    pub fn recovery_only(scope_id: impl Into<String>) -> Self {
        Self {
            scope_id: scope_id.into(),
            kind: ScopeRestrictionKind::RecoveryOnly,
        }
    }

    pub fn scope_id(&self) -> &str {
        &self.scope_id
    }

    pub fn kind(&self) -> ScopeRestrictionKind {
        self.kind
    }
}
