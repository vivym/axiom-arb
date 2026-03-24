use std::collections::HashMap;

use rust_decimal::Decimal;

use crate::{ConditionId, EventFamilyId, IdentifierMap, InventoryBucket, MarketRoute, TokenId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NegRiskFamily {
    pub family_id: EventFamilyId,
    pub route: MarketRoute,
    pub members: Vec<NegRiskNode>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NegRiskNode {
    pub token_id: TokenId,
    pub outcome_label: String,
    pub is_placeholder: bool,
    pub is_other: bool,
    pub route: MarketRoute,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FamilyExclusionReason {
    PlaceholderOutcome,
    OtherOutcome,
    AugmentedVariant,
    MissingNamedOutcomes,
    NonNegRiskRoute,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NegRiskVariant {
    Standard,
    Augmented,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NegRiskExposureError {
    EmptyInventory,
    MissingTokenCondition {
        token_id: TokenId,
    },
    MissingFamilyMapping {
        token_id: TokenId,
        condition_id: ConditionId,
    },
    InvalidRoute {
        token_id: TokenId,
        route: Option<MarketRoute>,
    },
    MixedFamily {
        token_id: TokenId,
        expected_family_id: EventFamilyId,
        found_family_id: EventFamilyId,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NegRiskExposureVector {
    pub family_id: EventFamilyId,
    pub members: Vec<NegRiskMemberExposure>,
    pub rollup: NegRiskExposureRollup,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NegRiskMemberExposure {
    pub token_id: TokenId,
    pub outcome_label: String,
    pub bucket_quantities: HashMap<InventoryBucket, Decimal>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NegRiskExposureRollup {
    pub bucket_quantities: HashMap<InventoryBucket, Decimal>,
    pub member_count: usize,
}

impl NegRiskExposureVector {
    pub fn from_inventory(
        inventory: Vec<(TokenId, InventoryBucket, Decimal)>,
        identifier_map: IdentifierMap,
    ) -> Result<Self, NegRiskExposureError> {
        if inventory.is_empty() {
            return Err(NegRiskExposureError::EmptyInventory);
        }

        let mut member_quantities = HashMap::<TokenId, NegRiskMemberExposure>::new();
        let mut family_id: Option<EventFamilyId> = None;

        for (token_id, bucket, quantity) in inventory {
            let Some(condition_id) = identifier_map.condition_for_token(&token_id).cloned() else {
                return Err(NegRiskExposureError::MissingTokenCondition { token_id });
            };

            let route = identifier_map.route_for_condition(&condition_id);
            if route != Some(MarketRoute::NegRisk) {
                return Err(NegRiskExposureError::InvalidRoute { token_id, route });
            }

            let Some(found_family_id) = identifier_map.family_for_condition(&condition_id).cloned()
            else {
                return Err(NegRiskExposureError::MissingFamilyMapping {
                    token_id,
                    condition_id,
                });
            };

            match &family_id {
                Some(expected_family_id) if *expected_family_id != found_family_id => {
                    return Err(NegRiskExposureError::MixedFamily {
                        token_id,
                        expected_family_id: expected_family_id.clone(),
                        found_family_id,
                    });
                }
                None => family_id = Some(found_family_id.clone()),
                _ => {}
            }

            let outcome_label = identifier_map
                .outcome_label_for_token(&token_id)
                .unwrap_or(token_id.as_str())
                .to_owned();

            let entry = member_quantities
                .entry(token_id.clone())
                .or_insert_with(|| NegRiskMemberExposure {
                    token_id: token_id.clone(),
                    outcome_label,
                    bucket_quantities: HashMap::new(),
                });

            entry
                .bucket_quantities
                .entry(bucket)
                .and_modify(|existing| *existing += quantity)
                .or_insert(quantity);
        }

        let mut members: Vec<_> = member_quantities.into_values().collect();
        members.sort_by(|left, right| left.token_id.as_str().cmp(right.token_id.as_str()));

        let mut rollup = NegRiskExposureRollup {
            member_count: members.len(),
            ..NegRiskExposureRollup::default()
        };

        for member in &members {
            for (bucket, quantity) in &member.bucket_quantities {
                *rollup
                    .bucket_quantities
                    .entry(*bucket)
                    .or_insert_with(|| Decimal::new(0, 0)) += *quantity;
            }
        }

        Ok(Self {
            family_id: family_id.expect("validated inventory must contain a family id"),
            members,
            rollup,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HaltPriority {
    Global,
    Family,
    MarketLocal,
    StrategyLocal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FamilyHaltPolicy {
    priority: HaltPriority,
    blocks_new_risk: bool,
}

impl FamilyHaltPolicy {
    pub fn default_block_new_risk() -> Self {
        Self {
            priority: HaltPriority::Family,
            blocks_new_risk: true,
        }
    }

    pub fn priority(&self) -> HaltPriority {
        self.priority
    }

    pub fn blocks_new_risk(&self) -> bool {
        self.blocks_new_risk
    }
}

impl Default for FamilyHaltPolicy {
    fn default() -> Self {
        Self::default_block_new_risk()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FamilyHaltStatus {
    ActiveBlocking,
    StaleBlocking,
    Cleared,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FamilyHaltState {
    pub family_id: EventFamilyId,
    metadata_snapshot_hash: String,
    status: FamilyHaltStatus,
}

impl FamilyHaltState {
    pub fn active(
        family_id: impl Into<EventFamilyId>,
        metadata_snapshot_hash: impl Into<String>,
    ) -> Self {
        Self {
            family_id: family_id.into(),
            metadata_snapshot_hash: metadata_snapshot_hash.into(),
            status: FamilyHaltStatus::ActiveBlocking,
        }
    }

    pub fn reconcile_against_snapshot_hash(
        &mut self,
        latest_snapshot_hash: &str,
    ) -> FamilyHaltStatus {
        if self.status == FamilyHaltStatus::Cleared {
            return self.status;
        }

        if self.metadata_snapshot_hash == latest_snapshot_hash {
            self.status = FamilyHaltStatus::ActiveBlocking;
        } else {
            self.status = FamilyHaltStatus::StaleBlocking;
        }

        self.status
    }

    pub fn revalidate_against_snapshot_hash(
        &mut self,
        latest_snapshot_hash: impl Into<String>,
    ) -> FamilyHaltStatus {
        self.metadata_snapshot_hash = latest_snapshot_hash.into();
        self.status = FamilyHaltStatus::ActiveBlocking;
        self.status
    }

    pub fn clear(&mut self) -> FamilyHaltStatus {
        self.status = FamilyHaltStatus::Cleared;
        self.status
    }

    pub fn metadata_snapshot_hash(&self) -> &str {
        &self.metadata_snapshot_hash
    }

    pub fn status(&self) -> FamilyHaltStatus {
        self.status
    }
}
