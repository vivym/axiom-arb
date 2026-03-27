use std::{collections::VecDeque, fmt, future::Future, pin::Pin};

use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::{connect_async, tungstenite::Message, MaybeTlsStream, WebSocketStream};
use url::Url;

use crate::{
    parse_user_message, ws_market::parse_market_messages, MarketWsEvent, UserWsEvent, WsParseError,
};

type WsMessageFuture<'a> =
    Pin<Box<dyn Future<Output = Result<WsTransportMessage, WsClientError>> + Send + 'a>>;
type WsSendFuture<'a> = Pin<Box<dyn Future<Output = Result<(), WsClientError>> + Send + 'a>>;

pub trait WsMessageSource: Send {
    fn next_message<'a>(&'a mut self) -> WsMessageFuture<'a>;

    fn send_message<'a>(&'a mut self, _message: WsTransportMessage) -> WsSendFuture<'a> {
        Box::pin(async {
            Err(WsClientError::Transport(
                "websocket transport does not support sending messages".to_owned(),
            ))
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WsTransportMessage {
    Text(String),
    Binary(Vec<u8>),
    Ping,
    Pong,
    Close(WsCloseFrame),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WsCloseFrame {
    pub code: u16,
    pub label: String,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WsSubscriptionOp {
    Subscribe,
    Unsubscribe,
}

impl WsSubscriptionOp {
    fn as_str(self) -> &'static str {
        match self {
            Self::Subscribe => "subscribe",
            Self::Unsubscribe => "unsubscribe",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WsUserChannelAuth<'a> {
    pub api_key: &'a str,
    pub secret: &'a str,
    pub passphrase: &'a str,
}

#[derive(Debug)]
pub enum WsClientError {
    Transport(String),
    Parse(WsParseError),
}

impl fmt::Display for WsClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Transport(message) => write!(f, "websocket transport error: {message}"),
            Self::Parse(err) => write!(f, "websocket parse error: {err}"),
        }
    }
}

impl std::error::Error for WsClientError {}

impl From<WsParseError> for WsClientError {
    fn from(value: WsParseError) -> Self {
        Self::Parse(value)
    }
}

impl From<tokio_tungstenite::tungstenite::Error> for WsClientError {
    fn from(value: tokio_tungstenite::tungstenite::Error) -> Self {
        Self::Transport(value.to_string())
    }
}

#[derive(Debug)]
pub struct RealWsMessageSource {
    stream: WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
}

#[derive(Debug)]
pub struct PolymarketWsClient<T = RealWsMessageSource> {
    pub market_ws_url: Url,
    pub user_ws_url: Url,
    market_transport: T,
    user_transport: T,
    pending_market_events: VecDeque<MarketWsEvent>,
}

impl RealWsMessageSource {
    pub async fn connect(url: Url) -> Result<Self, WsClientError> {
        let (stream, _) = connect_async(url.as_str()).await?;
        Ok(Self { stream })
    }
}

impl WsMessageSource for RealWsMessageSource {
    fn next_message<'a>(&'a mut self) -> WsMessageFuture<'a> {
        Box::pin(async move {
            loop {
                let Some(message) = self.stream.next().await else {
                    return Err(WsClientError::Transport(
                        "websocket stream ended before a text message arrived".to_owned(),
                    ));
                };

                match map_tungstenite_message(message?)? {
                    Some(message) => return Ok(message),
                    None => continue,
                }
            }
        })
    }

    fn send_message<'a>(&'a mut self, message: WsTransportMessage) -> WsSendFuture<'a> {
        Box::pin(async move {
            let tungstenite_message = map_transport_message_to_tungstenite(message);
            self.stream.send(tungstenite_message).await?;
            Ok(())
        })
    }
}

impl PolymarketWsClient<RealWsMessageSource> {
    pub async fn connect(market_ws_url: Url, user_ws_url: Url) -> Result<Self, WsClientError> {
        let market_transport = RealWsMessageSource::connect(market_ws_url.clone()).await?;
        let user_transport = RealWsMessageSource::connect(user_ws_url.clone()).await?;
        Ok(Self {
            market_ws_url,
            user_ws_url,
            market_transport,
            user_transport,
            pending_market_events: VecDeque::new(),
        })
    }
}

impl<T> PolymarketWsClient<T>
where
    T: WsMessageSource,
{
    pub fn with_transports(
        market_ws_url: Url,
        user_ws_url: Url,
        market_transport: T,
        user_transport: T,
    ) -> Self {
        Self {
            market_ws_url,
            user_ws_url,
            market_transport,
            user_transport,
            pending_market_events: VecDeque::new(),
        }
    }

    pub async fn next_market_event(&mut self) -> Result<MarketWsEvent, WsClientError> {
        if let Some(event) = self.pending_market_events.pop_front() {
            return Ok(event);
        }

        loop {
            let message = self.market_transport.next_message().await?;
            let message = transport_message_to_payload(message)?;
            let mut events = parse_market_messages(&message).map_err(WsClientError::from)?;
            if let Some(event) = events.pop_front() {
                self.pending_market_events.extend(events);
                return Ok(event);
            }
        }
    }

    pub async fn next_user_event(&mut self) -> Result<UserWsEvent, WsClientError> {
        let message = self.user_transport.next_message().await?;
        let message = transport_message_to_payload(message)?;
        parse_user_message(&message).map_err(WsClientError::from)
    }

    pub async fn subscribe_market_assets(
        &mut self,
        asset_ids: &[String],
        custom_feature_enabled: bool,
    ) -> Result<(), WsClientError> {
        self.market_transport
            .send_message(WsTransportMessage::Text(
                serde_json::json!({
                    "assets_ids": asset_ids,
                    "type": "market",
                    "custom_feature_enabled": custom_feature_enabled,
                })
                .to_string(),
            ))
            .await
    }

    pub async fn update_market_assets(
        &mut self,
        operation: WsSubscriptionOp,
        asset_ids: &[String],
        custom_feature_enabled: bool,
    ) -> Result<(), WsClientError> {
        self.market_transport
            .send_message(WsTransportMessage::Text(
                serde_json::json!({
                    "assets_ids": asset_ids,
                    "operation": operation.as_str(),
                    "custom_feature_enabled": custom_feature_enabled,
                })
                .to_string(),
            ))
            .await
    }

    pub async fn subscribe_user_markets(
        &mut self,
        auth: &WsUserChannelAuth<'_>,
        markets: &[String],
    ) -> Result<(), WsClientError> {
        self.user_transport
            .send_message(WsTransportMessage::Text(
                serde_json::json!({
                    "auth": {
                        "apiKey": auth.api_key,
                        "secret": auth.secret,
                        "passphrase": auth.passphrase,
                    },
                    "markets": markets,
                    "type": "user",
                })
                .to_string(),
            ))
            .await
    }

    pub async fn update_user_markets(
        &mut self,
        operation: WsSubscriptionOp,
        markets: &[String],
    ) -> Result<(), WsClientError> {
        self.user_transport
            .send_message(WsTransportMessage::Text(
                serde_json::json!({
                    "markets": markets,
                    "operation": operation.as_str(),
                })
                .to_string(),
            ))
            .await
    }

    pub async fn send_market_ping(&mut self) -> Result<(), WsClientError> {
        self.market_transport
            .send_message(WsTransportMessage::Ping)
            .await
    }

    pub async fn send_user_ping(&mut self) -> Result<(), WsClientError> {
        self.user_transport
            .send_message(WsTransportMessage::Ping)
            .await
    }
}

fn map_tungstenite_message(message: Message) -> Result<Option<WsTransportMessage>, WsClientError> {
    Ok(match message {
        Message::Text(text) => Some(WsTransportMessage::Text(text.to_string())),
        Message::Binary(bytes) => Some(WsTransportMessage::Binary(bytes.to_vec())),
        Message::Ping(_) => Some(WsTransportMessage::Ping),
        Message::Pong(_) => Some(WsTransportMessage::Pong),
        Message::Close(frame) => Some(WsTransportMessage::Close(
            frame
                .map(close_frame_from_tungstenite)
                .unwrap_or(WsCloseFrame {
                    code: 1005,
                    label: "no_status".to_owned(),
                    reason: String::new(),
                }),
        )),
        _ => None,
    })
}

fn map_transport_message_to_tungstenite(message: WsTransportMessage) -> Message {
    match message {
        WsTransportMessage::Text(text) => Message::Text(text.into()),
        WsTransportMessage::Binary(bytes) => Message::Binary(bytes.into()),
        WsTransportMessage::Ping => Message::Text("PING".into()),
        WsTransportMessage::Pong => Message::Text("PONG".into()),
        WsTransportMessage::Close(frame) => {
            Message::Close(Some(tokio_tungstenite::tungstenite::protocol::CloseFrame {
                code: frame.code.into(),
                reason: frame.reason.into(),
            }))
        }
    }
}

fn close_frame_from_tungstenite(
    frame: tokio_tungstenite::tungstenite::protocol::CloseFrame,
) -> WsCloseFrame {
    WsCloseFrame {
        code: u16::from(frame.code),
        label: format!("{:?}", frame.code).to_lowercase(),
        reason: frame.reason.to_string(),
    }
}

fn transport_message_to_payload(message: WsTransportMessage) -> Result<String, WsClientError> {
    match message {
        WsTransportMessage::Text(text) => {
            let trimmed = text.trim();
            if trimmed.eq_ignore_ascii_case("PING") {
                Ok(r#"{"event":"PING"}"#.to_owned())
            } else if trimmed.eq_ignore_ascii_case("PONG") {
                Ok(r#"{"event":"PONG"}"#.to_owned())
            } else {
                Ok(text)
            }
        }
        WsTransportMessage::Binary(bytes) => {
            String::from_utf8(bytes).map_err(|err| WsClientError::Transport(err.to_string()))
        }
        WsTransportMessage::Ping => Ok(r#"{"event":"PING"}"#.to_owned()),
        WsTransportMessage::Pong => Ok(r#"{"event":"PONG"}"#.to_owned()),
        WsTransportMessage::Close(frame) => {
            if frame.reason.is_empty() {
                Err(WsClientError::Transport(format!(
                    "websocket closed with code {} ({})",
                    frame.code, frame.label
                )))
            } else {
                Err(WsClientError::Transport(format!(
                    "websocket closed with code {} ({}): {}",
                    frame.code, frame.label, frame.reason
                )))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn map_tungstenite_message_surfaces_ping_and_pong_control_frames() {
        assert_eq!(
            map_tungstenite_message(Message::Ping(Vec::new().into())).expect("ping"),
            Some(WsTransportMessage::Ping)
        );
        assert_eq!(
            map_tungstenite_message(Message::Pong(Vec::new().into())).expect("pong"),
            Some(WsTransportMessage::Pong)
        );
    }

    #[tokio::test]
    async fn map_tungstenite_message_preserves_close_frame_details() {
        let mapped = map_tungstenite_message(Message::Close(Some(
            tokio_tungstenite::tungstenite::protocol::CloseFrame {
                code: tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode::Policy,
                reason: "session replaced".into(),
            },
        )))
        .expect("close frame");

        assert_eq!(
            mapped,
            Some(WsTransportMessage::Close(WsCloseFrame {
                code: 1008,
                label: "policy".to_owned(),
                reason: "session replaced".to_owned(),
            }))
        );
    }
}
