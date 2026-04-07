use super::{RouteAdapter, RouteScopeError};

pub const ROUTE: &str = "full-set";

#[derive(Debug, Default)]
pub struct FullSetRouteAdapter;

impl RouteAdapter for FullSetRouteAdapter {
    fn route(&self) -> &'static str {
        ROUTE
    }

    fn validate_scope(&self, scope: &str) -> Result<(), RouteScopeError> {
        if scope == "default" {
            Ok(())
        } else {
            Err(RouteScopeError::new(
                ROUTE,
                scope,
                "full-set only supports the default scope",
            ))
        }
    }
}
