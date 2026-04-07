use super::{RouteAdapter, RouteScopeError};

pub const ROUTE: &str = "neg-risk";

#[derive(Debug, Default)]
pub struct NegRiskRouteAdapter;

impl RouteAdapter for NegRiskRouteAdapter {
    fn route(&self) -> &'static str {
        ROUTE
    }

    fn validate_scope(&self, scope: &str) -> Result<(), RouteScopeError> {
        if scope.trim().is_empty() {
            Err(RouteScopeError::new(
                ROUTE,
                scope,
                "neg-risk scope must be a family id or the default fallback",
            ))
        } else {
            Ok(())
        }
    }
}
