use chrono::{DateTime, Utc};

use crate::WsChannelKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WsSessionStatus {
    Connected,
    Reconnected,
    Disconnected,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WsSessionEvent {
    pub channel: WsChannelKind,
    pub connection_id: String,
    pub status: WsSessionStatus,
    pub reconnect_total: u64,
    pub observed_at: DateTime<Utc>,
    pub disconnect_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WsSessionState {
    pub channel: WsChannelKind,
    pub connection_id: Option<String>,
    pub connected: bool,
    pub reconnect_total: u64,
    pub last_connected_at: Option<DateTime<Utc>>,
    pub last_disconnected_at: Option<DateTime<Utc>>,
    pub last_disconnect_reason: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub struct WsSessionMonitor {
    channel: WsChannelKind,
}

impl WsSessionState {
    pub fn new(channel: WsChannelKind) -> Self {
        Self {
            channel,
            connection_id: None,
            connected: false,
            reconnect_total: 0,
            last_connected_at: None,
            last_disconnected_at: None,
            last_disconnect_reason: None,
        }
    }
}

impl WsSessionMonitor {
    pub fn new(channel: WsChannelKind) -> Self {
        Self { channel }
    }

    pub fn record_connected(
        &self,
        state: &mut WsSessionState,
        connection_id: &str,
        observed_at: DateTime<Utc>,
    ) -> WsSessionEvent {
        debug_assert_eq!(state.channel, self.channel);

        let status = if state.connected {
            WsSessionStatus::Connected
        } else if state.last_disconnected_at.is_some() {
            state.reconnect_total = state.reconnect_total.saturating_add(1);
            WsSessionStatus::Reconnected
        } else {
            WsSessionStatus::Connected
        };

        state.connection_id = Some(connection_id.to_owned());
        state.connected = true;
        state.last_connected_at = Some(observed_at);
        state.last_disconnected_at = None;
        state.last_disconnect_reason = None;

        WsSessionEvent {
            channel: self.channel,
            connection_id: connection_id.to_owned(),
            status,
            reconnect_total: state.reconnect_total,
            observed_at,
            disconnect_reason: None,
        }
    }

    pub fn record_disconnected(
        &self,
        state: &mut WsSessionState,
        reason: &str,
        observed_at: DateTime<Utc>,
    ) -> Option<WsSessionEvent> {
        debug_assert_eq!(state.channel, self.channel);

        if !state.connected {
            return None;
        }

        let connection_id = state.connection_id.clone().unwrap_or_default();
        state.connected = false;
        state.last_disconnected_at = Some(observed_at);
        state.last_disconnect_reason = Some(reason.to_owned());

        Some(WsSessionEvent {
            channel: self.channel,
            connection_id,
            status: WsSessionStatus::Disconnected,
            reconnect_total: state.reconnect_total,
            observed_at,
            disconnect_reason: Some(reason.to_owned()),
        })
    }
}
