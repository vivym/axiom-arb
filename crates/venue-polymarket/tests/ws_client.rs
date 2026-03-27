use std::{collections::VecDeque, future::Future, pin::Pin};

use venue_polymarket::{
    MarketBookUpdate, MarketWsEvent, PolymarketWsClient, Url, UserTradeUpdate, UserWsEvent,
    WsMessageSource,
};

#[tokio::test]
async fn market_ws_client_yields_parsed_market_events_from_scripted_messages() {
    let mut client = PolymarketWsClient::with_transports(
        Url::parse("wss://market.example/ws").expect("market url"),
        Url::parse("wss://user.example/ws").expect("user url"),
        ScriptedWsTransport::new(vec![
            r#"{"event":"book","asset_id":"token-1","best_bid":"0.40","best_ask":"0.41"}"#
                .to_owned(),
            r#"{"event":"PONG"}"#.to_owned(),
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
            r#"{"event":"PING"}"#.to_owned(),
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

#[derive(Debug)]
struct ScriptedWsTransport {
    messages: VecDeque<String>,
}

impl ScriptedWsTransport {
    fn new(messages: Vec<String>) -> Self {
        Self {
            messages: VecDeque::from(messages),
        }
    }
}

impl WsMessageSource for ScriptedWsTransport {
    fn next_message<'a>(
        &'a mut self,
    ) -> Pin<Box<dyn Future<Output = Result<String, venue_polymarket::WsClientError>> + Send + 'a>>
    {
        Box::pin(async move {
            self.messages.pop_front().ok_or_else(|| {
                venue_polymarket::WsClientError::Transport(
                    "scripted websocket transport exhausted".to_owned(),
                )
            })
        })
    }
}
