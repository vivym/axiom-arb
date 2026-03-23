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
        if self.mode == RuntimeMode::GlobalHalt {
            return self;
        }

        match (venue_status, account_status) {
            (VenueTradingStatus::TradingDisabled, _)
            | (_, AccountTradingStatus::Geoblocked | AccountTradingStatus::Banned) => Self {
                mode: RuntimeMode::GlobalHalt,
                overlay: None,
            },
            _ => {
                let constrained_overlay = merge_overlay(
                    self.effective_overlay(),
                    external_overlay(venue_status, account_status),
                );
                let constrained_mode = constrain_mode(self.mode, venue_status, account_status);

                Self {
                    mode: constrained_mode,
                    overlay: constrained_overlay,
                }
            }
        }
    }

    fn effective_overlay(self) -> Option<RuntimeOverlay> {
        self.overlay.or(self.mode.default_overlay())
    }
}

fn constrain_mode(
    current_mode: RuntimeMode,
    venue_status: VenueTradingStatus,
    account_status: AccountTradingStatus,
) -> RuntimeMode {
    match current_mode {
        RuntimeMode::Bootstrapping
        | RuntimeMode::Reconciling
        | RuntimeMode::Degraded
        | RuntimeMode::NoNewRisk => current_mode,
        RuntimeMode::Healthy
            if matches!(venue_status, VenueTradingStatus::CancelOnly)
                || matches!(account_status, AccountTradingStatus::CloseOnly) =>
        {
            RuntimeMode::NoNewRisk
        }
        RuntimeMode::Healthy => RuntimeMode::Healthy,
        RuntimeMode::GlobalHalt => RuntimeMode::GlobalHalt,
    }
}

fn external_overlay(
    venue_status: VenueTradingStatus,
    account_status: AccountTradingStatus,
) -> Option<RuntimeOverlay> {
    match (venue_status, account_status) {
        (VenueTradingStatus::CancelOnly, _) => Some(RuntimeOverlay::CancelOnly),
        (_, AccountTradingStatus::CloseOnly) => Some(RuntimeOverlay::ReduceOnly),
        _ => None,
    }
}

fn merge_overlay(
    current_overlay: Option<RuntimeOverlay>,
    incoming_overlay: Option<RuntimeOverlay>,
) -> Option<RuntimeOverlay> {
    match (current_overlay, incoming_overlay) {
        (Some(RuntimeOverlay::CancelOnly), _) | (_, Some(RuntimeOverlay::CancelOnly)) => {
            Some(RuntimeOverlay::CancelOnly)
        }
        (None, overlay) | (overlay, None) => overlay,
        (Some(left), Some(right)) if left == right => Some(left),
        (Some(_), Some(_)) => Some(RuntimeOverlay::CancelOnly),
    }
}
