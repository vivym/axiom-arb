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

#[derive(Debug, Clone, Default)]
pub struct IdentifierMap {
    token_to_condition: HashMap<TokenId, ConditionId>,
    condition_to_route: HashMap<ConditionId, MarketRoute>,
}

impl IdentifierMap {
    pub fn new<T, C>(token_conditions: T, condition_routes: C) -> Self
    where
        T: IntoIterator,
        T::Item: IntoTokenConditionPair,
        C: IntoIterator,
        C::Item: IntoConditionRoutePair,
    {
        let token_to_condition = token_conditions
            .into_iter()
            .map(IntoTokenConditionPair::into_pair)
            .collect();
        let condition_to_route = condition_routes
            .into_iter()
            .map(IntoConditionRoutePair::into_pair)
            .collect();

        Self {
            token_to_condition,
            condition_to_route,
        }
    }

    pub fn condition_for_token(&self, token_id: &str) -> Option<&str> {
        self.token_to_condition
            .get(&TokenId::from(token_id))
            .map(ConditionId::as_str)
    }

    pub fn route_for_condition(&self, condition_id: &str) -> MarketRoute {
        self.condition_to_route
            .get(&ConditionId::from(condition_id))
            .copied()
            .expect("condition route should exist")
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
