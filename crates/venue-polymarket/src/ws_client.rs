use std::{collections::VecDeque, fmt, future::Future, pin::Pin};

use futures_util::{SinkExt, StreamExt};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_tungstenite::{
    client_async_tls_with_config, connect_async, tungstenite::client::IntoClientRequest,
    tungstenite::Message, Connector, MaybeTlsStream, WebSocketStream,
};
use url::Url;

use crate::proxy::{resolve_proxy_url, ProxyConfigError, ProxyEnvironment};
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

impl From<ProxyConfigError> for WsClientError {
    fn from(value: ProxyConfigError) -> Self {
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
        Self::connect_with_proxy(url, None).await
    }

    pub async fn connect_with_proxy(
        url: Url,
        explicit_proxy_url: Option<&Url>,
    ) -> Result<Self, WsClientError> {
        let env = ProxyEnvironment::from_env();
        if let Some(proxy_url) = resolve_proxy_url(&url, explicit_proxy_url, &env)? {
            return connect_via_http_proxy(url, proxy_url).await;
        }
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
        Self::connect_with_proxy(market_ws_url, user_ws_url, None).await
    }

    pub async fn connect_with_proxy(
        market_ws_url: Url,
        user_ws_url: Url,
        explicit_proxy_url: Option<Url>,
    ) -> Result<Self, WsClientError> {
        let market_transport = RealWsMessageSource::connect_with_proxy(
            market_ws_url.clone(),
            explicit_proxy_url.as_ref(),
        )
        .await?;
        let user_transport = RealWsMessageSource::connect_with_proxy(
            user_ws_url.clone(),
            explicit_proxy_url.as_ref(),
        )
        .await?;
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

async fn connect_via_http_proxy(
    url: Url,
    proxy_url: Url,
) -> Result<RealWsMessageSource, WsClientError> {
    let proxy_authority = authority_for_url(&proxy_url, 80)?;
    let target_authority = authority_for_target(&url)?;
    let mut stream = TcpStream::connect(proxy_authority.as_str())
        .await
        .map_err(|error| {
            WsClientError::Transport(format!(
                "failed to connect to proxy {proxy_authority}: {error}"
            ))
        })?;

    let connect_request = format!(
        "CONNECT {target_authority} HTTP/1.1\r\nHost: {target_authority}\r\nProxy-Connection: Keep-Alive\r\n\r\n"
    );
    stream
        .write_all(connect_request.as_bytes())
        .await
        .map_err(|error| {
            WsClientError::Transport(format!(
                "failed to write proxy CONNECT request for {target_authority}: {error}"
            ))
        })?;
    stream.flush().await.map_err(|error| {
        WsClientError::Transport(format!(
            "failed to flush proxy CONNECT request for {target_authority}: {error}"
        ))
    })?;

    let response = read_proxy_connect_response(&mut stream).await?;
    if !response.starts_with("HTTP/1.1 200") && !response.starts_with("HTTP/1.0 200") {
        let status_line = response.lines().next().unwrap_or("unknown proxy response");
        return Err(WsClientError::Transport(format!(
            "proxy CONNECT {target_authority} failed via {proxy_authority}: {status_line}"
        )));
    }

    let request = url.as_str().into_client_request()?;
    let connector = match url.scheme() {
        "ws" => Some(Connector::Plain),
        "wss" => None,
        scheme => {
            return Err(WsClientError::Transport(format!(
                "unsupported websocket URL scheme '{scheme}'"
            )))
        }
    };
    let (stream, _) = client_async_tls_with_config(request, stream, None, connector).await?;
    Ok(RealWsMessageSource { stream })
}

fn authority_for_target(url: &Url) -> Result<String, WsClientError> {
    let port = url.port_or_known_default().ok_or_else(|| {
        WsClientError::Transport(format!(
            "websocket URL {} is missing a known port",
            url.as_str()
        ))
    })?;
    authority_for_url(url, port)
}

fn authority_for_url(url: &Url, default_port: u16) -> Result<String, WsClientError> {
    let host = url.host_str().ok_or_else(|| {
        WsClientError::Transport(format!("URL {} is missing a host", url.as_str()))
    })?;
    let port = url.port().unwrap_or(default_port);
    Ok(format!("{host}:{port}"))
}

async fn read_proxy_connect_response(stream: &mut TcpStream) -> Result<String, WsClientError> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 1024];

    loop {
        let bytes_read = stream.read(&mut chunk).await.map_err(|error| {
            WsClientError::Transport(format!("failed to read proxy CONNECT response: {error}"))
        })?;
        if bytes_read == 0 {
            return Err(WsClientError::Transport(
                "proxy closed the connection before CONNECT completed".to_owned(),
            ));
        }

        buffer.extend_from_slice(&chunk[..bytes_read]);
        if buffer.windows(4).any(|window| window == b"\r\n\r\n") {
            return String::from_utf8(buffer).map_err(|error| {
                WsClientError::Transport(format!(
                    "proxy CONNECT response was not valid UTF-8: {error}"
                ))
            });
        }
        if buffer.len() >= 16 * 1024 {
            return Err(WsClientError::Transport(
                "proxy CONNECT response exceeded 16 KiB without terminating headers".to_owned(),
            ));
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

    #[test]
    fn resolve_ws_proxy_prefers_explicit_proxy_over_environment() {
        let target = Url::parse("wss://ws-subscriptions-clob.polymarket.com/ws/market")
            .expect("target url should parse");
        let explicit_proxy =
            Url::parse("http://127.0.0.1:7897").expect("explicit proxy should parse");
        let env = ProxyEnvironment {
            http_proxy: None,
            https_proxy: Some("http://127.0.0.1:9999".to_owned()),
            all_proxy: None,
            no_proxy: None,
        };

        let resolved = resolve_proxy_url(&target, Some(&explicit_proxy), &env)
            .expect("explicit proxy should resolve");

        assert_eq!(
            resolved.as_ref().map(Url::as_str),
            Some("http://127.0.0.1:7897/")
        );
    }

    #[test]
    fn resolve_ws_proxy_uses_https_proxy_for_secure_websocket_targets() {
        let target = Url::parse("wss://ws-subscriptions-clob.polymarket.com/ws/market")
            .expect("target url should parse");
        let env = ProxyEnvironment {
            http_proxy: Some("http://127.0.0.1:8888".to_owned()),
            https_proxy: Some("http://127.0.0.1:7897".to_owned()),
            all_proxy: Some("http://127.0.0.1:6666".to_owned()),
            no_proxy: None,
        };

        let resolved =
            resolve_proxy_url(&target, None, &env).expect("environment proxy should resolve");

        assert_eq!(
            resolved.as_ref().map(Url::as_str),
            Some("http://127.0.0.1:7897/")
        );
    }

    #[test]
    fn resolve_ws_proxy_respects_no_proxy_for_environment_proxy() {
        let target = Url::parse("wss://api.polymarket.com/ws").expect("target url should parse");
        let env = ProxyEnvironment {
            http_proxy: None,
            https_proxy: Some("http://127.0.0.1:7897".to_owned()),
            all_proxy: None,
            no_proxy: Some("polymarket.com".to_owned()),
        };

        let resolved = resolve_proxy_url(&target, None, &env)
            .expect("no_proxy should still be a valid resolution path");

        assert!(resolved.is_none());
    }
}
