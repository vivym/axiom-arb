use std::sync::OnceLock;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TracingBootstrap {
    service_name: String,
}

impl TracingBootstrap {
    pub fn service_name(&self) -> &str {
        &self.service_name
    }
}

pub fn bootstrap_tracing(service_name: impl Into<String>) -> TracingBootstrap {
    static TRACING_BOOTSTRAPPED: OnceLock<()> = OnceLock::new();

    let service_name = service_name.into();
    TRACING_BOOTSTRAPPED.get_or_init(|| {
        let subscriber = tracing_subscriber::fmt()
            .with_target(false)
            .without_time()
            .finish();
        let _ = ::tracing::subscriber::set_global_default(subscriber);
        ::tracing::info!(service_name = %service_name, "tracing bootstrapped");
    });

    TracingBootstrap { service_name }
}
