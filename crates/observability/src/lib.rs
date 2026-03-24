pub mod metrics;

pub use metrics::{CounterHandle, GaugeHandle, MetricKey, ModeHandle, RuntimeMetrics};

#[derive(Debug, Clone)]
pub struct Observability {
    service_name: String,
    metrics: RuntimeMetrics,
}

impl Observability {
    pub fn new(service_name: impl Into<String>) -> Self {
        Self {
            service_name: service_name.into(),
            metrics: RuntimeMetrics::default(),
        }
    }

    pub fn service_name(&self) -> &str {
        &self.service_name
    }

    pub fn metrics(&self) -> &RuntimeMetrics {
        &self.metrics
    }
}
