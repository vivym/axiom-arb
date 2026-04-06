mod support;

use std::sync::Arc;

use httpmock::{Method::GET, MockServer};
use polymarket_client_sdk::gamma::Client as SdkGammaClient;
use serde_json::json;
use support::{
    scripted_gateway_with_all_malformed_metadata,
    scripted_gateway_with_valid_and_malformed_metadata,
};
use venue_polymarket::{LiveMetadataSdkApi, PolymarketGateway, PolymarketGatewayErrorKind};

#[tokio::test]
async fn metadata_gateway_skips_malformed_neg_risk_rows_but_keeps_valid_rows() {
    let gateway = scripted_gateway_with_valid_and_malformed_metadata();

    let rows = gateway.refresh_neg_risk_metadata().await.unwrap();

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].event_id, "event-valid-1");
    assert_eq!(rows[0].condition_id, "condition-valid-1");
    assert_eq!(rows[0].token_id, "token-valid-1");
}

#[tokio::test]
async fn metadata_gateway_fails_closed_when_all_rows_are_malformed() {
    let gateway = scripted_gateway_with_all_malformed_metadata();

    let error = gateway.refresh_neg_risk_metadata().await.unwrap_err();

    assert_eq!(error.kind, PolymarketGatewayErrorKind::Policy);
    assert!(error.message.contains("no valid rows"));
}

#[tokio::test]
async fn live_metadata_sdk_api_fetches_gamma_events_through_gateway() {
    let server = MockServer::start();
    let first_page = server.mock(|when, then| {
        when.method(GET)
            .path("/events")
            .query_param("limit", "100")
            .query_param("offset", "0")
            .query_param("active", "true")
            .query_param("closed", "false");
        then.status(200).json_body(json!([
            {
                "id": "event-live-1",
                "title": "Live Event",
                "parentEvent": "family-live-1",
                "negRisk": true,
                "enableNegRisk": true,
                "negRiskAugmented": false,
                "markets": [
                    {
                        "id": "market-live-1",
                        "conditionId": "0x1111111111111111111111111111111111111111111111111111111111111111",
                        "clobTokenIds": "[\"123456789\"]",
                        "outcomes": "[\"Live\"]",
                        "shortOutcomes": "[\"Live\"]",
                        "negRisk": true,
                        "negRiskOther": false
                    }
                ]
            }
        ]));
    });
    let second_page = server.mock(|when, then| {
        when.method(GET)
            .path("/events")
            .query_param("limit", "100")
            .query_param("offset", "100")
            .query_param("active", "true")
            .query_param("closed", "false");
        then.status(200).json_body(json!([]));
    });
    let sdk_client = SdkGammaClient::new(&server.base_url()).expect("sdk gamma client");
    let gateway =
        PolymarketGateway::from_metadata_api(Arc::new(LiveMetadataSdkApi::new(sdk_client)));

    let rows = gateway.refresh_neg_risk_metadata().await.unwrap();

    first_page.assert();
    second_page.assert();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].event_family_id, "family-live-1");
    assert_eq!(
        rows[0].condition_id,
        "0x1111111111111111111111111111111111111111111111111111111111111111"
    );
    assert_eq!(rows[0].token_id, "123456789");
}
