use chrono::{TimeZone, Utc};
use venue_polymarket::{
    parse_market_message, parse_user_message, MarketBookUpdate, MarketWsEvent, UserTradeUpdate,
    UserWsEvent,
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

fn ts(hour: u32, minute: u32, second: u32) -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 3, 24, hour, minute, second)
        .single()
        .unwrap()
}
