use std::{collections::VecDeque, fmt};

use chrono::{DateTime, Duration, TimeZone, Utc};
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

    pub fn reset_reconcile_attention(
        &self,
        state: &mut WsChannelState,
        recovered_at: DateTime<Utc>,
    ) {
        debug_assert_eq!(state.channel, self.channel);

        state.last_message_at = recovered_at;
        state.stale_since = None;
        state.requires_reconcile_attention = false;
    }

    fn record_observation(&self, state: &mut WsChannelState, observed_at: DateTime<Utc>) {
        state.last_message_at = observed_at;
    }
}

#[derive(Debug)]
pub enum WsParseError {
    Json(serde_json::Error),
    MissingField(&'static str),
    UnknownEvent(String),
    InvalidField { field: &'static str, value: String },
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
    #[serde(default)]
    event: Option<String>,
    #[serde(default)]
    event_type: Option<String>,
    #[serde(default)]
    asset_id: Option<String>,
    #[serde(default)]
    market_id: Option<String>,
    #[serde(default, alias = "market")]
    market: Option<String>,
    #[serde(default)]
    bids: Option<Vec<BookLevel>>,
    #[serde(default)]
    asks: Option<Vec<BookLevel>>,
    #[serde(default)]
    price_changes: Option<Vec<PriceChangeEntry>>,
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
    #[serde(default, alias = "tick_size", alias = "new_tick_size")]
    tick_size: Option<Value>,
    #[serde(default, alias = "previous_tick_size", alias = "old_tick_size")]
    previous_tick_size: Option<Value>,
    #[serde(default)]
    status: Option<Value>,
    #[serde(default, alias = "timestamp")]
    ts: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct BookLevel {
    price: Value,
}

#[derive(Debug, Deserialize)]
struct PriceChangeEntry {
    asset_id: String,
    price: Value,
    #[serde(default)]
    side: Option<Value>,
}

pub fn parse_market_message(message: &str) -> Result<MarketWsEvent, WsParseError> {
    parse_market_messages(message)?
        .pop_front()
        .ok_or_else(|| WsParseError::UnknownEvent(String::new()))
}

pub fn parse_market_messages(message: &str) -> Result<VecDeque<MarketWsEvent>, WsParseError> {
    let trimmed = message.trim();
    if trimmed.eq_ignore_ascii_case("PING") {
        return Ok(VecDeque::from([MarketWsEvent::Ping]));
    }
    if trimmed.eq_ignore_ascii_case("PONG") {
        return Ok(VecDeque::from([MarketWsEvent::Pong]));
    }

    let envelope: MarketEnvelope = serde_json::from_str(message)?;
    let event_type = normalized_market_event(&envelope)?;
    let event_ts = parse_timestamp(envelope.ts)?;

    match event_type.as_str() {
        "PING" => Ok(VecDeque::from([MarketWsEvent::Ping])),
        "PONG" => Ok(VecDeque::from([MarketWsEvent::Pong])),
        "BOOK" | "BEST_BID_ASK" => Ok(VecDeque::from([MarketWsEvent::Book(MarketBookUpdate {
            asset_id: envelope
                .asset_id
                .ok_or(WsParseError::MissingField("asset_id"))?,
            best_bid: best_book_price(envelope.best_bid, envelope.bids, "bids")?,
            best_ask: best_book_price(envelope.best_ask, envelope.asks, "asks")?,
            event_ts,
        })])),
        "PRICE_CHANGE" => {
            if let Some(price_changes) = envelope.price_changes {
                let mut events = VecDeque::with_capacity(price_changes.len());
                for change in price_changes {
                    events.push_back(MarketWsEvent::PriceChange(MarketPriceChangeUpdate {
                        asset_id: change.asset_id,
                        price: required_value(Some(change.price), "price")?,
                        side: optional_string(change.side, "side")?,
                        event_ts,
                    }));
                }
                Ok(events)
            } else {
                Ok(VecDeque::from([MarketWsEvent::PriceChange(
                    MarketPriceChangeUpdate {
                        asset_id: envelope
                            .asset_id
                            .ok_or(WsParseError::MissingField("asset_id"))?,
                        price: required_value(envelope.price, "price")?,
                        side: optional_string(envelope.side, "side")?,
                        event_ts,
                    },
                )]))
            }
        }
        "LAST_TRADE_PRICE" => Ok(VecDeque::from([MarketWsEvent::LastTradePrice(
            MarketTradePriceUpdate {
                asset_id: envelope
                    .asset_id
                    .ok_or(WsParseError::MissingField("asset_id"))?,
                price: required_value(
                    envelope.last_trade_price.or(envelope.price),
                    "last_trade_price",
                )?,
                size: optional_string(envelope.size, "size")?,
                event_ts,
            },
        )])),
        "TICK_SIZE_CHANGE" => Ok(VecDeque::from([MarketWsEvent::TickSizeChange(
            MarketTickSizeChangeUpdate {
                asset_id: envelope
                    .asset_id
                    .ok_or(WsParseError::MissingField("asset_id"))?,
                previous_tick_size: optional_string(
                    envelope.previous_tick_size,
                    "previous_tick_size",
                )?,
                tick_size: required_value(envelope.tick_size, "tick_size")?,
                event_ts,
            },
        )])),
        "LIFECYCLE" | "STATUS" | "MARKET_STATUS" => Ok(VecDeque::from([MarketWsEvent::Lifecycle(
            MarketLifecycleUpdate {
                market_id: envelope.market_id,
                asset_id: envelope.asset_id,
                status: required_value(envelope.status, "status")?,
                event_ts,
            },
        )])),
        "NEW_MARKET" | "MARKET_RESOLVED" => Ok(VecDeque::from([MarketWsEvent::Lifecycle(
            MarketLifecycleUpdate {
                market_id: envelope.market_id.or(envelope.market),
                asset_id: envelope.asset_id,
                status: event_type,
                event_ts,
            },
        )])),
        other => Err(WsParseError::UnknownEvent(other.to_owned())),
    }
}

fn normalized_market_event(envelope: &MarketEnvelope) -> Result<String, WsParseError> {
    envelope
        .event
        .as_deref()
        .or(envelope.event_type.as_deref())
        .map(normalize_event)
        .ok_or(WsParseError::MissingField("event_type"))
}

fn normalize_event(value: &str) -> String {
    value.trim().to_ascii_uppercase()
}

fn parse_timestamp(value: Option<Value>) -> Result<Option<DateTime<Utc>>, WsParseError> {
    let Some(value) = value else {
        return Ok(None);
    };

    let raw = match value {
        Value::String(value) => value,
        Value::Number(value) => value.to_string(),
        other => {
            return Err(WsParseError::InvalidField {
                field: "timestamp",
                value: other.to_string(),
            });
        }
    };

    if let Ok(parsed) = DateTime::parse_from_rfc3339(&raw) {
        return Ok(Some(parsed.with_timezone(&Utc)));
    }

    let numeric = raw
        .parse::<i64>()
        .map_err(|_| WsParseError::InvalidTimestamp(raw.clone()))?;
    let parsed = if raw.len() >= 13 {
        Utc.timestamp_millis_opt(numeric).single()
    } else {
        Utc.timestamp_opt(numeric, 0).single()
    }
    .ok_or_else(|| WsParseError::InvalidTimestamp(raw.clone()))?;

    Ok(Some(parsed))
}

fn best_book_price(
    direct: Option<Value>,
    levels: Option<Vec<BookLevel>>,
    field: &'static str,
) -> Result<Option<String>, WsParseError> {
    if direct.is_some() {
        return optional_string(direct, field);
    }

    let Some(levels) = levels else {
        return Ok(None);
    };

    let Some(first) = levels.into_iter().next() else {
        return Ok(None);
    };

    optional_string(Some(first.price), field)
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
