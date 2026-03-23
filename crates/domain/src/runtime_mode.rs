#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeMode {
    Bootstrapping,
    Healthy,
    Reconciling,
    Degraded,
    NoNewRisk,
    GlobalHalt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeOverlay {
    ReduceOnly,
    InventoryOnly,
    CancelOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VenueTradingStatus {
    TradingEnabled,
    TradingDisabled,
    CancelOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountTradingStatus {
    Normal,
    CloseOnly,
    Geoblocked,
    Banned,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimePolicy {
    pub mode: RuntimeMode,
    pub overlay: Option<RuntimeOverlay>,
}

impl RuntimeMode {
    pub fn default_overlay(&self) -> Option<RuntimeOverlay> {
        match self {
            RuntimeMode::Bootstrapping => Some(RuntimeOverlay::CancelOnly),
            _ => None,
        }
    }
}

impl RuntimePolicy {
    pub fn constrained_by(
        self,
        venue_status: VenueTradingStatus,
        account_status: AccountTradingStatus,
    ) -> Self {
        match (venue_status, account_status) {
            (VenueTradingStatus::TradingDisabled, _)
            | (_, AccountTradingStatus::Geoblocked | AccountTradingStatus::Banned) => Self {
                mode: RuntimeMode::GlobalHalt,
                overlay: None,
            },
            (VenueTradingStatus::CancelOnly, _) => Self {
                mode: RuntimeMode::NoNewRisk,
                overlay: Some(RuntimeOverlay::CancelOnly),
            },
            (_, AccountTradingStatus::CloseOnly) => Self {
                mode: RuntimeMode::NoNewRisk,
                overlay: Some(RuntimeOverlay::ReduceOnly),
            },
            _ => self,
        }
    }
}
