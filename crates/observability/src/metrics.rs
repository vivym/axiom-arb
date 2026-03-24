#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
}

impl Default for RuntimeMetrics {
    fn default() -> Self {
        Self {
            heartbeat_freshness: GaugeHandle::new("axiom_heartbeat_freshness_seconds"),
            runtime_mode: ModeHandle::new("axiom_runtime_mode"),
            relayer_pending_age: GaugeHandle::new("axiom_relayer_pending_age_seconds"),
            divergence_count: CounterHandle::new("axiom_runtime_divergence_total"),
        }
    }
}
