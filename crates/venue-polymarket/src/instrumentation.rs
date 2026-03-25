use observability::{
    metric_dimensions, span_names, MetricDimension, MetricDimensions, RuntimeMetricsRecorder,
};

use crate::{WsChannelKind, WsSessionEvent, WsSessionStatus};

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
                if let Some(recorder) = &self.recorder {
                    recorder.increment_websocket_reconnect_total(
                        1,
                        MetricDimensions::new([MetricDimension::Channel(channel)]),
                    );
                }
            }
        });
    }
}
