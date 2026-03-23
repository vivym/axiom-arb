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
}

#[derive(Debug, Clone, Default)]
pub struct IdentifierMap {
    token_to_condition: HashMap<TokenId, ConditionId>,
    condition_to_route: HashMap<ConditionId, MarketRoute>,
}

impl IdentifierMap {
    pub fn new<T, C>(token_conditions: T, condition_routes: C) -> Result<Self, IdentifierMapError>
    where
        T: IntoIterator,
        T::Item: IntoTokenConditionPair,
        C: IntoIterator,
        C::Item: IntoConditionRoutePair,
    {
        let mut token_to_condition: HashMap<TokenId, ConditionId> = HashMap::new();
        for (token_id, condition_id) in token_conditions
            .into_iter()
            .map(IntoTokenConditionPair::into_pair)
        {
            if let Some(existing_condition_id) = token_to_condition.get(&token_id) {
                if existing_condition_id != &condition_id {
                    return Err(IdentifierMapError::ConflictingTokenCondition {
                        token_id,
                        existing_condition_id: existing_condition_id.clone(),
                        new_condition_id: condition_id,
                    });
                }
            } else {
                token_to_condition.insert(token_id, condition_id);
            }
        }

        let mut condition_to_route: HashMap<ConditionId, MarketRoute> = HashMap::new();
        for (condition_id, route) in condition_routes
            .into_iter()
            .map(IntoConditionRoutePair::into_pair)
        {
            if let Some(existing_route) = condition_to_route.get(&condition_id) {
                if existing_route != &route {
                    return Err(IdentifierMapError::ConflictingConditionRoute {
                        condition_id,
                        existing_route: *existing_route,
                        new_route: route,
                    });
                }
            } else {
                condition_to_route.insert(condition_id, route);
            }
        }

        Ok(Self {
            token_to_condition,
            condition_to_route,
        })
    }

    pub fn condition_for_token(&self, token_id: &TokenId) -> Option<&ConditionId> {
        self.token_to_condition.get(token_id)
    }

    pub fn route_for_condition(&self, condition_id: &ConditionId) -> Option<MarketRoute> {
        self.condition_to_route.get(condition_id).copied()
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
