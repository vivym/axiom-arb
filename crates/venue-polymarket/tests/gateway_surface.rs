use serde_json::json;
use venue_polymarket::{
    PolymarketGateway, PolymarketGatewayError, PolymarketGatewayErrorKind, PolymarketOrderQuery,
    PolymarketSignedOrder,
};

#[test]
fn gateway_surface_exposes_route_agnostic_signed_order_dto() {
    let _gateway_type_check: Option<PolymarketGateway> = None;

    let order = PolymarketSignedOrder {
        order: json!({"tokenId": "token-1"}),
        owner: "0x1111111111111111111111111111111111111111".to_owned(),
        order_type: "GTC".to_owned(),
        defer_exec: false,
    };

    assert_eq!(order.owner, "0x1111111111111111111111111111111111111111");
}

#[test]
fn gateway_error_categories_are_stable() {
    let error = PolymarketGatewayError::auth("invalid api key");
    assert_eq!(error.kind, PolymarketGatewayErrorKind::Auth);
    assert!(error.to_string().contains("invalid api key"));
}

#[test]
fn order_query_does_not_encode_route_specific_fields() {
    let query = PolymarketOrderQuery::open_orders();
    let debug = format!("{query:?}");
    assert!(!debug.contains("family"));
    assert!(!debug.contains("neg-risk"));
}
