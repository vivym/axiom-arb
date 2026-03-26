use std::{
    collections::HashMap,
    hash::{Hash, Hasher},
    sync::{Arc, Mutex},
};

use crate::metric_dimensions;

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

fn is_dimensioned_counter_key(key: MetricKey) -> bool {
    matches!(
        key.as_str(),
        "axiom_websocket_reconnect_total"
            | "axiom_halt_activation_total"
            | "axiom_reconcile_attention_total"
    )
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
pub struct DimensionedCounterHandle {
    key: MetricKey,
}

impl DimensionedCounterHandle {
    pub(crate) const fn new(key: &'static str) -> Self {
        Self {
            key: MetricKey::new(key),
        }
    }

    pub const fn key(self) -> MetricKey {
        self.key
    }

    pub fn increment_with_dimensions(
        self,
        amount: u64,
        dimensions: MetricDimensions,
    ) -> CounterSampleWithDimensions {
        CounterSampleWithDimensions {
            key: self.key,
            amount,
            dimensions: dimensions.canonicalized(),
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MetricDimension {
    Channel(metric_dimensions::Channel),
    HaltScope(metric_dimensions::HaltScope),
    ReconcileReason(metric_dimensions::ReconcileReason),
}

impl MetricDimension {
    fn as_pair(&self) -> (&'static str, &'static str) {
        match self {
            Self::Channel(channel) => channel.as_pair(),
            Self::HaltScope(scope) => scope.as_pair(),
            Self::ReconcileReason(reason) => reason.as_pair(),
        }
    }

    fn key_name(&self) -> &'static str {
        self.as_pair().0
    }

    fn canonical_key(&self) -> (&'static str, &'static str) {
        self.as_pair()
    }
}

impl Hash for MetricDimension {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_pair().hash(state);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct MetricDimensions(Vec<MetricDimension>);

impl MetricDimensions {
    pub fn new<I>(dimensions: I) -> Self
    where
        I: IntoIterator<Item = MetricDimension>,
    {
        Self(dimensions.into_iter().collect()).canonicalized()
    }

    fn canonicalized(&self) -> Self {
        let mut dimensions = self.0.clone();
        dimensions.sort_by_key(MetricDimension::canonical_key);
        let mut canonical: Vec<MetricDimension> = Vec::with_capacity(dimensions.len());

        for dimension in dimensions {
            if let Some(previous) = canonical.last() {
                if previous.key_name() == dimension.key_name() {
                    if previous == &dimension {
                        continue;
                    }

                    panic!(
                        "conflicting metric dimension values for key {}",
                        dimension.key_name()
                    );
                }
            }

            canonical.push(dimension);
        }

        Self(canonical)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CounterSampleWithDimensions {
    key: MetricKey,
    amount: u64,
    dimensions: MetricDimensions,
}

impl CounterSampleWithDimensions {
    pub const fn key(&self) -> MetricKey {
        self.key
    }

    pub const fn amount(&self) -> u64 {
        self.amount
    }

    pub fn dimensions(&self) -> &MetricDimensions {
        &self.dimensions
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
    pub websocket_reconnect_total: DimensionedCounterHandle,
    pub halt_activation_total: DimensionedCounterHandle,
    pub reconcile_attention_total: DimensionedCounterHandle,
    pub dispatcher_backlog_count: GaugeHandle,
    pub projection_publish_lag_count: GaugeHandle,
    pub recovery_backlog_count: GaugeHandle,
    pub shadow_attempt_count: CounterHandle,
    pub neg_risk_family_discovered_count: GaugeHandle,
    pub neg_risk_family_included_count: GaugeHandle,
    pub neg_risk_family_excluded_count: GaugeHandle,
    pub neg_risk_family_halt_count: GaugeHandle,
    pub neg_risk_metadata_refresh_count: CounterHandle,
    pub neg_risk_live_ready_family_count: GaugeHandle,
    pub neg_risk_live_attempt_count: GaugeHandle,
    pub neg_risk_live_gate_block_count: GaugeHandle,
    pub neg_risk_live_submit_accepted_total: CounterHandle,
    pub neg_risk_live_submit_ambiguous_total: CounterHandle,
    pub neg_risk_rollout_parity_mismatch_count: CounterHandle,
}

impl Default for RuntimeMetrics {
    fn default() -> Self {
        Self {
            heartbeat_freshness: GaugeHandle::new("axiom_heartbeat_freshness_seconds"),
            runtime_mode: ModeHandle::new("axiom_runtime_mode"),
            relayer_pending_age: GaugeHandle::new("axiom_relayer_pending_age_seconds"),
            divergence_count: CounterHandle::new("axiom_runtime_divergence_total"),
            websocket_reconnect_total: DimensionedCounterHandle::new(
                "axiom_websocket_reconnect_total",
            ),
            halt_activation_total: DimensionedCounterHandle::new("axiom_halt_activation_total"),
            reconcile_attention_total: DimensionedCounterHandle::new(
                "axiom_reconcile_attention_total",
            ),
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
            neg_risk_live_ready_family_count: GaugeHandle::new(
                "axiom_neg_risk_live_ready_family_count",
            ),
            neg_risk_live_attempt_count: GaugeHandle::new("axiom_neg_risk_live_attempt_count"),
            neg_risk_live_gate_block_count: GaugeHandle::new(
                "axiom_neg_risk_live_gate_block_count",
            ),
            neg_risk_live_submit_accepted_total: CounterHandle::new(
                "axiom_neg_risk_live_submit_accepted_total",
            ),
            neg_risk_live_submit_ambiguous_total: CounterHandle::new(
                "axiom_neg_risk_live_submit_ambiguous_total",
            ),
            neg_risk_rollout_parity_mismatch_count: CounterHandle::new(
                "axiom_neg_risk_rollout_parity_mismatch_total",
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
    counters_with_dimensions: HashMap<(MetricKey, MetricDimensions), u64>,
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
        assert!(
            !is_dimensioned_counter_key(sample.key()),
            "dimensioned counters must not be recorded through the scalar path"
        );
        let mut state = self.inner.lock().expect("metric registry lock");
        let entry = state.counters.entry(sample.key()).or_default();
        *entry += sample.amount();
    }

    pub fn record_counter_with_dimensions(&self, sample: CounterSampleWithDimensions) {
        let mut state = self.inner.lock().expect("metric registry lock");
        let entry = state
            .counters_with_dimensions
            .entry((sample.key(), sample.dimensions().canonicalized()))
            .or_default();
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
            counters_with_dimensions: state.counters_with_dimensions.clone(),
            modes: state.modes.clone(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct MetricRegistrySnapshot {
    gauges: HashMap<MetricKey, f64>,
    counters: HashMap<MetricKey, u64>,
    counters_with_dimensions: HashMap<(MetricKey, MetricDimensions), u64>,
    modes: HashMap<MetricKey, String>,
}

impl MetricRegistrySnapshot {
    pub fn gauge(&self, key: MetricKey) -> Option<f64> {
        self.gauges.get(&key).copied()
    }

    pub fn counter(&self, key: MetricKey) -> Option<u64> {
        self.counters.get(&key).copied()
    }

    pub fn counter_with_dimensions(
        &self,
        key: MetricKey,
        dimensions: &MetricDimensions,
    ) -> Option<u64> {
        self.counters_with_dimensions
            .get(&(key, dimensions.canonicalized()))
            .copied()
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

    pub fn increment_websocket_reconnect_total(&self, amount: u64, dimensions: MetricDimensions) {
        self.registry.record_counter_with_dimensions(
            self.metrics
                .websocket_reconnect_total
                .increment_with_dimensions(amount, dimensions),
        );
    }

    pub fn increment_halt_activation_total(&self, amount: u64, dimensions: MetricDimensions) {
        self.registry.record_counter_with_dimensions(
            self.metrics
                .halt_activation_total
                .increment_with_dimensions(amount, dimensions),
        );
    }

    pub fn increment_reconcile_attention_total(&self, amount: u64, dimensions: MetricDimensions) {
        self.registry.record_counter_with_dimensions(
            self.metrics
                .reconcile_attention_total
                .increment_with_dimensions(amount, dimensions),
        );
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

    pub fn record_neg_risk_live_ready_family_count(&self, count: f64) {
        self.registry
            .record_gauge(self.metrics.neg_risk_live_ready_family_count.sample(count));
    }

    pub fn record_neg_risk_live_attempt_count(&self, count: f64) {
        self.registry
            .record_gauge(self.metrics.neg_risk_live_attempt_count.sample(count));
    }

    pub fn record_neg_risk_live_gate_block_count(&self, count: f64) {
        self.registry
            .record_gauge(self.metrics.neg_risk_live_gate_block_count.sample(count));
    }

    pub fn increment_neg_risk_live_submit_accepted_total(&self, amount: u64) {
        self.registry.record_counter(
            self.metrics
                .neg_risk_live_submit_accepted_total
                .increment(amount),
        );
    }

    pub fn increment_neg_risk_live_submit_ambiguous_total(&self, amount: u64) {
        self.registry.record_counter(
            self.metrics
                .neg_risk_live_submit_ambiguous_total
                .increment(amount),
        );
    }

    pub fn increment_neg_risk_rollout_parity_mismatch_count(&self, amount: u64) {
        self.registry.record_counter(
            self.metrics
                .neg_risk_rollout_parity_mismatch_count
                .increment(amount),
        );
    }
}
