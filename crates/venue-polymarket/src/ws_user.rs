use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::Value;

use crate::WsParseError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UserWsEvent {
    Order(UserOrderUpdate),
    Trade(UserTradeUpdate),
    Ping,
    Pong,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserOrderUpdate {
    pub order_id: String,
    pub status: String,
    pub condition_id: String,
    pub price: Option<String>,
    pub size: Option<String>,
    pub fee_rate_bps: Option<String>,
    pub transaction_hash: Option<String>,
    pub event_ts: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserTradeUpdate {
    pub trade_id: String,
    pub order_id: String,
    pub status: String,
    pub condition_id: String,
    pub price: Option<String>,
    pub size: Option<String>,
    pub fee_rate_bps: Option<String>,
    pub transaction_hash: Option<String>,
    pub event_ts: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
struct UserEnvelope {
    event: String,
    #[serde(default)]
    order_id: Option<String>,
    #[serde(default)]
    trade_id: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    condition_id: Option<String>,
    #[serde(default)]
    price: Option<Value>,
    #[serde(default)]
    size: Option<Value>,
    #[serde(default)]
    fee_rate_bps: Option<Value>,
    #[serde(default)]
    transaction_hash: Option<Value>,
    #[serde(default, alias = "timestamp")]
    ts: Option<String>,
}

pub fn parse_user_message(message: &str) -> Result<UserWsEvent, WsParseError> {
    let envelope: UserEnvelope = serde_json::from_str(message)?;
    match envelope.event.trim().to_ascii_uppercase().as_str() {
        "PING" => Ok(UserWsEvent::Ping),
        "PONG" => Ok(UserWsEvent::Pong),
        "ORDER" => Ok(UserWsEvent::Order(UserOrderUpdate {
            order_id: required(envelope.order_id, "order_id")?,
            status: required(envelope.status, "status")?,
            condition_id: required(envelope.condition_id, "condition_id")?,
            price: optional_string(envelope.price, "price")?,
            size: optional_string(envelope.size, "size")?,
            fee_rate_bps: optional_string(envelope.fee_rate_bps, "fee_rate_bps")?,
            transaction_hash: optional_string(envelope.transaction_hash, "transaction_hash")?,
            event_ts: parse_timestamp(envelope.ts)?,
        })),
        "TRADE" => Ok(UserWsEvent::Trade(UserTradeUpdate {
            trade_id: required(envelope.trade_id, "trade_id")?,
            order_id: required(envelope.order_id, "order_id")?,
            status: required(envelope.status, "status")?,
            condition_id: required(envelope.condition_id, "condition_id")?,
            price: optional_string(envelope.price, "price")?,
            size: optional_string(envelope.size, "size")?,
            fee_rate_bps: optional_string(envelope.fee_rate_bps, "fee_rate_bps")?,
            transaction_hash: optional_string(envelope.transaction_hash, "transaction_hash")?,
            event_ts: parse_timestamp(envelope.ts)?,
        })),
        other => Err(WsParseError::UnknownEvent(other.to_owned())),
    }
}

fn required(value: Option<String>, field: &'static str) -> Result<String, WsParseError> {
    value.ok_or(WsParseError::MissingField(field))
}

fn parse_timestamp(value: Option<String>) -> Result<Option<DateTime<Utc>>, WsParseError> {
    let Some(value) = value else {
        return Ok(None);
    };

    let parsed = DateTime::parse_from_rfc3339(&value)
        .map_err(|_| WsParseError::InvalidTimestamp(value.clone()))?;
    Ok(Some(parsed.with_timezone(&Utc)))
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
