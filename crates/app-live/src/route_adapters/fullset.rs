use super::RouteAdapter;

pub const ROUTE: &str = "full-set";

#[derive(Debug, Default)]
pub struct FullSetRouteAdapter;

impl RouteAdapter for FullSetRouteAdapter {
    fn route(&self) -> &'static str {
        ROUTE
    }
}
