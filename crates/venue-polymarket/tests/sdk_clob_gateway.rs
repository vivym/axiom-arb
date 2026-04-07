mod support;

use std::str::FromStr;

use polymarket_client_sdk::{
    auth::{Credentials, LocalSigner, Signer, Uuid},
    clob::{Client as SdkClobClient, Config},
    POLYGON,
};
use support::{
    sample_signed_order, scripted_gateway_with_heartbeat, scripted_gateway_with_open_orders,
    scripted_gateway_with_submit_rejection, scripted_open_order, MockServer,
};
use url::Url;
use venue_polymarket::{LiveClobSdkApi, PolymarketGatewayErrorKind};

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

#[tokio::test]
async fn live_sdk_clob_gateway_submits_existing_signed_order_through_sdk() {
    let server = MockServer::spawn(
        "200 OK",
        r#"{"makingAmount":"0","orderID":"order-1","status":"live","success":true,"takingAmount":"0"}"#,
    );
    let gateway = sample_live_sdk_gateway(server.base_url()).await;

    let response = gateway.submit_order(sample_signed_order()).await.unwrap();

    assert_eq!(response.order_id, "order-1");
    assert!(response.success);
    let request = server.finish();
    assert!(request.starts_with("POST /order HTTP/1.1"));
    assert!(request.contains(r#""owner":"550e8400-e29b-41d4-a716-446655440000""#));
    assert!(request.contains(r#""orderType":"GTC""#));
}

async fn sample_live_sdk_gateway(base_url: Url) -> venue_polymarket::PolymarketGateway {
    let signer =
        LocalSigner::from_str("0x59c6995e998f97a5a004497e5d5f3d4a7e4f6f7a4d4c3b2a1908070605040302")
            .expect("local signer")
            .with_chain_id(Some(POLYGON));
    let clob = SdkClobClient::new(base_url.as_str(), Config::default())
        .expect("sdk clob client")
        .authentication_builder(&signer)
        .credentials(sample_credentials())
        .authenticate()
        .await
        .expect("authenticated sdk client");

    venue_polymarket::PolymarketGateway::from_clob_api(std::sync::Arc::new(LiveClobSdkApi::new(
        clob,
    )))
}

fn sample_credentials() -> Credentials {
    Credentials::new(
        Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").expect("uuid"),
        "secret-1".to_owned(),
        "passphrase-1".to_owned(),
    )
}
