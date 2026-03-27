use std::{collections::VecDeque, future::Future, pin::Pin};

use url::Url;
use venue_polymarket::{
    MarketBookUpdate, MarketWsEvent, PolymarketWsClient, UserTradeUpdate, UserWsEvent,
    WsClientError, WsCloseFrame, WsMessageSource, WsTransportMessage,
};

#[tokio::test]
async fn market_ws_client_yields_parsed_market_events_from_scripted_messages() {
    let mut client = PolymarketWsClient::with_transports(
        Url::parse("wss://market.example/ws").expect("market url"),
        Url::parse("wss://user.example/ws").expect("user url"),
        ScriptedWsTransport::new(vec![
            WsTransportMessage::Text(
                r#"{"event":"book","asset_id":"token-1","best_bid":"0.40","best_ask":"0.41"}"#
                    .to_owned(),
            ),
            WsTransportMessage::Pong,
        ]),
        ScriptedWsTransport::new(vec![]),
    );

    let first = client.next_market_event().await.expect("book event");
    let second = client.next_market_event().await.expect("pong event");

    assert_eq!(
        first,
        MarketWsEvent::Book(MarketBookUpdate {
            asset_id: "token-1".to_owned(),
            best_bid: Some("0.40".to_owned()),
            best_ask: Some("0.41".to_owned()),
            event_ts: None,
        })
    );
    assert_eq!(second, MarketWsEvent::Pong);
}

#[tokio::test]
async fn user_ws_client_yields_parsed_user_events_from_scripted_messages() {
    let mut client = PolymarketWsClient::with_transports(
        Url::parse("wss://market.example/ws").expect("market url"),
        Url::parse("wss://user.example/ws").expect("user url"),
        ScriptedWsTransport::new(vec![]),
        ScriptedWsTransport::new(vec![
            WsTransportMessage::Text(
                r#"{
                "event":"trade",
                "trade_id":"trade-1",
                "order_id":"order-1",
                "status":"MATCHED",
                "condition_id":"condition-1",
                "price":"0.41",
                "size":"100",
                "fee_rate_bps":"15",
                "transaction_hash":"0xtrade"
            }"#
                .to_owned(),
            ),
            WsTransportMessage::Ping,
        ]),
    );

    let first = client.next_user_event().await.expect("trade event");
    let second = client.next_user_event().await.expect("ping event");

    assert_eq!(
        first,
        UserWsEvent::Trade(UserTradeUpdate {
            trade_id: "trade-1".to_owned(),
            order_id: "order-1".to_owned(),
            status: "MATCHED".to_owned(),
            condition_id: "condition-1".to_owned(),
            price: Some("0.41".to_owned()),
            size: Some("100".to_owned()),
            fee_rate_bps: Some("15".to_owned()),
            transaction_hash: Some("0xtrade".to_owned()),
            event_ts: None,
        })
    );
    assert_eq!(second, UserWsEvent::Ping);
}

#[tokio::test]
async fn websocket_client_surfaces_close_frame_details_from_scripted_transport() {
    let mut client = PolymarketWsClient::with_transports(
        Url::parse("wss://market.example/ws").expect("market url"),
        Url::parse("wss://user.example/ws").expect("user url"),
        ScriptedWsTransport::new(vec![WsTransportMessage::Close(WsCloseFrame {
            code: 1008,
            label: "policy".to_owned(),
            reason: "session replaced".to_owned(),
        })]),
        ScriptedWsTransport::new(vec![]),
    );

    let error = client
        .next_market_event()
        .await
        .expect_err("close frame should fail parsing path");

    match error {
        WsClientError::Transport(message) => {
            assert!(message.contains("1008"), "actual message: {message}");
            assert!(message.contains("policy"), "actual message: {message}");
            assert!(
                message.contains("session replaced"),
                "actual message: {message}"
            );
        }
        other => panic!("unexpected error: {other}"),
    }
}

#[derive(Debug)]
struct ScriptedWsTransport {
    messages: VecDeque<WsTransportMessage>,
}

impl ScriptedWsTransport {
    fn new(messages: Vec<WsTransportMessage>) -> Self {
        Self {
            messages: VecDeque::from(messages),
        }
    }
}

impl WsMessageSource for ScriptedWsTransport {
    fn next_message<'a>(
        &'a mut self,
    ) -> Pin<Box<dyn Future<Output = Result<WsTransportMessage, WsClientError>> + Send + 'a>> {
        Box::pin(async move {
            self.messages.pop_front().ok_or_else(|| {
                WsClientError::Transport("scripted websocket transport exhausted".to_owned())
            })
        })
    }
}
