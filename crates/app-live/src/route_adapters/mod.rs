pub mod fullset;
pub mod negrisk;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteScopeError {
    pub route: &'static str,
    pub scope: String,
    pub reason: String,
}

impl RouteScopeError {
    pub fn new(route: &'static str, scope: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            route,
            scope: scope.into(),
            reason: reason.into(),
        }
    }
}

pub trait RouteAdapter: Send + Sync {
    fn route(&self) -> &'static str;

    fn validate_scope(&self, scope: &str) -> Result<(), RouteScopeError>;

    fn supports_scope(&self, scope: &str) -> bool {
        self.validate_scope(scope).is_ok()
    }
}

#[derive(Default)]
pub struct RouteRegistry {
    adapters: Vec<Box<dyn RouteAdapter>>,
}

impl RouteRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, adapter: Box<dyn RouteAdapter>) {
        if let Some(index) = self
            .adapters
            .iter()
            .position(|existing| existing.route() == adapter.route())
        {
            self.adapters[index] = adapter;
        } else {
            self.adapters.push(adapter);
        }
    }

    pub fn adapter(&self, route: &str) -> Option<&dyn RouteAdapter> {
        self.adapters
            .iter()
            .find(|adapter| adapter.route() == route)
            .map(|adapter| adapter.as_ref())
    }
}
