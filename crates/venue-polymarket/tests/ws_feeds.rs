use chrono::{TimeZone, Utc};
use venue_polymarket::{
    parse_market_message, parse_user_message, MarketBookUpdate, MarketWsEvent,
    UserTradeUpdate, UserWsEvent, WsChannelKind, WsChannelLivenessMonitor,
    WsChannelReconcileReason, WsChannelState,
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
            event_ts: Some(ts(10, 0, 1)),
        })
    );
    assert_eq!(
        parse_user_message(r#"{"event":"PONG"}"#).unwrap(),
        UserWsEvent::Pong
    );
}

#[test]
fn ws_market_channel_marks_stale_after_freshness_gap() {
    let monitor = WsChannelLivenessMonitor::new(WsChannelKind::Market, chrono::Duration::seconds(30));
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
    assert_eq!(state.stale_since, Some(ts(10, 0, 41)));
}

#[test]
fn ws_user_channel_clears_stale_state_after_new_message() {
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
        event_ts: Some(ts(10, 0, 32)),
    });
    monitor.record_user_event(&mut state, &trade, ts(10, 0, 32));

    assert_eq!(state.last_message_at, ts(10, 0, 32));
    assert!(!state.requires_reconcile_attention);
    assert_eq!(state.stale_since, None);
    assert_eq!(monitor.reconcile_trigger(&mut state, ts(10, 0, 40)), None);
}

fn ts(hour: u32, minute: u32, second: u32) -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 3, 24, hour, minute, second)
        .single()
        .unwrap()
}
