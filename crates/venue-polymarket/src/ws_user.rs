use chrono::{DateTime, TimeZone, Utc};
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
    #[serde(default)]
    event: Option<String>,
    #[serde(default)]
    event_type: Option<String>,
    #[serde(default, rename = "type")]
    type_field: Option<String>,
    #[serde(default)]
    order_id: Option<String>,
    #[serde(default)]
    trade_id: Option<String>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    taker_order_id: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    condition_id: Option<String>,
    #[serde(default, alias = "market")]
    market: Option<String>,
    #[serde(default)]
    price: Option<Value>,
    #[serde(default)]
    size: Option<Value>,
    #[serde(default)]
    original_size: Option<Value>,
    #[serde(default)]
    fee_rate_bps: Option<Value>,
    #[serde(default)]
    transaction_hash: Option<Value>,
    #[serde(default)]
    trade_owner: Option<String>,
    #[serde(default)]
    owner: Option<String>,
    #[serde(default)]
    maker_orders: Option<Vec<MakerOrderEnvelope>>,
    #[serde(default, alias = "timestamp")]
    ts: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct MakerOrderEnvelope {
    #[serde(default)]
    order_id: Option<String>,
    #[serde(default)]
    owner: Option<String>,
}

pub fn parse_user_message(message: &str) -> Result<UserWsEvent, WsParseError> {
    let trimmed = message.trim();
    if trimmed.eq_ignore_ascii_case("PING") {
        return Ok(UserWsEvent::Ping);
    }
    if trimmed.eq_ignore_ascii_case("PONG") {
        return Ok(UserWsEvent::Pong);
    }

    let envelope: UserEnvelope = serde_json::from_str(message)?;
    match normalized_user_event(&envelope)?.as_str() {
        "PING" => Ok(UserWsEvent::Ping),
        "PONG" => Ok(UserWsEvent::Pong),
        "ORDER" => Ok(UserWsEvent::Order(UserOrderUpdate {
            order_id: required(envelope.order_id.or(envelope.id.clone()), "order_id")?,
            status: required(envelope.status.or(envelope.type_field.clone()), "status")?,
            condition_id: required(
                envelope.condition_id.or(envelope.market.clone()),
                "condition_id",
            )?,
            price: optional_string(envelope.price, "price")?,
            size: optional_string(envelope.size.or(envelope.original_size), "size")?,
            fee_rate_bps: optional_string(envelope.fee_rate_bps, "fee_rate_bps")?,
            transaction_hash: optional_string(envelope.transaction_hash, "transaction_hash")?,
            event_ts: parse_timestamp(envelope.ts)?,
        })),
        "TRADE" => Ok(UserWsEvent::Trade(UserTradeUpdate {
            trade_id: required(
                envelope.trade_id.clone().or(envelope.id.clone()),
                "trade_id",
            )?,
            order_id: required(resolve_trade_order_id(&envelope), "order_id")?,
            status: required(envelope.status, "status")?,
            condition_id: required(envelope.condition_id.or(envelope.market), "condition_id")?,
            price: optional_string(envelope.price, "price")?,
            size: optional_string(envelope.size, "size")?,
            fee_rate_bps: optional_string(envelope.fee_rate_bps, "fee_rate_bps")?,
            transaction_hash: optional_string(envelope.transaction_hash, "transaction_hash")?,
            event_ts: parse_timestamp(envelope.ts)?,
        })),
        other => Err(WsParseError::UnknownEvent(other.to_owned())),
    }
}

fn resolve_trade_order_id(envelope: &UserEnvelope) -> Option<String> {
    if let Some(order_id) = envelope.order_id.clone() {
        return Some(order_id);
    }

    let trade_owner = envelope
        .trade_owner
        .as_deref()
        .or(envelope.owner.as_deref());
    if let (Some(trade_owner), Some(maker_orders)) = (trade_owner, envelope.maker_orders.as_ref()) {
        if let Some(order_id) = maker_orders
            .iter()
            .find(|maker_order| maker_order.owner.as_deref() == Some(trade_owner))
            .and_then(|maker_order| maker_order.order_id.clone())
        {
            return Some(order_id);
        }
    }

    envelope.taker_order_id.clone()
}

fn required(value: Option<String>, field: &'static str) -> Result<String, WsParseError> {
    value.ok_or(WsParseError::MissingField(field))
}

fn normalized_user_event(envelope: &UserEnvelope) -> Result<String, WsParseError> {
    envelope
        .event
        .as_deref()
        .or(envelope.event_type.as_deref())
        .map(|value| value.trim().to_ascii_uppercase())
        .ok_or(WsParseError::MissingField("event_type"))
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
