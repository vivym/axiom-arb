use std::{collections::VecDeque, future::Future, pin::Pin};

use chrono::{TimeZone, Utc};
use venue_polymarket::{
    parse_market_message, parse_user_message, MarketBookUpdate, MarketLifecycleUpdate,
    MarketPriceChangeUpdate, MarketTickSizeChangeUpdate, MarketTradePriceUpdate, MarketWsEvent,
    PolymarketWsClient, Url, UserOrderUpdate, UserTradeUpdate, UserWsEvent, WsChannelKind,
    WsChannelLivenessMonitor, WsChannelReconcileReason, WsChannelState, WsMessageSource,
    WsParseError,
};

#[test]
fn ws_market_parses_book_update_and_ping_pong() {
    let book = parse_market_message(
        r#"{
            "event":"book",
            "asset_id":"token-yes",
            "best_bid":"0.45",
            "best_ask":"0.47",
            "timestamp":"2026-03-24T10:00:00Z"
        }"#,
    )
    .expect("book message should parse");

    assert_eq!(
        book,
        MarketWsEvent::Book(MarketBookUpdate {
            asset_id: "token-yes".to_owned(),
            best_bid: Some("0.45".to_owned()),
            best_ask: Some("0.47".to_owned()),
            event_ts: Some(ts(10, 0, 0)),
        })
    );
    assert_eq!(
        parse_market_message(r#"{"event":"PING"}"#).unwrap(),
        MarketWsEvent::Ping
    );
    assert_eq!(
        parse_market_message(r#"{"event":"PONG"}"#).unwrap(),
        MarketWsEvent::Pong
    );
}

#[test]
fn ws_user_parses_trade_and_pong() {
    let trade = parse_user_message(
        r#"{
            "event":"trade",
            "trade_id":"trade-1",
            "order_id":"order-1",
            "status":"MATCHED",
            "condition_id":"condition-1",
            "price":"0.41",
            "size":"100",
            "fee_rate_bps":"15",
            "transaction_hash":"0xtrade",
            "timestamp":"2026-03-24T10:00:01Z"
        }"#,
    )
    .expect("trade message should parse");

    assert_eq!(
        trade,
        UserWsEvent::Trade(UserTradeUpdate {
            trade_id: "trade-1".to_owned(),
            order_id: "order-1".to_owned(),
            status: "MATCHED".to_owned(),
            condition_id: "condition-1".to_owned(),
            price: Some("0.41".to_owned()),
            size: Some("100".to_owned()),
            fee_rate_bps: Some("15".to_owned()),
            transaction_hash: Some("0xtrade".to_owned()),
            event_ts: Some(ts(10, 0, 1)),
        })
    );
    assert_eq!(
        parse_user_message(r#"{"event":"PONG"}"#).unwrap(),
        UserWsEvent::Pong
    );
}

#[test]
fn ws_market_parses_price_tick_trade_and_lifecycle_events() {
    assert_eq!(
        parse_market_message(
            r#"{
                "event":"price_change",
                "asset_id":"token-yes",
                "price":"0.46",
                "side":"BUY",
                "timestamp":"2026-03-24T10:00:02Z"
            }"#
        )
        .unwrap(),
        MarketWsEvent::PriceChange(MarketPriceChangeUpdate {
            asset_id: "token-yes".to_owned(),
            price: "0.46".to_owned(),
            side: Some("BUY".to_owned()),
            event_ts: Some(ts(10, 0, 2)),
        })
    );
    assert_eq!(
        parse_market_message(
            r#"{
                "event":"last_trade_price",
                "asset_id":"token-yes",
                "last_trade_price":"0.47",
                "size":"55",
                "timestamp":"2026-03-24T10:00:03Z"
            }"#
        )
        .unwrap(),
        MarketWsEvent::LastTradePrice(MarketTradePriceUpdate {
            asset_id: "token-yes".to_owned(),
            price: "0.47".to_owned(),
            size: Some("55".to_owned()),
            event_ts: Some(ts(10, 0, 3)),
        })
    );
    assert_eq!(
        parse_market_message(
            r#"{
                "event":"tick_size_change",
                "asset_id":"token-yes",
                "previous_tick_size":"0.01",
                "tick_size":"0.005",
                "timestamp":"2026-03-24T10:00:04Z"
            }"#
        )
        .unwrap(),
        MarketWsEvent::TickSizeChange(MarketTickSizeChangeUpdate {
            asset_id: "token-yes".to_owned(),
            previous_tick_size: Some("0.01".to_owned()),
            tick_size: "0.005".to_owned(),
            event_ts: Some(ts(10, 0, 4)),
        })
    );
    assert_eq!(
        parse_market_message(
            r#"{
                "event":"status",
                "market_id":"market-1",
                "asset_id":"token-yes",
                "status":"HALTED",
                "timestamp":"2026-03-24T10:00:05Z"
            }"#
        )
        .unwrap(),
        MarketWsEvent::Lifecycle(MarketLifecycleUpdate {
            market_id: Some("market-1".to_owned()),
            asset_id: Some("token-yes".to_owned()),
            status: "HALTED".to_owned(),
            event_ts: Some(ts(10, 0, 5)),
        })
    );
}

#[test]
fn ws_user_parses_order_and_trade_settlement_fields() {
    assert_eq!(
        parse_user_message(
            r#"{
                "event":"order",
                "order_id":"order-10",
                "status":"LIVE",
                "condition_id":"condition-10",
                "price":"0.52",
                "size":"250",
                "fee_rate_bps":"20",
                "transaction_hash":"0xorder",
                "timestamp":"2026-03-24T10:00:06Z"
            }"#
        )
        .unwrap(),
        UserWsEvent::Order(UserOrderUpdate {
            order_id: "order-10".to_owned(),
            status: "LIVE".to_owned(),
            condition_id: "condition-10".to_owned(),
            price: Some("0.52".to_owned()),
            size: Some("250".to_owned()),
            fee_rate_bps: Some("20".to_owned()),
            transaction_hash: Some("0xorder".to_owned()),
            event_ts: Some(ts(10, 0, 6)),
        })
    );
}

#[test]
fn ws_market_rejects_unknown_event_variants() {
    assert_eq!(
        parse_market_message(r#"{"event":"mystery"}"#)
            .unwrap_err()
            .to_string(),
        WsParseError::UnknownEvent("MYSTERY".to_owned()).to_string()
    );
}

#[test]
fn ws_market_channel_marks_stale_after_freshness_gap() {
    let monitor =
        WsChannelLivenessMonitor::new(WsChannelKind::Market, chrono::Duration::seconds(30));
    let mut state = WsChannelState::new(WsChannelKind::Market, ts(10, 0, 0));

    monitor.record_market_event(&mut state, &MarketWsEvent::Ping, ts(10, 0, 10));

    assert_eq!(state.last_ping_at, Some(ts(10, 0, 10)));
    assert_eq!(
        monitor.reconcile_trigger(&mut state, ts(10, 0, 41)),
        Some(WsChannelReconcileReason::StaleChannel {
            channel: WsChannelKind::Market,
        })
    );
    assert!(state.requires_reconcile_attention);
    assert_eq!(state.stale_since, Some(ts(10, 0, 40)));
    assert_eq!(monitor.reconcile_trigger(&mut state, ts(10, 0, 50)), None);
}

#[test]
fn ws_user_channel_keeps_reconcile_attention_latched_after_new_message() {
    let monitor = WsChannelLivenessMonitor::new(WsChannelKind::User, chrono::Duration::seconds(30));
    let mut state = WsChannelState::new(WsChannelKind::User, ts(10, 0, 0));

    assert_eq!(
        monitor.reconcile_trigger(&mut state, ts(10, 0, 31)),
        Some(WsChannelReconcileReason::StaleChannel {
            channel: WsChannelKind::User,
        })
    );
    assert!(state.requires_reconcile_attention);

    let trade = UserWsEvent::Trade(UserTradeUpdate {
        trade_id: "trade-2".to_owned(),
        order_id: "order-2".to_owned(),
        status: "MATCHED".to_owned(),
        condition_id: "condition-2".to_owned(),
        price: None,
        size: None,
        fee_rate_bps: None,
        transaction_hash: None,
        event_ts: Some(ts(10, 0, 32)),
    });
    monitor.record_user_event(&mut state, &trade, ts(10, 0, 32));

    assert_eq!(state.last_message_at, ts(10, 0, 32));
    assert!(state.requires_reconcile_attention);
    assert_eq!(state.stale_since, Some(ts(10, 0, 30)));
    assert_eq!(monitor.reconcile_trigger(&mut state, ts(10, 0, 40)), None);
}

#[test]
fn ws_channel_requires_explicit_reset_to_clear_reconcile_attention() {
    let monitor =
        WsChannelLivenessMonitor::new(WsChannelKind::Market, chrono::Duration::seconds(30));
    let mut state = WsChannelState::new(WsChannelKind::Market, ts(10, 0, 0));

    assert_eq!(
        monitor.reconcile_trigger(&mut state, ts(10, 0, 31)),
        Some(WsChannelReconcileReason::StaleChannel {
            channel: WsChannelKind::Market,
        })
    );
    assert!(state.requires_reconcile_attention);
    assert_eq!(state.stale_since, Some(ts(10, 0, 30)));

    monitor.reset_reconcile_attention(&mut state, ts(10, 0, 45));

    assert!(!state.requires_reconcile_attention);
    assert_eq!(state.stale_since, None);
    assert_eq!(state.last_message_at, ts(10, 0, 45));
}

#[tokio::test]
async fn ws_client_market_events_still_drive_existing_liveness_monitor() {
    let mut client = PolymarketWsClient::with_transports(
        Url::parse("wss://market.example/ws").expect("market url"),
        Url::parse("wss://user.example/ws").expect("user url"),
        ScriptedWsTransport::new(vec![r#"{"event":"PING"}"#.to_owned()]),
        ScriptedWsTransport::new(vec![]),
    );
    let monitor =
        WsChannelLivenessMonitor::new(WsChannelKind::Market, chrono::Duration::seconds(30));
    let mut state = WsChannelState::new(WsChannelKind::Market, ts(10, 0, 0));

    let event = client.next_market_event().await.expect("market event");
    monitor.record_market_event(&mut state, &event, ts(10, 0, 10));

    assert_eq!(event, MarketWsEvent::Ping);
    assert_eq!(state.last_message_at, ts(10, 0, 10));
    assert_eq!(state.last_ping_at, Some(ts(10, 0, 10)));
    assert!(!state.requires_reconcile_attention);
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

fn ts(hour: u32, minute: u32, second: u32) -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 3, 24, hour, minute, second)
        .single()
        .unwrap()
}
