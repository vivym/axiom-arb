use std::{fmt, future::Future, pin::Pin};

use futures_util::StreamExt;
use tokio_tungstenite::{connect_async, tungstenite::Message, MaybeTlsStream, WebSocketStream};
use url::Url;

use crate::{parse_market_message, parse_user_message, MarketWsEvent, UserWsEvent, WsParseError};

type WsMessageFuture<'a> =
    Pin<Box<dyn Future<Output = Result<WsTransportMessage, WsClientError>> + Send + 'a>>;

pub trait WsMessageSource: Send {
    fn next_message<'a>(&'a mut self) -> WsMessageFuture<'a>;
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
        }
    }

    pub async fn next_market_event(&mut self) -> Result<MarketWsEvent, WsClientError> {
        let message = self.market_transport.next_message().await?;
        let message = transport_message_to_payload(message)?;
        parse_market_message(&message).map_err(WsClientError::from)
    }

    pub async fn next_user_event(&mut self) -> Result<UserWsEvent, WsClientError> {
        let message = self.user_transport.next_message().await?;
        let message = transport_message_to_payload(message)?;
        parse_user_message(&message).map_err(WsClientError::from)
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
        WsTransportMessage::Text(text) => Ok(text),
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
