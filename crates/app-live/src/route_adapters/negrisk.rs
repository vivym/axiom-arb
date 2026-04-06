use super::RouteAdapter;

pub const ROUTE: &str = "neg-risk";

#[derive(Debug, Default)]
pub struct NegRiskRouteAdapter;

impl RouteAdapter for NegRiskRouteAdapter {
    fn route(&self) -> &'static str {
        ROUTE
    }
}
