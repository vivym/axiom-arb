mod support;

use support::{
    sample_client_for, scripted_metadata_api_valid_then_all_malformed,
    scripted_metadata_api_with_all_malformed, scripted_metadata_api_with_valid_and_malformed,
};
use url::Url;
use venue_polymarket::{NegRiskMetadataError, RestError};

#[tokio::test]
async fn legacy_metadata_shell_can_delegate_to_injected_metadata_api() {
    let client = sample_client_for(Url::parse("http://127.0.0.1:1/").unwrap())
        .with_metadata_api(scripted_metadata_api_with_valid_and_malformed());

    let rows = client.fetch_neg_risk_metadata_rows().await.unwrap();

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].event_id, "event-valid-1");
    assert_eq!(rows[0].condition_id, "condition-valid-1");
    assert_eq!(rows[0].token_id, "token-valid-1");
}

#[tokio::test]
async fn legacy_metadata_shell_surfaces_injected_metadata_api_fail_closed_errors() {
    let client = sample_client_for(Url::parse("http://127.0.0.1:1/").unwrap())
        .with_metadata_api(scripted_metadata_api_with_all_malformed());

    let err = client.try_fetch_neg_risk_metadata_rows().await.unwrap_err();

    match err {
        RestError::Metadata(NegRiskMetadataError::NoValidRowsAfterFiltering { skipped_rows }) => {
            assert_eq!(skipped_rows, 1);
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[tokio::test]
async fn legacy_metadata_shell_fetch_keeps_last_snapshot_when_injected_refresh_fails() {
    let client = sample_client_for(Url::parse("http://127.0.0.1:1/").unwrap())
        .with_metadata_api(scripted_metadata_api_valid_then_all_malformed());

    let first_rows = client.fetch_neg_risk_metadata_rows().await.unwrap();
    let fallback_rows = client.fetch_neg_risk_metadata_rows().await.unwrap();

    assert_eq!(first_rows, fallback_rows);
    assert_eq!(fallback_rows.len(), 1);
    assert_eq!(fallback_rows[0].event_id, "event-valid-1");
}
