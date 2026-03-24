use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MetricKey(&'static str);

impl MetricKey {
    pub const fn new(key: &'static str) -> Self {
        Self(key)
    }

    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GaugeHandle {
    key: MetricKey,
}

impl GaugeHandle {
    pub const fn new(key: &'static str) -> Self {
        Self {
            key: MetricKey::new(key),
        }
    }

    pub const fn key(self) -> MetricKey {
        self.key
    }

    pub fn sample(self, value: f64) -> GaugeSample {
        GaugeSample {
            key: self.key,
            value,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GaugeSample {
    key: MetricKey,
    value: f64,
}

impl GaugeSample {
    pub const fn key(self) -> MetricKey {
        self.key
    }

    pub const fn value(self) -> f64 {
        self.value
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CounterHandle {
    key: MetricKey,
}

impl CounterHandle {
    pub const fn new(key: &'static str) -> Self {
        Self {
            key: MetricKey::new(key),
        }
    }

    pub const fn key(self) -> MetricKey {
        self.key
    }

    pub const fn increment(self, amount: u64) -> CounterSample {
        CounterSample {
            key: self.key,
            amount,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CounterSample {
    key: MetricKey,
    amount: u64,
}

impl CounterSample {
    pub const fn key(self) -> MetricKey {
        self.key
    }

    pub const fn amount(self) -> u64 {
        self.amount
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModeHandle {
    key: MetricKey,
}

impl ModeHandle {
    pub const fn new(key: &'static str) -> Self {
        Self {
            key: MetricKey::new(key),
        }
    }

    pub const fn key(self) -> MetricKey {
        self.key
    }

    pub fn sample(self, mode: impl Into<String>) -> ModeSample {
        ModeSample {
            key: self.key,
            mode: mode.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModeSample {
    key: MetricKey,
    mode: String,
}

impl ModeSample {
    pub const fn key(&self) -> MetricKey {
        self.key
    }

    pub fn mode(&self) -> &str {
        &self.mode
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeMetrics {
    pub heartbeat_freshness: GaugeHandle,
    pub runtime_mode: ModeHandle,
    pub relayer_pending_age: GaugeHandle,
    pub divergence_count: CounterHandle,
    pub dispatcher_backlog_count: GaugeHandle,
    pub projection_publish_lag_count: GaugeHandle,
    pub recovery_backlog_count: GaugeHandle,
    pub shadow_attempt_count: CounterHandle,
    pub neg_risk_family_discovered_count: GaugeHandle,
    pub neg_risk_family_included_count: GaugeHandle,
    pub neg_risk_family_excluded_count: GaugeHandle,
    pub neg_risk_family_halt_count: GaugeHandle,
    pub neg_risk_metadata_refresh_count: CounterHandle,
}

impl Default for RuntimeMetrics {
    fn default() -> Self {
        Self {
            heartbeat_freshness: GaugeHandle::new("axiom_heartbeat_freshness_seconds"),
            runtime_mode: ModeHandle::new("axiom_runtime_mode"),
            relayer_pending_age: GaugeHandle::new("axiom_relayer_pending_age_seconds"),
            divergence_count: CounterHandle::new("axiom_runtime_divergence_total"),
            dispatcher_backlog_count: GaugeHandle::new("axiom_dispatcher_backlog_count"),
            projection_publish_lag_count: GaugeHandle::new("axiom_projection_publish_lag_count"),
            recovery_backlog_count: GaugeHandle::new("axiom_recovery_backlog_count"),
            shadow_attempt_count: CounterHandle::new("axiom_shadow_attempt_total"),
            neg_risk_family_discovered_count: GaugeHandle::new(
                "axiom_neg_risk_family_discovered_count",
            ),
            neg_risk_family_included_count: GaugeHandle::new(
                "axiom_neg_risk_family_included_count",
            ),
            neg_risk_family_excluded_count: GaugeHandle::new(
                "axiom_neg_risk_family_excluded_count",
            ),
            neg_risk_family_halt_count: GaugeHandle::new("axiom_neg_risk_family_halt_count"),
            neg_risk_metadata_refresh_count: CounterHandle::new(
                "axiom_neg_risk_metadata_refresh_total",
            ),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct MetricRegistry {
    inner: Arc<Mutex<MetricRegistryState>>,
}

#[derive(Debug, Default)]
struct MetricRegistryState {
    gauges: HashMap<MetricKey, f64>,
    counters: HashMap<MetricKey, u64>,
    modes: HashMap<MetricKey, String>,
}

impl MetricRegistry {
    pub fn record_gauge(&self, sample: GaugeSample) {
        self.inner
            .lock()
            .expect("metric registry lock")
            .gauges
            .insert(sample.key(), sample.value());
    }

    pub fn record_counter(&self, sample: CounterSample) {
        let mut state = self.inner.lock().expect("metric registry lock");
        let entry = state.counters.entry(sample.key()).or_default();
        *entry += sample.amount();
    }

    pub fn record_mode(&self, sample: ModeSample) {
        self.inner
            .lock()
            .expect("metric registry lock")
            .modes
            .insert(sample.key(), sample.mode().to_owned());
    }

    pub fn snapshot(&self) -> MetricRegistrySnapshot {
        let state = self.inner.lock().expect("metric registry lock");
        MetricRegistrySnapshot {
            gauges: state.gauges.clone(),
            counters: state.counters.clone(),
            modes: state.modes.clone(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct MetricRegistrySnapshot {
    gauges: HashMap<MetricKey, f64>,
    counters: HashMap<MetricKey, u64>,
    modes: HashMap<MetricKey, String>,
}

impl MetricRegistrySnapshot {
    pub fn gauge(&self, key: MetricKey) -> Option<f64> {
        self.gauges.get(&key).copied()
    }

    pub fn counter(&self, key: MetricKey) -> Option<u64> {
        self.counters.get(&key).copied()
    }

    pub fn mode(&self, key: MetricKey) -> Option<&str> {
        self.modes.get(&key).map(String::as_str)
    }
}

#[derive(Debug, Clone)]
pub struct RuntimeMetricsRecorder {
    metrics: RuntimeMetrics,
    registry: MetricRegistry,
}

impl RuntimeMetricsRecorder {
    pub fn new(metrics: RuntimeMetrics, registry: MetricRegistry) -> Self {
        Self { metrics, registry }
    }

    pub fn record_heartbeat_freshness(&self, seconds: f64) {
        self.registry
            .record_gauge(self.metrics.heartbeat_freshness.sample(seconds));
    }

    pub fn record_runtime_mode(&self, mode: impl Into<String>) {
        self.registry
            .record_mode(self.metrics.runtime_mode.sample(mode));
    }

    pub fn record_relayer_pending_age(&self, seconds: f64) {
        self.registry
            .record_gauge(self.metrics.relayer_pending_age.sample(seconds));
    }

    pub fn increment_divergence_count(&self, amount: u64) {
        self.registry
            .record_counter(self.metrics.divergence_count.increment(amount));
    }

    pub fn record_dispatcher_backlog_count(&self, count: f64) {
        self.registry
            .record_gauge(self.metrics.dispatcher_backlog_count.sample(count));
    }

    pub fn record_projection_publish_lag_count(&self, count: f64) {
        self.registry
            .record_gauge(self.metrics.projection_publish_lag_count.sample(count));
    }

    pub fn record_recovery_backlog_count(&self, count: f64) {
        self.registry
            .record_gauge(self.metrics.recovery_backlog_count.sample(count));
    }

    pub fn increment_shadow_attempt_count(&self, amount: u64) {
        self.registry
            .record_counter(self.metrics.shadow_attempt_count.increment(amount));
    }

    pub fn record_neg_risk_family_discovered_count(&self, count: f64) {
        self.registry
            .record_gauge(self.metrics.neg_risk_family_discovered_count.sample(count));
    }

    pub fn record_neg_risk_family_included_count(&self, count: f64) {
        self.registry
            .record_gauge(self.metrics.neg_risk_family_included_count.sample(count));
    }

    pub fn record_neg_risk_family_excluded_count(&self, count: f64) {
        self.registry
            .record_gauge(self.metrics.neg_risk_family_excluded_count.sample(count));
    }

    pub fn record_neg_risk_family_halt_count(&self, count: f64) {
        self.registry
            .record_gauge(self.metrics.neg_risk_family_halt_count.sample(count));
    }

    pub fn increment_neg_risk_metadata_refresh_count(&self, amount: u64) {
        self.registry.record_counter(
            self.metrics
                .neg_risk_metadata_refresh_count
                .increment(amount),
        );
    }
}
