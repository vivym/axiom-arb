pub mod fullset;
pub mod negrisk;

pub trait RouteAdapter: Send + Sync {
    fn route(&self) -> &'static str;
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
