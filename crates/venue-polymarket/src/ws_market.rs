use std::fmt;

use chrono::{DateTime, Utc};
use serde::Deserialize;

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
