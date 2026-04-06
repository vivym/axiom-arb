mod support;

use std::sync::Arc;

use serde_json::json;
use support::{
    sample_builder_relayer_auth, sample_client_for, sample_relayer_api_auth,
    scripted_gateway_with_open_orders, scripted_gateway_with_relayer, MockServer,
};
use venue_polymarket::{
    LiveRelayerApi, PolymarketGateway, PolymarketGatewayErrorKind, RelayerTransaction,
    RelayerTransactionType,
};

fn sample_transaction(transaction_id: &str) -> RelayerTransaction {
    serde_json::from_value(json!({
        "transactionID": transaction_id,
        "transactionHash": format!("0x{transaction_id}"),
        "state": "STATE_PENDING",
        "type": "SAFE",
        "createdAt": "2026-04-06T00:00:00Z"
    }))
    .expect("sample transaction should deserialize")
}

#[tokio::test]
async fn gateway_recent_transactions_forwards_to_scripted_relayer_api() {
    let transaction = sample_transaction("tx-1");
    let gateway = scripted_gateway_with_relayer(Ok(vec![transaction.clone()]), Ok("8".to_owned()));

    let transactions = gateway
        .recent_transactions(&sample_builder_relayer_auth())
        .await
        .unwrap();

    assert_eq!(transactions, vec![transaction]);
}

#[tokio::test]
async fn gateway_current_nonce_forwards_to_scripted_relayer_api() {
    let gateway =
        scripted_gateway_with_relayer(Ok(vec![sample_transaction("tx-1")]), Ok("31".to_owned()));

    let nonce = gateway
        .current_nonce(
            &sample_relayer_api_auth(),
            "0x5555555555555555555555555555555555555555",
            RelayerTransactionType::Proxy,
        )
        .await
        .unwrap();

    assert_eq!(nonce, "31");
}

#[tokio::test]
async fn gateway_recent_transactions_uses_live_relayer_backend() {
    let server = MockServer::spawn(
        "200 OK",
        r#"[{"transactionID":"tx-live-1","transactionHash":"0xlive","state":"STATE_CONFIRMED","type":"SAFE","createdAt":"2026-04-06T00:00:00Z"}]"#,
    );
    let client = sample_client_for(server.base_url());
    let gateway = PolymarketGateway::from_relayer_api(Arc::new(LiveRelayerApi::new(client)));

    let transactions = gateway
        .recent_transactions(&sample_builder_relayer_auth())
        .await
        .unwrap();
    let request = server.finish();

    assert!(request.starts_with("GET /transactions HTTP/1.1"));
    assert!(request.contains("poly-builder-api-key: builder-key-1"));
    assert_eq!(transactions[0].transaction_id, "tx-live-1");
    assert_eq!(
        transactions[0].wallet_type,
        Some(RelayerTransactionType::Safe)
    );
}

#[tokio::test]
async fn gateway_recent_transactions_maps_live_relayer_upstream_errors() {
    let server = MockServer::spawn("503 Service Unavailable", r#"{"error":"down"}"#);
    let client = sample_client_for(server.base_url());
    let gateway = PolymarketGateway::from_relayer_api(Arc::new(LiveRelayerApi::new(client)));

    let error = gateway
        .recent_transactions(&sample_builder_relayer_auth())
        .await
        .expect_err("relayer upstream failure should surface as gateway error");
    let request = server.finish();

    assert!(request.starts_with("GET /transactions HTTP/1.1"));
    assert_eq!(error.kind, PolymarketGatewayErrorKind::UpstreamResponse);
}

#[tokio::test]
async fn gateway_current_nonce_uses_live_relayer_backend() {
    let server = MockServer::spawn("200 OK", r#"{"nonce":"31"}"#);
    let client = sample_client_for(server.base_url());
    let gateway = PolymarketGateway::from_relayer_api(Arc::new(LiveRelayerApi::new(client)));

    let nonce = gateway
        .current_nonce(
            &sample_relayer_api_auth(),
            "0x5555555555555555555555555555555555555555",
            RelayerTransactionType::Proxy,
        )
        .await
        .unwrap();
    let request = server.finish();

    assert!(request.starts_with("GET /nonce?"));
    assert!(request.contains("address=0x5555555555555555555555555555555555555555"));
    assert!(request.contains("type=PROXY"));
    assert_eq!(nonce, "31");
}

#[tokio::test]
async fn gateway_recent_transactions_requires_relayer_backend() {
    let gateway = scripted_gateway_with_open_orders(Vec::new());

    let error = gateway
        .recent_transactions(&sample_builder_relayer_auth())
        .await
        .expect_err("missing relayer backend should fail");

    assert_eq!(error.kind, PolymarketGatewayErrorKind::Protocol);
    assert!(error.message.contains("relayer api is not configured"));
}
