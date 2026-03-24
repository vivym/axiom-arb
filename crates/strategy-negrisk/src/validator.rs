use domain::{FamilyExclusionReason, MarketRoute, NegRiskVariant};

use crate::graph::NegRiskGraphFamily;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FamilyValidationStatus {
    Included,
    Excluded,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FamilyValidation {
    pub family_id: String,
    pub status: FamilyValidationStatus,
    pub reason: Option<FamilyExclusionReason>,
    pub discovery_revision: i64,
    pub metadata_snapshot_hash: String,
    pub member_count: usize,
}

pub fn validate_family(
    family: &NegRiskGraphFamily,
    discovery_revision: i64,
    metadata_snapshot_hash: &str,
) -> FamilyValidation {
    let reason = if family.family.route != MarketRoute::NegRisk {
        Some(FamilyExclusionReason::NonNegRiskRoute)
    } else if family.neg_risk_variant == NegRiskVariant::Augmented {
        Some(FamilyExclusionReason::AugmentedVariant)
    } else if family
        .family
        .members
        .iter()
        .any(|member| member.is_placeholder)
    {
        Some(FamilyExclusionReason::PlaceholderOutcome)
    } else if family.family.members.iter().any(|member| member.is_other) {
        Some(FamilyExclusionReason::OtherOutcome)
    } else if !family.family.members.iter().any(has_named_outcome) {
        Some(FamilyExclusionReason::MissingNamedOutcomes)
    } else {
        None
    };

    FamilyValidation {
        family_id: family.family.family_id.as_str().to_owned(),
        status: if reason.is_some() {
            FamilyValidationStatus::Excluded
        } else {
            FamilyValidationStatus::Included
        },
        reason,
        discovery_revision,
        metadata_snapshot_hash: metadata_snapshot_hash.to_owned(),
        member_count: family.family.members.len(),
    }
}

fn has_named_outcome(member: &domain::NegRiskNode) -> bool {
    !member.is_placeholder && !member.is_other && !member.outcome_label.trim().is_empty()
}
