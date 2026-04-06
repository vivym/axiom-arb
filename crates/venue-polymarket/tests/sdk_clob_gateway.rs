mod support;

use support::{
    sample_signed_order, scripted_gateway_with_heartbeat, scripted_gateway_with_open_orders,
    scripted_gateway_with_submit_rejection, scripted_open_order,
};
use venue_polymarket::PolymarketGatewayErrorKind;

#[tokio::test]
async fn gateway_open_orders_maps_sdk_rows() {
    let gateway = scripted_gateway_with_open_orders(vec![scripted_open_order("order-1")]);

    let orders = gateway
        .open_orders(venue_polymarket::PolymarketOrderQuery::open_orders())
        .await
        .unwrap();

    assert_eq!(orders[0].order_id, "order-1");
}

#[tokio::test]
async fn gateway_heartbeat_maps_success_response() {
    let gateway = scripted_gateway_with_heartbeat("hb-1");

    let heartbeat = gateway.post_heartbeat(Some("hb-0")).await.unwrap();

    assert_eq!(heartbeat.heartbeat_id, "hb-1");
    assert!(heartbeat.valid);
}

#[tokio::test]
async fn gateway_submit_maps_upstream_rejection_to_upstream_response_error() {
    let gateway = scripted_gateway_with_submit_rejection(401, "{\"error\":\"bad auth\"}");

    let error = gateway
        .submit_order(sample_signed_order())
        .await
        .unwrap_err();

    assert_eq!(error.kind, PolymarketGatewayErrorKind::UpstreamResponse);
}
