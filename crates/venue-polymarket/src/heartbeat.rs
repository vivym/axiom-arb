use chrono::{DateTime, Duration, Utc};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderHeartbeatState {
    pub heartbeat_id: Option<String>,
    pub last_success_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeartbeatReconcileReason {
    MissedHeartbeat,
    InvalidHeartbeat,
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
    }

    pub fn record_invalid(
        &self,
        state: &mut OrderHeartbeatState,
        _at: DateTime<Utc>,
    ) -> Option<HeartbeatReconcileReason> {
        state.heartbeat_id = None;
        Some(HeartbeatReconcileReason::InvalidHeartbeat)
    }

    pub fn reconcile_trigger(
        &self,
        state: &OrderHeartbeatState,
        now: DateTime<Utc>,
    ) -> Option<HeartbeatReconcileReason> {
        if now.signed_duration_since(state.last_success_at) > self.max_gap {
            return Some(HeartbeatReconcileReason::MissedHeartbeat);
        }

        None
    }
}
