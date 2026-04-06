mod support;

use std::sync::Arc;

use async_trait::async_trait;
use support::{sample_builder_relayer_auth, sample_relayer_api_auth};
use url::Url;
use venue_polymarket::{
    PolymarketGatewayError, PolymarketRelayerApi, PolymarketRestClient, RelayerTransaction,
    RelayerTransactionType,
};

#[derive(Debug, Clone)]
struct ScriptedLegacyRelayerApi {
    recent_transactions: Result<Vec<RelayerTransaction>, PolymarketGatewayError>,
    current_nonce: Result<String, PolymarketGatewayError>,
}

#[async_trait]
impl PolymarketRelayerApi for ScriptedLegacyRelayerApi {
    async fn recent_transactions(
        &self,
        _auth: &venue_polymarket::RelayerAuth<'_>,
    ) -> Result<Vec<RelayerTransaction>, PolymarketGatewayError> {
        self.recent_transactions.clone()
    }

    async fn current_nonce(
        &self,
        _auth: &venue_polymarket::RelayerAuth<'_>,
        _address: &str,
        _wallet_type: RelayerTransactionType,
    ) -> Result<String, PolymarketGatewayError> {
        self.current_nonce.clone()
    }
}

fn sample_transaction(transaction_id: &str) -> RelayerTransaction {
    serde_json::from_str(&format!(
        r#"{{"transactionID":"{transaction_id}","transactionHash":"0x{transaction_id}","state":"STATE_CONFIRMED","type":"SAFE"}}"#
    ))
    .expect("sample transaction should deserialize")
}

#[tokio::test]
async fn legacy_relayer_shell_can_delegate_recent_transactions() {
    let client = sample_client().with_relayer_api(Arc::new(ScriptedLegacyRelayerApi {
        recent_transactions: Ok(vec![sample_transaction("tx-1")]),
        current_nonce: Ok("31".to_owned()),
    }));

    let transactions = client
        .fetch_recent_transactions(&sample_builder_relayer_auth())
        .await
        .expect("injected relayer api should delegate recent transactions");

    assert_eq!(transactions.len(), 1);
    assert_eq!(transactions[0].transaction_id, "tx-1");
    assert_eq!(
        transactions[0].wallet_type,
        Some(RelayerTransactionType::Safe)
    );
}

#[tokio::test]
async fn legacy_relayer_shell_can_delegate_current_nonce() {
    let client = sample_client().with_relayer_api(Arc::new(ScriptedLegacyRelayerApi {
        recent_transactions: Ok(vec![sample_transaction("tx-1")]),
        current_nonce: Ok("31".to_owned()),
    }));

    let nonce = client
        .fetch_current_nonce(
            &sample_relayer_api_auth(),
            "0x5555555555555555555555555555555555555555",
            RelayerTransactionType::Proxy,
        )
        .await
        .expect("injected relayer api should delegate current nonce");

    assert_eq!(nonce, "31");
}

#[tokio::test]
async fn legacy_relayer_shell_surfaces_injected_gateway_errors() {
    let client = sample_client().with_relayer_api(Arc::new(ScriptedLegacyRelayerApi {
        recent_transactions: Err(PolymarketGatewayError::upstream_response("503: down")),
        current_nonce: Ok("31".to_owned()),
    }));

    let err = client
        .fetch_recent_transactions(&sample_builder_relayer_auth())
        .await
        .expect_err("injected relayer api error should surface");

    match err {
        venue_polymarket::RestError::Gateway(inner) => {
            assert_eq!(
                inner.kind,
                venue_polymarket::PolymarketGatewayErrorKind::UpstreamResponse
            );
            assert!(inner.message.contains("503: down"));
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

fn sample_client() -> PolymarketRestClient {
    let client = reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("test client");
    let base_url = Url::parse("http://127.0.0.1:1/").expect("base url");

    PolymarketRestClient::with_http_client(
        client,
        base_url.clone(),
        base_url.clone(),
        base_url,
        None,
    )
}
