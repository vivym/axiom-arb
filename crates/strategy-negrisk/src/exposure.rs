use std::collections::HashMap;

use domain::{
    ConditionId, IdentifierMap, InventoryBucket, MarketRoute, NegRiskExposureError, TokenId,
};
use rust_decimal::Decimal;

use crate::graph::NegRiskGraphFamily;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FamilyExposure {
    pub family_id: domain::EventFamilyId,
    pub members: Vec<FamilyMemberExposure>,
    pub rollup: FamilyExposureRollup,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FamilyMemberExposure {
    pub condition_id: ConditionId,
    pub token_id: TokenId,
    pub outcome_label: String,
    pub bucket_quantities: HashMap<InventoryBucket, Decimal>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FamilyExposureRollup {
    pub bucket_quantities: HashMap<InventoryBucket, Decimal>,
    pub member_count: usize,
}

pub fn reconstruct_family_exposure(
    family: &NegRiskGraphFamily,
    inventory_rows: Vec<(TokenId, InventoryBucket, Decimal)>,
    identifier_map: &IdentifierMap,
) -> Result<FamilyExposure, NegRiskExposureError> {
    let family_id = family.family.family_id.clone();
    let family_tokens: HashMap<_, _> = family
        .family
        .members
        .iter()
        .map(|member| (member.token_id.clone(), member))
        .collect();

    let mut quantities_by_token = HashMap::<TokenId, HashMap<InventoryBucket, Decimal>>::new();
    let mut condition_by_token = HashMap::<TokenId, ConditionId>::new();

    for (token_id, bucket, quantity) in inventory_rows {
        if !family_tokens.contains_key(&token_id) {
            continue;
        }

        let Some(condition_id) = identifier_map.condition_for_token(&token_id).cloned() else {
            return Err(NegRiskExposureError::MissingTokenCondition { token_id });
        };

        let route = identifier_map.route_for_condition(&condition_id);
        if route != Some(MarketRoute::NegRisk) {
            return Err(NegRiskExposureError::InvalidRoute { token_id, route });
        }

        let Some(mapped_family_id) = identifier_map.family_for_condition(&condition_id).cloned()
        else {
            return Err(NegRiskExposureError::MissingFamilyMapping {
                token_id,
                condition_id,
            });
        };

        if mapped_family_id != family_id {
            return Err(NegRiskExposureError::MixedFamily {
                token_id,
                expected_family_id: family_id.clone(),
                found_family_id: mapped_family_id,
            });
        }

        condition_by_token.insert(token_id.clone(), condition_id);
        quantities_by_token
            .entry(token_id)
            .or_default()
            .entry(bucket)
            .and_modify(|existing| *existing += quantity)
            .or_insert(quantity);
    }

    if quantities_by_token.is_empty() {
        return Err(NegRiskExposureError::EmptyInventory);
    }

    let mut members = Vec::new();
    for member in &family.family.members {
        let Some(bucket_quantities) = quantities_by_token.remove(&member.token_id) else {
            continue;
        };
        let condition_id = condition_by_token
            .remove(&member.token_id)
            .expect("condition id should be present for each bucket row");

        members.push(FamilyMemberExposure {
            condition_id,
            token_id: member.token_id.clone(),
            outcome_label: member.outcome_label.clone(),
            bucket_quantities,
        });
    }

    let mut rollup = FamilyExposureRollup {
        member_count: members.len(),
        ..FamilyExposureRollup::default()
    };
    for member in &members {
        for (bucket, quantity) in &member.bucket_quantities {
            *rollup
                .bucket_quantities
                .entry(*bucket)
                .or_insert_with(|| Decimal::new(0, 0)) += *quantity;
        }
    }

    Ok(FamilyExposure {
        family_id,
        members,
        rollup,
    })
}
