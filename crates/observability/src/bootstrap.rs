use crate::{
    bootstrap_tracing, MetricRegistry, Observability, RuntimeMetrics, RuntimeMetricsRecorder,
    TracingBootstrap,
};

/// Preferred repo-owned observability bootstrap surface.
#[derive(Debug, Clone)]
pub struct BootstrappedObservability {
    tracing: TracingBootstrap,
    observability: Observability,
}

impl BootstrappedObservability {
    pub fn observability(&self) -> &Observability {
        &self.observability
    }

    pub fn tracing(&self) -> &TracingBootstrap {
        &self.tracing
    }

    pub fn service_name(&self) -> &str {
        self.observability().service_name()
    }

    pub fn metrics(&self) -> &RuntimeMetrics {
        self.observability().metrics()
    }

    pub fn registry(&self) -> &MetricRegistry {
        self.observability().registry()
    }

    pub fn recorder(&self) -> RuntimeMetricsRecorder {
        self.observability().recorder()
    }
}

/// Initializes tracing and returns the repo-owned observability context for a service.
pub fn bootstrap_observability(service_name: impl Into<String>) -> BootstrappedObservability {
    let service_name = service_name.into();
    let tracing = bootstrap_tracing(service_name.clone());
    let observability = Observability::new(service_name);

    BootstrappedObservability {
        tracing,
        observability,
    }
}
