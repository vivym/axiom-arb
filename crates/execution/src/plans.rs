use domain::{ConditionId, EventFamilyId, OrderId, TokenId};
use rust_decimal::Decimal;

const FULL_SET_ROUTE: &str = "full-set";
const NEG_RISK_ROUTE: &str = "neg-risk";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NegRiskMemberOrderPlan {
    pub condition_id: ConditionId,
    pub token_id: TokenId,
    pub price: Decimal,
    pub quantity: Decimal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutionPlan {
    FullSetBuyThenMerge {
        condition_id: ConditionId,
    },
    FullSetSplitThenSell {
        condition_id: ConditionId,
    },
    CancelStale {
        order_id: OrderId,
    },
    RedeemResolved {
        condition_id: ConditionId,
    },
    NegRiskSubmitFamily {
        family_id: EventFamilyId,
        members: Vec<NegRiskMemberOrderPlan>,
    },
}

impl ExecutionPlan {
    pub fn route(&self) -> Option<&'static str> {
        match self {
            Self::FullSetBuyThenMerge { .. }
            | Self::FullSetSplitThenSell { .. }
            | Self::RedeemResolved { .. } => Some(FULL_SET_ROUTE),
            Self::NegRiskSubmitFamily { .. } => Some(NEG_RISK_ROUTE),
            Self::CancelStale { .. } => None,
        }
    }

    pub fn plan_id(&self) -> String {
        match self {
            Self::FullSetBuyThenMerge { condition_id } => {
                format!("fullset-buy-merge:{}", condition_id.as_str())
            }
            Self::FullSetSplitThenSell { condition_id } => {
                format!("fullset-split-sell:{}", condition_id.as_str())
            }
            Self::CancelStale { order_id } => format!("cancel-stale:{}", order_id.as_str()),
            Self::RedeemResolved { condition_id } => {
                format!("redeem-resolved:{}", condition_id.as_str())
            }
            Self::NegRiskSubmitFamily { family_id, members } => {
                let mut canonical_members: Vec<_> = members.iter().collect();
                canonical_members.sort_by(|left, right| {
                    left.condition_id
                        .as_str()
                        .cmp(right.condition_id.as_str())
                        .then_with(|| left.token_id.as_str().cmp(right.token_id.as_str()))
                        .then_with(|| {
                            left.price
                                .normalize()
                                .to_string()
                                .cmp(&right.price.normalize().to_string())
                        })
                        .then_with(|| {
                            left.quantity
                                .normalize()
                                .to_string()
                                .cmp(&right.quantity.normalize().to_string())
                        })
                });

                format!(
                    "negrisk-submit-family:{}:{}",
                    family_id.as_str(),
                    canonical_members
                        .into_iter()
                        .map(|member| format!(
                            "{}:{}:{}:{}",
                            member.condition_id.as_str(),
                            member.token_id.as_str(),
                            member.price.normalize(),
                            member.quantity.normalize()
                        ))
                        .collect::<Vec<_>>()
                        .join("|")
                )
            }
        }
    }

    pub fn condition_id(&self) -> Option<&ConditionId> {
        match self {
            Self::FullSetBuyThenMerge { condition_id }
            | Self::FullSetSplitThenSell { condition_id }
            | Self::RedeemResolved { condition_id } => Some(condition_id),
            Self::CancelStale { .. } | Self::NegRiskSubmitFamily { .. } => None,
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

    pub fn is_risk_expanding(&self) -> bool {
        matches!(
            self,
            Self::FullSetBuyThenMerge { .. }
                | Self::FullSetSplitThenSell { .. }
                | Self::NegRiskSubmitFamily { .. }
        )
    }
}
