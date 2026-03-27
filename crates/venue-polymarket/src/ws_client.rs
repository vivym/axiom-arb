use std::{fmt, future::Future, pin::Pin};

use futures_util::StreamExt;
use tokio_tungstenite::{connect_async, tungstenite::Message, MaybeTlsStream, WebSocketStream};
use url::Url;

use crate::{parse_market_message, parse_user_message, MarketWsEvent, UserWsEvent, WsParseError};

type WsMessageFuture<'a> = Pin<Box<dyn Future<Output = Result<String, WsClientError>> + Send + 'a>>;

pub trait WsMessageSource: Send {
    fn next_message<'a>(&'a mut self) -> WsMessageFuture<'a>;
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

                match message? {
                    Message::Text(text) => return Ok(text.to_string()),
                    Message::Binary(bytes) => {
                        return String::from_utf8(bytes.to_vec())
                            .map_err(|err| WsClientError::Transport(err.to_string()));
                    }
                    Message::Ping(_) | Message::Pong(_) => continue,
                    Message::Close(_) => {
                        return Err(WsClientError::Transport("websocket closed".to_owned()));
                    }
                    _ => continue,
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
        parse_market_message(&message).map_err(WsClientError::from)
    }

    pub async fn next_user_event(&mut self) -> Result<UserWsEvent, WsClientError> {
        let message = self.user_transport.next_message().await?;
        parse_user_message(&message).map_err(WsClientError::from)
    }
}
