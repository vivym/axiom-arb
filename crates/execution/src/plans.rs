use domain::{ConditionId, OrderId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutionPlan {
    FullSetBuyThenMerge { condition_id: ConditionId },
    FullSetSplitThenSell { condition_id: ConditionId },
    CancelStale { order_id: OrderId },
    RedeemResolved { condition_id: ConditionId },
}

impl ExecutionPlan {
    pub fn condition_id(&self) -> Option<&ConditionId> {
        match self {
            Self::FullSetBuyThenMerge { condition_id }
            | Self::FullSetSplitThenSell { condition_id }
            | Self::RedeemResolved { condition_id } => Some(condition_id),
            Self::CancelStale { .. } => None,
        }
    }

    pub fn order_id(&self) -> Option<&OrderId> {
        match self {
            Self::CancelStale { order_id } => Some(order_id),
            _ => None,
        }
    }

    pub fn is_amountless(&self) -> bool {
        matches!(self, Self::RedeemResolved { .. })
    }
}
