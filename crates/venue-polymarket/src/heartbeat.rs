use chrono::{DateTime, Duration, Utc};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderHeartbeatState {
    pub heartbeat_id: Option<String>,
    pub last_success_at: DateTime<Utc>,
    pub reconcile_attention_since: Option<DateTime<Utc>>,
    pub reconcile_reason: Option<HeartbeatReconcileReason>,
    pub requires_reconcile_attention: bool,
}

impl OrderHeartbeatState {
    pub fn freshness_seconds(&self, at: DateTime<Utc>) -> f64 {
        (at - self.last_success_at).num_seconds() as f64
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeartbeatReconcileReason {
    MissedHeartbeat,
    InvalidHeartbeat,
}

impl HeartbeatReconcileReason {
    pub const fn as_status(self) -> &'static str {
        match self {
            Self::MissedHeartbeat => "missed",
            Self::InvalidHeartbeat => "invalid",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderHeartbeatMonitor {
    max_gap: Duration,
}

impl OrderHeartbeatMonitor {
    pub fn new(max_gap: Duration) -> Self {
        Self { max_gap }
    }

    pub fn record_success(
        &self,
        state: &mut OrderHeartbeatState,
        heartbeat_id: impl Into<String>,
        at: DateTime<Utc>,
    ) {
        state.heartbeat_id = Some(heartbeat_id.into());
        state.last_success_at = at;
        state.reconcile_attention_since = None;
        state.reconcile_reason = None;
        state.requires_reconcile_attention = false;
    }

    pub fn record_invalid(
        &self,
        state: &mut OrderHeartbeatState,
        at: DateTime<Utc>,
    ) -> Option<HeartbeatReconcileReason> {
        state.heartbeat_id = None;
        self.raise_attention(state, at, HeartbeatReconcileReason::InvalidHeartbeat)
    }

    pub fn reconcile_trigger(
        &self,
        state: &mut OrderHeartbeatState,
        now: DateTime<Utc>,
    ) -> Option<HeartbeatReconcileReason> {
        let stale_onset = state.last_success_at + self.max_gap;
        if now > stale_onset {
            return self.raise_attention(
                state,
                stale_onset,
                HeartbeatReconcileReason::MissedHeartbeat,
            );
        }

        None
    }

    fn raise_attention(
        &self,
        state: &mut OrderHeartbeatState,
        at: DateTime<Utc>,
        reason: HeartbeatReconcileReason,
    ) -> Option<HeartbeatReconcileReason> {
        if state.requires_reconcile_attention {
            return None;
        }

        state.reconcile_attention_since = Some(at);
        state.reconcile_reason = Some(reason);
        state.requires_reconcile_attention = true;
        Some(reason)
    }
}
