use std::{
    collections::{HashMap, HashSet},
    fmt,
};

use domain::{
    EventFamilyId, IdentifierMap, IdentifierMapError, IdentifierRecord, MarketRoute, NegRiskFamily,
    NegRiskNode, NegRiskVariant,
};
use venue_polymarket::NegRiskMarketMetadata;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NegRiskGraph {
    families: Vec<NegRiskGraphFamily>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NegRiskGraphFamily {
    pub family: NegRiskFamily,
    pub neg_risk_variant: NegRiskVariant,
}

impl NegRiskGraph {
    pub fn families(&self) -> &[NegRiskGraphFamily] {
        &self.families
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraphBuildError {
    InvalidIdentifierRecords(IdentifierMapError),
    DuplicateMetadataMember {
        event_family_id: String,
        condition_id: String,
        token_id: String,
    },
    MissingIdentifierRecord {
        condition_id: String,
        token_id: String,
    },
    MismatchedFamilyId {
        condition_id: String,
        record_family_id: String,
        metadata_family_id: String,
    },
    MismatchedRoute {
        condition_id: String,
        record_route: MarketRoute,
        metadata_route: MarketRoute,
    },
    MixedFamilyRoute {
        event_family_id: String,
        existing_route: MarketRoute,
        new_route: MarketRoute,
    },
}

impl fmt::Display for GraphBuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidIdentifierRecords(err) => {
                write!(f, "invalid identifier records for neg-risk graph build: {err:?}")
            }
            Self::DuplicateMetadataMember {
                event_family_id,
                condition_id,
                token_id,
            } => write!(
                f,
                "duplicate neg-risk metadata member for family {event_family_id} condition {condition_id} token {token_id}"
            ),
            Self::MissingIdentifierRecord {
                condition_id,
                token_id,
            } => write!(
                f,
                "missing identifier record for neg-risk metadata condition {condition_id} token {token_id}"
            ),
            Self::MismatchedFamilyId {
                condition_id,
                record_family_id,
                metadata_family_id,
            } => write!(
                f,
                "identifier record family {record_family_id} does not match metadata family {metadata_family_id} for condition {condition_id}"
            ),
            Self::MismatchedRoute {
                condition_id,
                record_route,
                metadata_route,
            } => write!(
                f,
                "identifier record route {record_route:?} does not match metadata route {metadata_route:?} for condition {condition_id}"
            ),
            Self::MixedFamilyRoute {
                event_family_id,
                existing_route,
                new_route,
            } => write!(
                f,
                "family {event_family_id} mixes routes {existing_route:?} and {new_route:?}"
            ),
        }
    }
}

impl std::error::Error for GraphBuildError {}

impl From<IdentifierMapError> for GraphBuildError {
    fn from(value: IdentifierMapError) -> Self {
        Self::InvalidIdentifierRecords(value)
    }
}

#[derive(Debug)]
struct FamilyAccumulator {
    family_id: EventFamilyId,
    route: MarketRoute,
    members: Vec<NegRiskNode>,
    variant: NegRiskVariant,
}

pub fn build_family_graph(
    records: Vec<IdentifierRecord>,
    metadata: Vec<NegRiskMarketMetadata>,
) -> Result<NegRiskGraph, GraphBuildError> {
    let _ = IdentifierMap::from_records(records.clone())?;

    let mut records_by_member = HashMap::new();
    for record in records {
        let key = (record.condition_id.clone(), record.token_id.clone());
        records_by_member.entry(key).or_insert(record);
    }

    let mut family_order = Vec::<EventFamilyId>::new();
    let mut accumulators = HashMap::<EventFamilyId, FamilyAccumulator>::new();
    let mut seen_metadata_members = HashSet::new();

    for row in metadata {
        let member_key = (row.condition_id.clone(), row.token_id.clone());
        if !seen_metadata_members.insert(member_key.clone()) {
            return Err(GraphBuildError::DuplicateMetadataMember {
                event_family_id: row.event_family_id,
                condition_id: row.condition_id,
                token_id: row.token_id,
            });
        }

        let record = records_by_member
            .get(&(
                row.condition_id.as_str().into(),
                row.token_id.as_str().into(),
            ))
            .ok_or_else(|| GraphBuildError::MissingIdentifierRecord {
                condition_id: row.condition_id.clone(),
                token_id: row.token_id.clone(),
            })?;

        if record.event_family_id.as_str() != row.event_family_id {
            return Err(GraphBuildError::MismatchedFamilyId {
                condition_id: row.condition_id.clone(),
                record_family_id: record.event_family_id.as_str().to_owned(),
                metadata_family_id: row.event_family_id.clone(),
            });
        }

        if record.route != row.route {
            return Err(GraphBuildError::MismatchedRoute {
                condition_id: row.condition_id.clone(),
                record_route: record.route,
                metadata_route: row.route,
            });
        }

        let family_id = record.event_family_id.clone();
        let family = if let Some(family) = accumulators.get_mut(&family_id) {
            family
        } else {
            family_order.push(family_id.clone());
            accumulators.insert(
                family_id.clone(),
                FamilyAccumulator {
                    family_id: family_id.clone(),
                    route: record.route,
                    members: Vec::new(),
                    variant: row.neg_risk_variant,
                },
            );
            accumulators
                .get_mut(&family_id)
                .expect("family accumulator should exist after insert")
        };

        if family.route != record.route {
            return Err(GraphBuildError::MixedFamilyRoute {
                event_family_id: family.family_id.as_str().to_owned(),
                existing_route: family.route,
                new_route: record.route,
            });
        }

        family.variant = merge_variant(family.variant, row.neg_risk_variant);
        family.members.push(NegRiskNode {
            token_id: row.token_id.as_str().into(),
            outcome_label: row.outcome_label,
            is_placeholder: row.is_placeholder,
            is_other: row.is_other,
            route: row.route,
        });
    }

    let mut families = Vec::with_capacity(family_order.len());
    for family_id in family_order {
        let family = accumulators
            .remove(&family_id)
            .expect("family accumulator should exist");
        families.push(NegRiskGraphFamily {
            family: NegRiskFamily {
                family_id: family.family_id,
                route: family.route,
                members: family.members,
            },
            neg_risk_variant: family.variant,
        });
    }

    Ok(NegRiskGraph { families })
}

fn merge_variant(existing: NegRiskVariant, incoming: NegRiskVariant) -> NegRiskVariant {
    match (existing, incoming) {
        (NegRiskVariant::Augmented, _) | (_, NegRiskVariant::Augmented) => {
            NegRiskVariant::Augmented
        }
        (NegRiskVariant::Unknown, _) | (_, NegRiskVariant::Unknown) => NegRiskVariant::Unknown,
        _ => NegRiskVariant::Standard,
    }
}
