use crate::route_adapters::{
    fullset::FullSetRouteAdapter, negrisk::NegRiskRouteAdapter, RouteRegistry,
};

pub fn live_route_registry() -> RouteRegistry {
    let mut registry = RouteRegistry::new();
    registry.register(Box::new(FullSetRouteAdapter));
    registry.register(Box::new(NegRiskRouteAdapter));
    registry
}
