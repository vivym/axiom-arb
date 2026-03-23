use std::fmt;

use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;

use crate::UserWsEvent;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MarketWsEvent {
    Book(MarketBookUpdate),
    Ping,
    Pong,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarketBookUpdate {
    pub asset_id: String,
    pub best_bid: Option<String>,
    pub best_ask: Option<String>,
    pub event_ts: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WsChannelKind {
    Market,
    User,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WsChannelState {
    pub channel: WsChannelKind,
    pub last_message_at: DateTime<Utc>,
    pub last_ping_at: Option<DateTime<Utc>>,
    pub last_pong_at: Option<DateTime<Utc>>,
    pub stale_since: Option<DateTime<Utc>>,
    pub requires_reconcile_attention: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WsChannelReconcileReason {
    StaleChannel { channel: WsChannelKind },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WsChannelLivenessMonitor {
    channel: WsChannelKind,
    max_gap: Duration,
}

impl WsChannelState {
    pub fn new(channel: WsChannelKind, observed_at: DateTime<Utc>) -> Self {
        Self {
            channel,
            last_message_at: observed_at,
            last_ping_at: None,
            last_pong_at: None,
            stale_since: None,
            requires_reconcile_attention: false,
        }
    }
}

impl WsChannelLivenessMonitor {
    pub fn new(channel: WsChannelKind, max_gap: Duration) -> Self {
        Self { channel, max_gap }
    }

    pub fn record_market_event(
        &self,
        state: &mut WsChannelState,
        event: &MarketWsEvent,
        observed_at: DateTime<Utc>,
    ) {
        debug_assert_eq!(state.channel, WsChannelKind::Market);
        debug_assert_eq!(self.channel, WsChannelKind::Market);

        self.record_observation(state, observed_at);
        match event {
            MarketWsEvent::Ping => state.last_ping_at = Some(observed_at),
            MarketWsEvent::Pong => state.last_pong_at = Some(observed_at),
            MarketWsEvent::Book(_) => {}
        }
    }

    pub fn record_user_event(
        &self,
        state: &mut WsChannelState,
        event: &UserWsEvent,
        observed_at: DateTime<Utc>,
    ) {
        debug_assert_eq!(state.channel, WsChannelKind::User);
        debug_assert_eq!(self.channel, WsChannelKind::User);

        self.record_observation(state, observed_at);
        match event {
            UserWsEvent::Ping => state.last_ping_at = Some(observed_at),
            UserWsEvent::Pong => state.last_pong_at = Some(observed_at),
            UserWsEvent::Order(_) | UserWsEvent::Trade(_) => {}
        }
    }

    pub fn reconcile_trigger(
        &self,
        state: &mut WsChannelState,
        now: DateTime<Utc>,
    ) -> Option<WsChannelReconcileReason> {
        debug_assert_eq!(state.channel, self.channel);

        if now.signed_duration_since(state.last_message_at) <= self.max_gap {
            return None;
        }

        if state.requires_reconcile_attention {
            return None;
        }

        state.requires_reconcile_attention = true;
        state.stale_since = Some(now);
        Some(WsChannelReconcileReason::StaleChannel {
            channel: self.channel,
        })
    }

    fn record_observation(&self, state: &mut WsChannelState, observed_at: DateTime<Utc>) {
        state.last_message_at = observed_at;
        state.stale_since = None;
        state.requires_reconcile_attention = false;
    }
}

#[derive(Debug)]
pub enum WsParseError {
    Json(serde_json::Error),
    MissingField(&'static str),
    UnknownEvent(String),
    InvalidTimestamp(String),
}

impl fmt::Display for WsParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(err) => write!(f, "websocket json parse error: {err}"),
            Self::MissingField(field) => write!(f, "websocket payload missing field: {field}"),
            Self::UnknownEvent(event) => write!(f, "unsupported websocket event: {event}"),
            Self::InvalidTimestamp(value) => write!(f, "invalid websocket timestamp: {value}"),
        }
    }
}

impl std::error::Error for WsParseError {}

impl From<serde_json::Error> for WsParseError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

#[derive(Debug, Deserialize)]
struct MarketEnvelope {
    event: String,
    #[serde(default)]
    asset_id: Option<String>,
    #[serde(default)]
    best_bid: Option<String>,
    #[serde(default)]
    best_ask: Option<String>,
    #[serde(default, alias = "timestamp")]
    ts: Option<String>,
}

pub fn parse_market_message(message: &str) -> Result<MarketWsEvent, WsParseError> {
    let envelope: MarketEnvelope = serde_json::from_str(message)?;
    match normalize_event(&envelope.event).as_str() {
        "PING" => Ok(MarketWsEvent::Ping),
        "PONG" => Ok(MarketWsEvent::Pong),
        "BOOK" => Ok(MarketWsEvent::Book(MarketBookUpdate {
            asset_id: envelope
                .asset_id
                .ok_or(WsParseError::MissingField("asset_id"))?,
            best_bid: envelope.best_bid,
            best_ask: envelope.best_ask,
            event_ts: parse_timestamp(envelope.ts)?,
        })),
        other => Err(WsParseError::UnknownEvent(other.to_owned())),
    }
}

fn normalize_event(value: &str) -> String {
    value.trim().to_ascii_uppercase()
}

fn parse_timestamp(value: Option<String>) -> Result<Option<DateTime<Utc>>, WsParseError> {
    let Some(value) = value else {
        return Ok(None);
    };

    let parsed = DateTime::parse_from_rfc3339(&value)
        .map_err(|_| WsParseError::InvalidTimestamp(value.clone()))?;
    Ok(Some(parsed.with_timezone(&Utc)))
}
