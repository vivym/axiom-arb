mod bootstrap;
mod conventions;
pub mod metrics;
mod tracing_bootstrap;

pub use bootstrap::{bootstrap_observability, BootstrappedObservability};
pub use conventions::{field_keys, metric_dimensions, span_names};
pub use metrics::{
    CounterHandle, CounterSampleWithDimensions, DimensionedCounterHandle, GaugeHandle,
    MetricDimension, MetricDimensions, MetricKey, MetricRegistry, MetricRegistrySnapshot,
    ModeHandle, RuntimeMetrics, RuntimeMetricsRecorder,
};
#[doc(hidden)]
pub use tracing_bootstrap::{bootstrap_tracing, TracingBootstrap};

/// Shared observability state owned by [`bootstrap_observability`].
#[derive(Debug, Clone)]
pub struct Observability {
    service_name: String,
    metrics: RuntimeMetrics,
    registry: MetricRegistry,
}

impl Observability {
    #[doc(hidden)]
    pub fn new(service_name: impl Into<String>) -> Self {
        Self {
            service_name: service_name.into(),
            metrics: RuntimeMetrics::default(),
            registry: MetricRegistry::default(),
        }
    }

    pub fn service_name(&self) -> &str {
        &self.service_name
    }

    pub fn metrics(&self) -> &RuntimeMetrics {
        &self.metrics
    }

    pub fn registry(&self) -> &MetricRegistry {
        &self.registry
    }

    pub fn recorder(&self) -> RuntimeMetricsRecorder {
        RuntimeMetricsRecorder::new(self.metrics, self.registry.clone())
    }
}
