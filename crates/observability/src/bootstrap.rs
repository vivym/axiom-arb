use crate::{
    bootstrap_tracing, MetricRegistry, Observability, RuntimeMetrics, RuntimeMetricsRecorder,
};

#[derive(Debug, Clone)]
pub struct BootstrappedObservability {
    _tracing: crate::TracingBootstrap,
    observability: Observability,
}

impl BootstrappedObservability {
    pub fn service_name(&self) -> &str {
        self.observability.service_name()
    }

    pub fn metrics(&self) -> &RuntimeMetrics {
        self.observability.metrics()
    }

    pub fn registry(&self) -> &MetricRegistry {
        self.observability.registry()
    }

    pub fn recorder(&self) -> RuntimeMetricsRecorder {
        self.observability.recorder()
    }
}

pub fn bootstrap_observability(service_name: impl Into<String>) -> BootstrappedObservability {
    let service_name = service_name.into();
    let tracing = bootstrap_tracing(service_name.clone());
    let observability = Observability::new(service_name);

    BootstrappedObservability {
        _tracing: tracing,
        observability,
    }
}
