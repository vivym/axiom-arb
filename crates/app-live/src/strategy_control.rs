use std::fmt;

use crate::route_adapters::{
    fullset::FullSetRouteAdapter, negrisk::NegRiskRouteAdapter, RouteRegistry,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteArtifactValidationError {
    message: String,
}

impl RouteArtifactValidationError {
    fn unknown_route(route: &str) -> Self {
        Self {
            message: format!("unregistered route_artifacts route {route}"),
        }
    }

    fn invalid_scope(route: &str, scope: &str, reason: &str) -> Self {
        Self {
            message: format!("route {route} scope {scope} is invalid: {reason}"),
        }
    }
}

impl fmt::Display for RouteArtifactValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for RouteArtifactValidationError {}

pub fn live_route_registry() -> RouteRegistry {
    let mut registry = RouteRegistry::new();
    registry.register(Box::new(FullSetRouteAdapter));
    registry.register(Box::new(NegRiskRouteAdapter));
    registry
}

pub fn validate_live_route_scope(
    route: &str,
    scope: &str,
) -> Result<(), RouteArtifactValidationError> {
    let registry = live_route_registry();
    let Some(adapter) = registry.adapter(route) else {
        return Err(RouteArtifactValidationError::unknown_route(route));
    };
    adapter
        .validate_scope(scope)
        .map_err(|error| RouteArtifactValidationError::invalid_scope(route, scope, &error.reason))
}
