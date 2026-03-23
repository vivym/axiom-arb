use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EventId(String);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EventFamilyId(String);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MarketId(String);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ConditionId(String);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TokenId(String);

macro_rules! impl_identifier {
    ($name:ident) => {
        impl $name {
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self::new(value)
            }
        }

        impl From<String> for $name {
            fn from(value: String) -> Self {
                Self::new(value)
            }
        }
    };
}

impl_identifier!(EventId);
impl_identifier!(EventFamilyId);
impl_identifier!(MarketId);
impl_identifier!(ConditionId);
impl_identifier!(TokenId);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    pub event_id: EventId,
    pub family_id: EventFamilyId,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventFamily {
    pub family_id: EventFamilyId,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Condition {
    pub condition_id: ConditionId,
    pub market_id: MarketId,
    pub event_id: EventId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Market {
    pub market_id: MarketId,
    pub condition_id: ConditionId,
    pub route: MarketRoute,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub token_id: TokenId,
    pub condition_id: ConditionId,
    pub outcome_label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentifierRecord {
    pub event_id: EventId,
    pub event_family_id: EventFamilyId,
    pub market_id: MarketId,
    pub condition_id: ConditionId,
    pub token_id: TokenId,
    pub outcome_label: String,
    pub route: MarketRoute,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarketRoute {
    Standard,
    NegRisk,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdentifierMapError {
    ConflictingTokenCondition {
        token_id: TokenId,
        existing_condition_id: ConditionId,
        new_condition_id: ConditionId,
    },
    ConflictingConditionRoute {
        condition_id: ConditionId,
        existing_route: MarketRoute,
        new_route: MarketRoute,
    },
    ConflictingTokenMetadata {
        token_id: TokenId,
    },
    ConflictingConditionMetadata {
        condition_id: ConditionId,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TokenMetadata {
    condition_id: ConditionId,
    outcome_label: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct ConditionMetadata {
    market_id: Option<MarketId>,
    event_id: Option<EventId>,
    event_family_id: Option<EventFamilyId>,
    route: Option<MarketRoute>,
}

#[derive(Debug, Clone, Default)]
pub struct IdentifierMap {
    token_to_metadata: HashMap<TokenId, TokenMetadata>,
    condition_to_metadata: HashMap<ConditionId, ConditionMetadata>,
}

impl IdentifierMap {
    pub fn new<T, C>(token_conditions: T, condition_routes: C) -> Result<Self, IdentifierMapError>
    where
        T: IntoIterator,
        T::Item: IntoTokenConditionPair,
        C: IntoIterator,
        C::Item: IntoConditionRoutePair,
    {
        let mut map = Self::default();

        for (token_id, condition_id) in token_conditions
            .into_iter()
            .map(IntoTokenConditionPair::into_pair)
        {
            map.insert_token_metadata(
                token_id,
                TokenMetadata {
                    condition_id,
                    outcome_label: None,
                },
            )?;
        }

        for (condition_id, route) in condition_routes
            .into_iter()
            .map(IntoConditionRoutePair::into_pair)
        {
            map.merge_condition_metadata(
                condition_id,
                ConditionMetadata {
                    route: Some(route),
                    ..ConditionMetadata::default()
                },
            )?;
        }

        Ok(map)
    }

    pub fn from_records<R>(records: R) -> Result<Self, IdentifierMapError>
    where
        R: IntoIterator<Item = IdentifierRecord>,
    {
        let mut map = Self::default();

        for record in records {
            map.insert_token_metadata(
                record.token_id.clone(),
                TokenMetadata {
                    condition_id: record.condition_id.clone(),
                    outcome_label: Some(record.outcome_label),
                },
            )?;

            map.merge_condition_metadata(
                record.condition_id,
                ConditionMetadata {
                    market_id: Some(record.market_id),
                    event_id: Some(record.event_id),
                    event_family_id: Some(record.event_family_id),
                    route: Some(record.route),
                },
            )?;
        }

        Ok(map)
    }

    pub fn condition_for_token(&self, token_id: &TokenId) -> Option<&ConditionId> {
        self.token_to_metadata
            .get(token_id)
            .map(|metadata| &metadata.condition_id)
    }

    pub fn outcome_label_for_token(&self, token_id: &TokenId) -> Option<&str> {
        self.token_to_metadata
            .get(token_id)
            .and_then(|metadata| metadata.outcome_label.as_deref())
    }

    pub fn market_for_condition(&self, condition_id: &ConditionId) -> Option<&MarketId> {
        self.condition_to_metadata
            .get(condition_id)
            .and_then(|metadata| metadata.market_id.as_ref())
    }

    pub fn event_for_condition(&self, condition_id: &ConditionId) -> Option<&EventId> {
        self.condition_to_metadata
            .get(condition_id)
            .and_then(|metadata| metadata.event_id.as_ref())
    }

    pub fn family_for_condition(&self, condition_id: &ConditionId) -> Option<&EventFamilyId> {
        self.condition_to_metadata
            .get(condition_id)
            .and_then(|metadata| metadata.event_family_id.as_ref())
    }

    pub fn route_for_condition(&self, condition_id: &ConditionId) -> Option<MarketRoute> {
        self.condition_to_metadata
            .get(condition_id)
            .and_then(|metadata| metadata.route)
    }

    fn insert_token_metadata(
        &mut self,
        token_id: TokenId,
        metadata: TokenMetadata,
    ) -> Result<(), IdentifierMapError> {
        if let Some(existing_metadata) = self.token_to_metadata.get(&token_id) {
            if existing_metadata.condition_id != metadata.condition_id {
                return Err(IdentifierMapError::ConflictingTokenCondition {
                    token_id,
                    existing_condition_id: existing_metadata.condition_id.clone(),
                    new_condition_id: metadata.condition_id,
                });
            }

            if existing_metadata != &metadata {
                return Err(IdentifierMapError::ConflictingTokenMetadata { token_id });
            }

            return Ok(());
        }

        self.token_to_metadata.insert(token_id, metadata);
        Ok(())
    }

    fn merge_condition_metadata(
        &mut self,
        condition_id: ConditionId,
        incoming: ConditionMetadata,
    ) -> Result<(), IdentifierMapError> {
        if let Some(existing) = self.condition_to_metadata.get(&condition_id) {
            if let (Some(existing_route), Some(new_route)) = (existing.route, incoming.route) {
                if existing_route != new_route {
                    return Err(IdentifierMapError::ConflictingConditionRoute {
                        condition_id,
                        existing_route,
                        new_route,
                    });
                }
            }

            if merge_metadata(existing.clone(), incoming.clone()).is_none() {
                return Err(IdentifierMapError::ConflictingConditionMetadata { condition_id });
            }
        }

        let merged = match self.condition_to_metadata.remove(&condition_id) {
            Some(existing) => merge_metadata(existing, incoming)
                .expect("condition metadata conflicts should be rejected before merge"),
            None => incoming,
        };

        self.condition_to_metadata.insert(condition_id, merged);
        Ok(())
    }
}

fn merge_metadata(
    existing: ConditionMetadata,
    incoming: ConditionMetadata,
) -> Option<ConditionMetadata> {
    Some(ConditionMetadata {
        market_id: merge_optional(existing.market_id, incoming.market_id)?,
        event_id: merge_optional(existing.event_id, incoming.event_id)?,
        event_family_id: merge_optional(existing.event_family_id, incoming.event_family_id)?,
        route: merge_optional(existing.route, incoming.route)?,
    })
}

fn merge_optional<T>(existing: Option<T>, incoming: Option<T>) -> Option<Option<T>>
where
    T: PartialEq,
{
    match (existing, incoming) {
        (Some(existing), Some(incoming)) if existing != incoming => None,
        (Some(existing), Some(_)) => Some(Some(existing)),
        (Some(existing), None) => Some(Some(existing)),
        (None, Some(incoming)) => Some(Some(incoming)),
        (None, None) => Some(None),
    }
}

pub trait IntoTokenConditionPair {
    fn into_pair(self) -> (TokenId, ConditionId);
}

impl IntoTokenConditionPair for (&str, &str) {
    fn into_pair(self) -> (TokenId, ConditionId) {
        (TokenId::from(self.0), ConditionId::from(self.1))
    }
}

impl IntoTokenConditionPair for (TokenId, ConditionId) {
    fn into_pair(self) -> (TokenId, ConditionId) {
        self
    }
}

pub trait IntoConditionRoutePair {
    fn into_pair(self) -> (ConditionId, MarketRoute);
}

impl IntoConditionRoutePair for (&str, MarketRoute) {
    fn into_pair(self) -> (ConditionId, MarketRoute) {
        (ConditionId::from(self.0), self.1)
    }
}

impl IntoConditionRoutePair for (ConditionId, MarketRoute) {
    fn into_pair(self) -> (ConditionId, MarketRoute) {
        self
    }
}
