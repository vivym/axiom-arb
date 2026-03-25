use domain::{ConditionId, EventFamilyId, ExecutionRequest, TokenId};
use rust_decimal::Decimal;

use crate::plans::{ExecutionPlan, NegRiskMemberOrderPlan};

pub const ROUTE: &str = "neg-risk";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NegRiskMemberTarget {
    pub condition_id: ConditionId,
    pub token_id: TokenId,
    pub price: Decimal,
    pub quantity: Decimal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NegRiskFamilyTarget {
    pub family_id: EventFamilyId,
    pub members: Vec<NegRiskMemberTarget>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NegRiskPlanningError {
    RouteMismatch {
        route: String,
    },
    ScopeMismatch {
        request_scope: String,
        family_id: EventFamilyId,
    },
}

pub fn plan_family_submission(
    request: &ExecutionRequest,
    config: &NegRiskFamilyTarget,
) -> Result<ExecutionPlan, NegRiskPlanningError> {
    if request.route != ROUTE {
        return Err(NegRiskPlanningError::RouteMismatch {
            route: request.route.clone(),
        });
    }

    if request.scope != config.family_id.as_str() {
        return Err(NegRiskPlanningError::ScopeMismatch {
            request_scope: request.scope.clone(),
            family_id: config.family_id.clone(),
        });
    }

    Ok(ExecutionPlan::NegRiskSubmitFamily {
        family_id: config.family_id.clone(),
        members: config
            .members
            .iter()
            .map(|member| NegRiskMemberOrderPlan {
                condition_id: member.condition_id.clone(),
                token_id: member.token_id.clone(),
                price: member.price,
                quantity: member.quantity,
            })
            .collect(),
    })
}
