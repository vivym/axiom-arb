mod support;

use support::{
    sample_user_stream_auth, scripted_gateway_with_market_events,
    scripted_gateway_with_user_events, scripted_market_trade_event, scripted_user_trade_event,
};

#[tokio::test]
async fn market_stream_projects_repo_owned_market_events() {
    let gateway = scripted_gateway_with_market_events(vec![scripted_market_trade_event()]);

    let events = gateway
        .collect_market_events(vec!["token-1".to_owned()])
        .await
        .unwrap();

    assert!(matches!(
        events[0],
        venue_polymarket::MarketWsEvent::LastTradePrice(_)
    ));
}

#[tokio::test]
async fn user_stream_projects_repo_owned_user_events() {
    let gateway = scripted_gateway_with_user_events(vec![scripted_user_trade_event()]);

    let events = gateway
        .collect_user_events(sample_user_stream_auth(), vec!["condition-1".to_owned()])
        .await
        .unwrap();

    assert!(matches!(events[0], venue_polymarket::UserWsEvent::Trade(_)));
}
