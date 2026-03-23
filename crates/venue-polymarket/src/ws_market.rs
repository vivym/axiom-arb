use std::fmt;

use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
use serde_json::Value;

use crate::UserWsEvent;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MarketWsEvent {
    Book(MarketBookUpdate),
    PriceChange(MarketPriceChangeUpdate),
    LastTradePrice(MarketTradePriceUpdate),
    TickSizeChange(MarketTickSizeChangeUpdate),
    Lifecycle(MarketLifecycleUpdate),
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarketPriceChangeUpdate {
    pub asset_id: String,
    pub price: String,
    pub side: Option<String>,
    pub event_ts: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarketTradePriceUpdate {
    pub asset_id: String,
    pub price: String,
    pub size: Option<String>,
    pub event_ts: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarketTickSizeChangeUpdate {
    pub asset_id: String,
    pub previous_tick_size: Option<String>,
    pub tick_size: String,
    pub event_ts: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarketLifecycleUpdate {
    pub market_id: Option<String>,
    pub asset_id: Option<String>,
    pub status: String,
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
            MarketWsEvent::Book(_)
            | MarketWsEvent::PriceChange(_)
            | MarketWsEvent::LastTradePrice(_)
            | MarketWsEvent::TickSizeChange(_)
            | MarketWsEvent::Lifecycle(_) => {}
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
        state.stale_since = Some(state.last_message_at + self.max_gap);
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
    InvalidField {
        field: &'static str,
        value: String,
    },
    InvalidTimestamp(String),
}

impl fmt::Display for WsParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(err) => write!(f, "websocket json parse error: {err}"),
            Self::MissingField(field) => write!(f, "websocket payload missing field: {field}"),
            Self::UnknownEvent(event) => write!(f, "unsupported websocket event: {event}"),
            Self::InvalidField { field, value } => {
                write!(f, "invalid websocket field {field}: {value}")
            }
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
    market_id: Option<String>,
    #[serde(default)]
    best_bid: Option<Value>,
    #[serde(default)]
    best_ask: Option<Value>,
    #[serde(default)]
    price: Option<Value>,
    #[serde(default)]
    side: Option<Value>,
    #[serde(default, alias = "last_trade_price")]
    last_trade_price: Option<Value>,
    #[serde(default)]
    size: Option<Value>,
    #[serde(default, alias = "tick_size")]
    tick_size: Option<Value>,
    #[serde(default, alias = "previous_tick_size")]
    previous_tick_size: Option<Value>,
    #[serde(default)]
    status: Option<Value>,
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
            best_bid: optional_string(envelope.best_bid, "best_bid")?,
            best_ask: optional_string(envelope.best_ask, "best_ask")?,
            event_ts: parse_timestamp(envelope.ts)?,
        })),
        "PRICE_CHANGE" => Ok(MarketWsEvent::PriceChange(MarketPriceChangeUpdate {
            asset_id: envelope
                .asset_id
                .ok_or(WsParseError::MissingField("asset_id"))?,
            price: required_value(envelope.price, "price")?,
            side: optional_string(envelope.side, "side")?,
            event_ts: parse_timestamp(envelope.ts)?,
        })),
        "LAST_TRADE_PRICE" => Ok(MarketWsEvent::LastTradePrice(MarketTradePriceUpdate {
            asset_id: envelope
                .asset_id
                .ok_or(WsParseError::MissingField("asset_id"))?,
            price: required_value(
                envelope.last_trade_price.or(envelope.price),
                "last_trade_price",
            )?,
            size: optional_string(envelope.size, "size")?,
            event_ts: parse_timestamp(envelope.ts)?,
        })),
        "TICK_SIZE_CHANGE" => Ok(MarketWsEvent::TickSizeChange(
            MarketTickSizeChangeUpdate {
                asset_id: envelope
                    .asset_id
                    .ok_or(WsParseError::MissingField("asset_id"))?,
                previous_tick_size: optional_string(
                    envelope.previous_tick_size,
                    "previous_tick_size",
                )?,
                tick_size: required_value(envelope.tick_size, "tick_size")?,
                event_ts: parse_timestamp(envelope.ts)?,
            },
        )),
        "LIFECYCLE" | "STATUS" | "MARKET_STATUS" => {
            Ok(MarketWsEvent::Lifecycle(MarketLifecycleUpdate {
                market_id: envelope.market_id,
                asset_id: envelope.asset_id,
                status: required_value(envelope.status, "status")?,
                event_ts: parse_timestamp(envelope.ts)?,
            }))
        }
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

fn required_value(value: Option<Value>, field: &'static str) -> Result<String, WsParseError> {
    optional_string(value, field)?.ok_or(WsParseError::MissingField(field))
}

fn optional_string(
    value: Option<Value>,
    field: &'static str,
) -> Result<Option<String>, WsParseError> {
    match value {
        None => Ok(None),
        Some(Value::String(value)) => Ok(Some(value)),
        Some(Value::Number(value)) => Ok(Some(value.to_string())),
        Some(Value::Bool(value)) => Ok(Some(value.to_string())),
        Some(other) => Err(WsParseError::InvalidField {
            field,
            value: other.to_string(),
        }),
    }
}
