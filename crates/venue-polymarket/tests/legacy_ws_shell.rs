mod support;

use std::{future::Future, pin::Pin};

use support::{
    scripted_gateway_with_market_events, scripted_gateway_with_user_events,
    scripted_market_trade_event, scripted_user_trade_event,
};
use url::Url;
use venue_polymarket::{
    MarketWsEvent, PolymarketWsClient, UserWsEvent, WsClientError, WsMessageSource,
    WsTransportMessage, WsUserChannelAuth,
};

#[tokio::test]
async fn gateway_backed_market_ws_client_reads_gateway_events_without_transport_reads() {
    let mut client = PolymarketWsClient::with_gateway(
        Url::parse("wss://market.example/ws").unwrap(),
        Url::parse("wss://user.example/ws").unwrap(),
        FailOnReadTransport,
        FailOnReadTransport,
        scripted_gateway_with_market_events(vec![scripted_market_trade_event()]),
        None,
    );

    client
        .subscribe_market_assets(&["token-1".to_owned()], false)
        .await
        .expect("market subscribe should succeed");

    let event = client
        .next_market_event()
        .await
        .expect("gateway market event");
    assert!(matches!(event, MarketWsEvent::LastTradePrice(_)));
}

#[tokio::test]
async fn gateway_backed_user_ws_client_reads_gateway_events_without_transport_reads() {
    let mut client = PolymarketWsClient::with_gateway(
        Url::parse("wss://market.example/ws").unwrap(),
        Url::parse("wss://user.example/ws").unwrap(),
        FailOnReadTransport,
        FailOnReadTransport,
        scripted_gateway_with_user_events(vec![scripted_user_trade_event()]),
        Some("0x1111111111111111111111111111111111111111".to_owned()),
    );

    client
        .subscribe_user_markets(
            &WsUserChannelAuth {
                api_key: "550e8400-e29b-41d4-a716-446655440000",
                secret: "secret-1",
                passphrase: "passphrase-1",
            },
            &["condition-1".to_owned()],
        )
        .await
        .expect("user subscribe should succeed");

    let event = client.next_user_event().await.expect("gateway user event");
    assert!(matches!(event, UserWsEvent::Trade(_)));
}

#[tokio::test]
async fn gateway_backed_market_ws_client_requires_subscription_before_polling() {
    let mut client = PolymarketWsClient::with_gateway(
        Url::parse("wss://market.example/ws").unwrap(),
        Url::parse("wss://user.example/ws").unwrap(),
        FailOnReadTransport,
        FailOnReadTransport,
        scripted_gateway_with_market_events(vec![scripted_market_trade_event()]),
        None,
    );

    let error = client
        .next_market_event()
        .await
        .expect_err("missing subscription should fail");

    match error {
        WsClientError::Transport(message) => {
            assert!(message.contains("no market assets subscribed"));
        }
        other => panic!("unexpected error: {other}"),
    }
}

#[derive(Debug, Clone, Copy)]
struct FailOnReadTransport;

impl WsMessageSource for FailOnReadTransport {
    fn next_message<'a>(
        &'a mut self,
    ) -> Pin<Box<dyn Future<Output = Result<WsTransportMessage, WsClientError>> + Send + 'a>> {
        Box::pin(async {
            Err(WsClientError::Transport(
                "legacy transport should not be read in gateway-backed mode".to_owned(),
            ))
        })
    }

    fn send_message<'a>(
        &'a mut self,
        _message: WsTransportMessage,
    ) -> Pin<Box<dyn Future<Output = Result<(), WsClientError>> + Send + 'a>> {
        Box::pin(async {
            Err(WsClientError::Transport(
                "legacy transport should not be written in gateway-backed mode".to_owned(),
            ))
        })
    }
}
