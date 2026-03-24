use std::sync::OnceLock;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TracingBootstrap {
    service_name: String,
    initialized_global_subscriber: bool,
}

impl TracingBootstrap {
    pub fn service_name(&self) -> &str {
        &self.service_name
    }

    pub fn initialized_global_subscriber(&self) -> bool {
        self.initialized_global_subscriber
    }
}

pub fn bootstrap_tracing(service_name: impl Into<String>) -> TracingBootstrap {
    static TRACING_BOOTSTRAPPED: OnceLock<()> = OnceLock::new();

    let service_name = service_name.into();
    let initialized_global_subscriber = std::cell::Cell::new(false);
    TRACING_BOOTSTRAPPED.get_or_init(|| {
        initialized_global_subscriber.set(true);
        let subscriber = tracing_subscriber::fmt()
            .with_target(false)
            .without_time()
            .finish();
        let _ = ::tracing::subscriber::set_global_default(subscriber);
        ::tracing::info!(service_name = %service_name, "tracing bootstrapped");
    });

    TracingBootstrap {
        service_name,
        initialized_global_subscriber: initialized_global_subscriber.get(),
    }
}
