use chrono::{DateTime, Utc};
use observability::{
    field_keys, metric_dimensions, span_names, MetricDimension, MetricDimensions,
    RuntimeMetricsRecorder,
};

use crate::{
    relayer::summarize_recent_transactions, HeartbeatReconcileReason, OrderHeartbeatState,
    RelayerTransaction, WsChannelKind, WsSessionEvent, WsSessionStatus,
};

#[derive(Debug, Clone, Default)]
pub struct VenueProducerInstrumentation {
    recorder: Option<RuntimeMetricsRecorder>,
}

impl VenueProducerInstrumentation {
    pub fn disabled() -> Self {
        Self { recorder: None }
    }

    pub fn enabled(recorder: RuntimeMetricsRecorder) -> Self {
        Self {
            recorder: Some(recorder),
        }
    }

    pub fn record_ws_session_event(&self, event: &WsSessionEvent) {
        let Some(recorder) = &self.recorder else {
            return;
        };

        let channel = match event.channel {
            WsChannelKind::Market => metric_dimensions::Channel::Market,
            WsChannelKind::User => metric_dimensions::Channel::User,
        };
        let session_status = match event.status {
            WsSessionStatus::Connected => "connected",
            WsSessionStatus::Reconnected => "reconnected",
            WsSessionStatus::Disconnected => "disconnected",
        };

        tracing::info_span!(
            span_names::VENUE_WS_SESSION,
            channel = channel.as_pair().1,
            connection_id = %event.connection_id,
            session_status = session_status,
            reconnect_total = event.reconnect_total,
            disconnect_reason = event.disconnect_reason.as_deref(),
            observed_at = %event.observed_at,
        )
        .in_scope(|| {
            if matches!(event.status, WsSessionStatus::Reconnected) {
                recorder.increment_websocket_reconnect_total(
                    1,
                    MetricDimensions::new([MetricDimension::Channel(channel)]),
                );
            }
        });
    }

    pub fn record_heartbeat_success(&self, state: &OrderHeartbeatState, freshness_seconds: f64) {
        let Some(recorder) = &self.recorder else {
            return;
        };

        recorder.record_heartbeat_freshness(freshness_seconds);

        let span = tracing::info_span!(
            span_names::VENUE_HEARTBEAT,
            heartbeat_id = tracing::field::Empty,
            heartbeat_status = "success",
        );
        if let Some(heartbeat_id) = state.heartbeat_id.as_deref() {
            span.record(field_keys::HEARTBEAT_ID, heartbeat_id);
        }

        span.in_scope(|| {});
    }

    pub fn record_heartbeat_attention(
        &self,
        state: &OrderHeartbeatState,
        reason: HeartbeatReconcileReason,
        at: DateTime<Utc>,
    ) {
        let Some(recorder) = &self.recorder else {
            return;
        };

        recorder.record_heartbeat_freshness(state.freshness_seconds(at));

        let span = tracing::warn_span!(
            span_names::VENUE_HEARTBEAT,
            heartbeat_id = tracing::field::Empty,
            heartbeat_status = reason.as_status(),
        );
        if let Some(heartbeat_id) = state.heartbeat_id.as_deref() {
            span.record(field_keys::HEARTBEAT_ID, heartbeat_id);
        }

        span.in_scope(|| {});
    }

    pub fn record_relayer_transactions(
        &self,
        transactions: &[RelayerTransaction],
        observed_at: DateTime<Utc>,
    ) {
        let Some(recorder) = &self.recorder else {
            return;
        };

        let (relayer_tx_count, pending_tx_count, pending_age_seconds) =
            summarize_recent_transactions(transactions, observed_at);

        recorder.record_relayer_pending_age(pending_age_seconds);

        tracing::info_span!(
            span_names::VENUE_RELAYER_POLL,
            relayer_tx_count = relayer_tx_count,
            pending_tx_count = pending_tx_count,
            pending_age_seconds = pending_age_seconds,
        )
        .in_scope(|| {});
    }
}
