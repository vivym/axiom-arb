mod support;

use std::sync::Arc;

use async_trait::async_trait;
use support::sample_auth_with_funder;
use url::Url;
use venue_polymarket::{
    HeartbeatFetchResult, OpenOrderSummary, PolymarketClobApi, PolymarketGatewayError,
    PolymarketHeartbeatStatus, PolymarketOpenOrderSummary, PolymarketOrderQuery,
    PolymarketRestClient, PolymarketSignedOrder,
};

#[derive(Debug, Clone)]
struct ScriptedLegacyClobApi {
    open_orders: Result<Vec<PolymarketOpenOrderSummary>, PolymarketGatewayError>,
    heartbeat: Result<PolymarketHeartbeatStatus, PolymarketGatewayError>,
}

#[async_trait]
impl PolymarketClobApi for ScriptedLegacyClobApi {
    async fn open_orders(
        &self,
        _query: &PolymarketOrderQuery,
    ) -> Result<Vec<PolymarketOpenOrderSummary>, PolymarketGatewayError> {
        self.open_orders.clone()
    }

    async fn submit_order(
        &self,
        _order: &PolymarketSignedOrder,
    ) -> Result<venue_polymarket::PolymarketSubmitResponse, PolymarketGatewayError> {
        unreachable!("submit_order is not part of the legacy clob shell slice")
    }

    async fn post_heartbeat(
        &self,
        _previous_heartbeat_id: Option<&str>,
    ) -> Result<PolymarketHeartbeatStatus, PolymarketGatewayError> {
        self.heartbeat.clone()
    }
}

#[tokio::test]
async fn legacy_clob_shell_can_delegate_open_orders_without_using_request_builder_auth() {
    let client = sample_client().with_clob_api(Arc::new(ScriptedLegacyClobApi {
        open_orders: Ok(vec![PolymarketOpenOrderSummary {
            order_id: "order-1".to_owned(),
        }]),
        heartbeat: Ok(PolymarketHeartbeatStatus {
            heartbeat_id: "hb-unused".to_owned(),
            valid: true,
        }),
    }));

    let orders = client
        .fetch_open_orders(&sample_auth_with_funder(""))
        .await
        .expect("injected clob api should bypass request-builder auth validation");

    assert_eq!(
        orders,
        vec![OpenOrderSummary {
            order_id: "order-1".to_owned(),
            status: None,
            market: None,
        }]
    );
}

#[tokio::test]
async fn legacy_clob_shell_can_delegate_heartbeat_without_using_request_builder_auth() {
    let client = sample_client().with_clob_api(Arc::new(ScriptedLegacyClobApi {
        open_orders: Ok(Vec::new()),
        heartbeat: Ok(PolymarketHeartbeatStatus {
            heartbeat_id: "hb-42".to_owned(),
            valid: true,
        }),
    }));

    let heartbeat = client
        .post_order_heartbeat(&sample_auth_with_funder(""), "hb-41")
        .await
        .expect("injected clob api should bypass request-builder auth validation");

    assert_eq!(
        heartbeat,
        HeartbeatFetchResult {
            heartbeat_id: "hb-42".to_owned(),
            valid: true,
        }
    );
}

#[test]
fn legacy_clob_shell_request_builders_remain_on_http_path() {
    let client = sample_client().with_clob_api(Arc::new(ScriptedLegacyClobApi {
        open_orders: Ok(Vec::new()),
        heartbeat: Ok(PolymarketHeartbeatStatus {
            heartbeat_id: "hb-unused".to_owned(),
            valid: true,
        }),
    }));
    let request = client
        .build_open_orders_request(&sample_auth_with_funder("0xfunder"))
        .expect("request should still build on the legacy HTTP path");

    assert_eq!(request.method().as_str(), "GET");
    assert_eq!(request.url().path(), "/data/orders");
    assert!(request
        .url()
        .query()
        .expect("query")
        .contains("owner=0xowner"));
}

#[tokio::test]
async fn legacy_clob_shell_surfaces_injected_gateway_errors() {
    let client = sample_client().with_clob_api(Arc::new(ScriptedLegacyClobApi {
        open_orders: Err(PolymarketGatewayError::upstream_response("503: down")),
        heartbeat: Ok(PolymarketHeartbeatStatus {
            heartbeat_id: "hb-unused".to_owned(),
            valid: true,
        }),
    }));

    let err = client
        .fetch_open_orders(&sample_auth_with_funder(""))
        .await
        .expect_err("injected clob api error should surface");

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
