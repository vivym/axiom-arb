use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
};

use url::Url;
use venue_polymarket::{NegRiskMetadataError, NegRiskVariant, PolymarketRestClient, RestError};

#[tokio::test]
async fn fetch_neg_risk_metadata_rows_discovers_all_pages_and_classifies_members() {
    let server = spawn_local_listener(sample_paginated_neg_risk_payloads());
    let client = test_client(server.base_url());

    let rows = client.fetch_neg_risk_metadata_rows().await.unwrap();

    assert_eq!(rows.len(), 4);
    assert!(rows.iter().any(|row| row.is_other));
}

#[tokio::test]
async fn successful_refresh_publishes_a_new_discovery_revision() {
    let server = spawn_local_listener(sample_refreshing_neg_risk_payloads());
    let client = test_client(server.base_url());

    let initial = client.fetch_neg_risk_metadata_rows().await.unwrap();
    let refreshed = client.fetch_neg_risk_metadata_rows().await.unwrap();

    let initial_row = initial
        .iter()
        .find(|row| row.event_family_id == "family-1" && row.token_id == "token-1")
        .unwrap();
    let refreshed_row = refreshed
        .iter()
        .find(|row| row.event_family_id == "family-1" && row.token_id == "token-1")
        .unwrap();

    assert!(initial_row.discovery_revision < refreshed_row.discovery_revision);
}

#[tokio::test]
async fn metadata_empty_refresh_publishes_an_authoritative_zero_family_snapshot() {
    let server = spawn_local_listener(sample_empty_refreshing_neg_risk_payloads());
    let client = test_client(server.base_url());

    let initial = client.fetch_neg_risk_metadata_rows().await.unwrap();
    let emptied = client.fetch_neg_risk_metadata_rows().await.unwrap();
    let repopulated = client.fetch_neg_risk_metadata_rows().await.unwrap();

    assert!(!initial.is_empty());
    assert!(emptied.is_empty());
    assert_eq!(
        repopulated[0].discovery_revision,
        initial[0].discovery_revision + 2
    );
}

#[tokio::test]
async fn failed_refresh_does_not_publish_a_new_revision_or_replace_current_view() {
    let server = spawn_local_listener(sample_partial_failure_neg_risk_payloads());
    let client = test_client(server.base_url());

    let initial = client.fetch_neg_risk_metadata_rows().await.unwrap();
    let failed = client.try_fetch_neg_risk_metadata_rows().await;
    let after_failure = client.fetch_neg_risk_metadata_rows().await.unwrap();

    assert!(failed.is_err());
    assert_eq!(
        initial.iter().map(|row| row.discovery_revision).max(),
        after_failure.iter().map(|row| row.discovery_revision).max()
    );
    assert_eq!(
        initial
            .iter()
            .map(|row| row.metadata_snapshot_hash.clone())
            .max(),
        after_failure
            .iter()
            .map(|row| row.metadata_snapshot_hash.clone())
            .max()
    );
}

#[tokio::test]
async fn augmented_family_is_classified_from_family_level_flags() {
    let server = spawn_local_listener(sample_augmented_neg_risk_payloads());
    let client = test_client(server.base_url());

    let rows = client.fetch_neg_risk_metadata_rows().await.unwrap();

    assert!(rows
        .iter()
        .any(|row| row.neg_risk_variant == NegRiskVariant::Augmented));
}

#[tokio::test]
async fn out_of_order_rows_are_canonicalized_before_publication_and_hashing() {
    let server = spawn_local_listener(sample_out_of_order_refreshing_neg_risk_payloads());
    let client = test_client(server.base_url());

    let initial = client.fetch_neg_risk_metadata_rows().await.unwrap();
    let refreshed = client.fetch_neg_risk_metadata_rows().await.unwrap();

    let initial_tokens: Vec<_> = initial.iter().map(|row| row.token_id.as_str()).collect();
    let refreshed_tokens: Vec<_> = refreshed.iter().map(|row| row.token_id.as_str()).collect();

    assert_eq!(
        initial_tokens,
        vec!["token-1", "token-2", "token-3", "token-4"]
    );
    assert_eq!(initial_tokens, refreshed_tokens);
    assert_eq!(
        initial
            .iter()
            .map(|row| row.metadata_snapshot_hash.as_str())
            .collect::<Vec<_>>(),
        refreshed
            .iter()
            .map(|row| row.metadata_snapshot_hash.as_str())
            .collect::<Vec<_>>()
    );
    assert!(initial[0].metadata_snapshot_hash.starts_with("sha256:"));
    assert_eq!(initial[0].metadata_snapshot_hash.len(), 71);
}

#[tokio::test]
async fn array_outcomes_use_named_title_semantics_and_yes_token_selection() {
    let server = spawn_local_listener(sample_array_payloads());
    let client = test_client(server.base_url());

    let rows = client.fetch_neg_risk_metadata_rows().await.unwrap();
    let row = rows
        .iter()
        .find(|row| row.event_family_id == "family-array" && row.condition_id == "condition-array")
        .unwrap();

    assert_eq!(row.outcome_label, "Alice");
    assert_eq!(row.token_id, "token-yes-array");
}

#[tokio::test]
async fn conflicting_duplicate_rows_within_one_revision_are_reported() {
    let server = spawn_local_listener(sample_conflicting_duplicate_payloads());
    let client = test_client(server.base_url());

    let err = client
        .try_fetch_neg_risk_metadata_rows()
        .await
        .expect_err("conflicting duplicate rows should fail discovery");

    match err {
        RestError::Metadata(NegRiskMetadataError::ConflictingDuplicateRow {
            event_family_id,
            condition_id,
            token_id,
            existing_outcome_label,
            incoming_outcome_label,
        }) => {
            assert_eq!(event_family_id, "family-dup");
            assert_eq!(condition_id, "condition-dup");
            assert_eq!(token_id, "token-yes-dup");
            assert_eq!(existing_outcome_label, "Alice");
            assert_eq!(incoming_outcome_label, "Bob");
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

fn test_client(base_url: Url) -> PolymarketRestClient {
    let client = reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("test client");

    PolymarketRestClient::with_http_client(
        client,
        base_url.clone(),
        base_url.clone(),
        base_url,
        None,
    )
}

fn spawn_local_listener(scripted_responses: Vec<ScriptedResponse>) -> ScriptedServer {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
    let address = listener.local_addr().expect("server addr");
    let scripted_responses_for_thread = scripted_responses;
    let handle = thread::spawn(move || {
        for response in scripted_responses_for_thread {
            let (mut stream, _) = listener.accept().expect("accept request");
            let request = read_request(&mut stream);

            assert!(
                request.starts_with("GET /events?"),
                "unexpected request line: {request}"
            );
            for fragment in response.expected_query_fragments {
                assert!(
                    request.contains(fragment),
                    "request missing fragment `{fragment}`: {request}"
                );
            }

            let wire_response = format!(
                "HTTP/1.1 {}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                response.status_line,
                response.body.len(),
                response.body
            );
            stream
                .write_all(wire_response.as_bytes())
                .expect("write response");
            stream.flush().expect("flush response");
        }
    });

    ScriptedServer {
        base_url: Url::parse(&format!("http://{address}/")).expect("base url"),
        handle: Some(handle),
    }
}

fn read_request(stream: &mut std::net::TcpStream) -> String {
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 1024];

    loop {
        let read = stream.read(&mut chunk).expect("read request");
        if read == 0 {
            break;
        }

        buffer.extend_from_slice(&chunk[..read]);
        if buffer.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
    }

    String::from_utf8_lossy(&buffer).into_owned()
}

#[derive(Debug)]
struct ScriptedServer {
    base_url: Url,
    handle: Option<thread::JoinHandle<()>>,
}

impl ScriptedServer {
    fn base_url(&self) -> Url {
        self.base_url.clone()
    }
}

impl Drop for ScriptedServer {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            handle.join().expect("join server thread");
        }
    }
}

#[derive(Debug)]
struct ScriptedResponse {
    expected_query_fragments: &'static [&'static str],
    status_line: &'static str,
    body: &'static str,
}

fn sample_paginated_neg_risk_payloads() -> Vec<ScriptedResponse> {
    vec![
        page_one_ok(),
        page_two_retry_needed(),
        page_two_retry_ok(),
        page_three_empty(),
    ]
}

fn sample_refreshing_neg_risk_payloads() -> Vec<ScriptedResponse> {
    vec![
        first_refresh_page_one_ok(),
        first_refresh_page_two_ok(),
        first_refresh_page_three_empty(),
        second_refresh_page_one_ok(),
        second_refresh_page_two_ok(),
        second_refresh_page_three_empty(),
    ]
}

fn sample_partial_failure_neg_risk_payloads() -> Vec<ScriptedResponse> {
    vec![
        first_refresh_page_one_ok(),
        first_refresh_page_two_ok(),
        first_refresh_page_three_empty(),
        retryable_failure_page_one_ok(),
        retryable_failure_page_two_unavailable(),
        retryable_failure_page_two_unavailable(),
        retryable_failure_page_one_ok(),
        retryable_failure_page_two_unavailable(),
        retryable_failure_page_two_unavailable(),
    ]
}

fn sample_empty_refreshing_neg_risk_payloads() -> Vec<ScriptedResponse> {
    vec![
        first_refresh_page_one_ok(),
        first_refresh_page_two_ok(),
        first_refresh_page_three_empty(),
        empty_page_one_ok(),
        second_refresh_page_one_ok(),
        second_refresh_page_two_ok(),
        second_refresh_page_three_empty(),
    ]
}

fn sample_augmented_neg_risk_payloads() -> Vec<ScriptedResponse> {
    vec![augmented_page_one_ok()]
}

fn sample_out_of_order_refreshing_neg_risk_payloads() -> Vec<ScriptedResponse> {
    vec![
        out_of_order_first_page_one_ok(),
        out_of_order_first_page_two_ok(),
        out_of_order_first_page_three_empty(),
        out_of_order_second_page_one_ok(),
        out_of_order_second_page_two_ok(),
        out_of_order_second_page_three_empty(),
    ]
}

fn sample_array_payloads() -> Vec<ScriptedResponse> {
    vec![array_payload_page_one_ok()]
}

fn sample_conflicting_duplicate_payloads() -> Vec<ScriptedResponse> {
    vec![conflicting_duplicate_page_one_ok()]
}

fn page_one_ok() -> ScriptedResponse {
    ScriptedResponse {
        expected_query_fragments: &["active=true", "closed=false", "limit=2", "offset=0"],
        status_line: "200 OK",
        body: r#"[{"id":"event-1","parentEvent":"family-1","negRisk":true,"enableNegRisk":true,"negRiskAugmented":false,"markets":[{"conditionId":"condition-1","clobTokenIds":"token-1","outcomes":"Alpha","shortOutcomes":"Alpha","negRisk":true,"negRiskOther":false}]},{"id":"event-2","parentEvent":"family-1","negRisk":true,"enableNegRisk":true,"negRiskAugmented":false,"markets":[{"conditionId":"condition-2","clobTokenIds":"token-2","outcomes":"Other","shortOutcomes":"Other","negRisk":true,"negRiskOther":true}]}]"#,
    }
}

fn page_two_retry_needed() -> ScriptedResponse {
    ScriptedResponse {
        expected_query_fragments: &["active=true", "closed=false", "limit=2", "offset=2"],
        status_line: "503 Service Unavailable",
        body: r#"{"error":"temporary upstream failure"}"#,
    }
}

fn page_two_retry_ok() -> ScriptedResponse {
    ScriptedResponse {
        expected_query_fragments: &["active=true", "closed=false", "limit=2", "offset=2"],
        status_line: "200 OK",
        body: r#"[{"id":"event-3","parentEvent":"family-2","negRisk":true,"enableNegRisk":true,"negRiskAugmented":false,"markets":[{"conditionId":"condition-3","clobTokenIds":"token-3","outcomes":"Beta","shortOutcomes":"Beta","negRisk":true,"negRiskOther":false}]},{"id":"event-4","parentEvent":"family-2","negRisk":true,"enableNegRisk":true,"negRiskAugmented":false,"markets":[{"conditionId":"condition-4","clobTokenIds":"token-4","outcomes":"Gamma","shortOutcomes":"Gamma","negRisk":true,"negRiskOther":false}]}]"#,
    }
}

fn page_three_empty() -> ScriptedResponse {
    ScriptedResponse {
        expected_query_fragments: &["active=true", "closed=false", "limit=2", "offset=4"],
        status_line: "200 OK",
        body: r#"[]"#,
    }
}

fn first_refresh_page_one_ok() -> ScriptedResponse {
    ScriptedResponse {
        expected_query_fragments: &["active=true", "closed=false", "limit=2", "offset=0"],
        status_line: "200 OK",
        body: r#"[{"id":"event-1","parentEvent":"family-1","negRisk":true,"enableNegRisk":true,"negRiskAugmented":false,"markets":[{"conditionId":"condition-1","clobTokenIds":"token-1","outcomes":"Alpha","shortOutcomes":"Alpha","negRisk":true,"negRiskOther":false}]},{"id":"event-2","parentEvent":"family-1","negRisk":true,"enableNegRisk":true,"negRiskAugmented":false,"markets":[{"conditionId":"condition-2","clobTokenIds":"token-2","outcomes":"Other","shortOutcomes":"Other","negRisk":true,"negRiskOther":true}]}]"#,
    }
}

fn first_refresh_page_two_ok() -> ScriptedResponse {
    ScriptedResponse {
        expected_query_fragments: &["active=true", "closed=false", "limit=2", "offset=2"],
        status_line: "200 OK",
        body: r#"[{"id":"event-3","parentEvent":"family-2","negRisk":true,"enableNegRisk":true,"negRiskAugmented":false,"markets":[{"conditionId":"condition-3","clobTokenIds":"token-3","outcomes":"Beta","shortOutcomes":"Beta","negRisk":true,"negRiskOther":false}]},{"id":"event-4","parentEvent":"family-2","negRisk":true,"enableNegRisk":true,"negRiskAugmented":false,"markets":[{"conditionId":"condition-4","clobTokenIds":"token-4","outcomes":"Gamma","shortOutcomes":"Gamma","negRisk":true,"negRiskOther":false}]}]"#,
    }
}

fn first_refresh_page_three_empty() -> ScriptedResponse {
    ScriptedResponse {
        expected_query_fragments: &["active=true", "closed=false", "limit=2", "offset=4"],
        status_line: "200 OK",
        body: r#"[]"#,
    }
}

fn empty_page_one_ok() -> ScriptedResponse {
    ScriptedResponse {
        expected_query_fragments: &["active=true", "closed=false", "limit=2", "offset=0"],
        status_line: "200 OK",
        body: r#"[]"#,
    }
}

fn second_refresh_page_one_ok() -> ScriptedResponse {
    ScriptedResponse {
        expected_query_fragments: &["active=true", "closed=false", "limit=2", "offset=0"],
        status_line: "200 OK",
        body: r#"[{"id":"event-1","parentEvent":"family-1","negRisk":true,"enableNegRisk":true,"negRiskAugmented":false,"markets":[{"conditionId":"condition-1","clobTokenIds":"token-1","outcomes":"Alpha","shortOutcomes":"Alpha","negRisk":true,"negRiskOther":false}]},{"id":"event-2","parentEvent":"family-1","negRisk":true,"enableNegRisk":true,"negRiskAugmented":false,"markets":[{"conditionId":"condition-2","clobTokenIds":"token-2","outcomes":"Other","shortOutcomes":"Other","negRisk":true,"negRiskOther":true}]}]"#,
    }
}

fn second_refresh_page_two_ok() -> ScriptedResponse {
    ScriptedResponse {
        expected_query_fragments: &["active=true", "closed=false", "limit=2", "offset=2"],
        status_line: "200 OK",
        body: r#"[{"id":"event-3","parentEvent":"family-2","negRisk":true,"enableNegRisk":true,"negRiskAugmented":false,"markets":[{"conditionId":"condition-3","clobTokenIds":"token-3","outcomes":"Beta","shortOutcomes":"Beta","negRisk":true,"negRiskOther":false}]},{"id":"event-4","parentEvent":"family-2","negRisk":true,"enableNegRisk":true,"negRiskAugmented":false,"markets":[{"conditionId":"condition-4","clobTokenIds":"token-4","outcomes":"Gamma","shortOutcomes":"Gamma","negRisk":true,"negRiskOther":false}]}]"#,
    }
}

fn second_refresh_page_three_empty() -> ScriptedResponse {
    ScriptedResponse {
        expected_query_fragments: &["active=true", "closed=false", "limit=2", "offset=4"],
        status_line: "200 OK",
        body: r#"[]"#,
    }
}

fn retryable_failure_page_one_ok() -> ScriptedResponse {
    ScriptedResponse {
        expected_query_fragments: &["active=true", "closed=false", "limit=2", "offset=0"],
        status_line: "200 OK",
        body: r#"[{"id":"event-1","parentEvent":"family-1","negRisk":true,"enableNegRisk":true,"negRiskAugmented":false,"markets":[{"conditionId":"condition-1","clobTokenIds":"token-1","outcomes":"Alpha","shortOutcomes":"Alpha","negRisk":true,"negRiskOther":false}]},{"id":"event-2","parentEvent":"family-1","negRisk":true,"enableNegRisk":true,"negRiskAugmented":false,"markets":[{"conditionId":"condition-2","clobTokenIds":"token-2","outcomes":"Other","shortOutcomes":"Other","negRisk":true,"negRiskOther":true}]}]"#,
    }
}

fn retryable_failure_page_two_unavailable() -> ScriptedResponse {
    ScriptedResponse {
        expected_query_fragments: &["active=true", "closed=false", "limit=2", "offset=2"],
        status_line: "503 Service Unavailable",
        body: r#"{"error":"temporary upstream failure"}"#,
    }
}

fn augmented_page_one_ok() -> ScriptedResponse {
    ScriptedResponse {
        expected_query_fragments: &["active=true", "closed=false", "limit=2", "offset=0"],
        status_line: "200 OK",
        body: r#"[{"id":"event-aug-1","parentEvent":"family-aug","negRisk":true,"enableNegRisk":true,"negRiskAugmented":true,"markets":[{"conditionId":"condition-aug-1","clobTokenIds":"token-aug-1","outcomes":"Augmented","shortOutcomes":"Augmented","negRisk":true,"negRiskOther":false}]}]"#,
    }
}

fn out_of_order_first_page_one_ok() -> ScriptedResponse {
    ScriptedResponse {
        expected_query_fragments: &["active=true", "closed=false", "limit=2", "offset=0"],
        status_line: "200 OK",
        body: r#"[{"id":"event-4","parentEvent":"family-2","negRisk":true,"enableNegRisk":true,"negRiskAugmented":false,"markets":[{"conditionId":"condition-4","clobTokenIds":"token-4","outcomes":"Gamma","shortOutcomes":"Gamma","negRisk":true,"negRiskOther":false}]},{"id":"event-2","parentEvent":"family-1","negRisk":true,"enableNegRisk":true,"negRiskAugmented":false,"markets":[{"conditionId":"condition-2","clobTokenIds":"token-2","outcomes":"Other","shortOutcomes":"Other","negRisk":true,"negRiskOther":true}]}]"#,
    }
}

fn out_of_order_first_page_two_ok() -> ScriptedResponse {
    ScriptedResponse {
        expected_query_fragments: &["active=true", "closed=false", "limit=2", "offset=2"],
        status_line: "200 OK",
        body: r#"[{"id":"event-3","parentEvent":"family-2","negRisk":true,"enableNegRisk":true,"negRiskAugmented":false,"markets":[{"conditionId":"condition-3","clobTokenIds":"token-3","outcomes":"Beta","shortOutcomes":"Beta","negRisk":true,"negRiskOther":false}]},{"id":"event-1","parentEvent":"family-1","negRisk":true,"enableNegRisk":true,"negRiskAugmented":false,"markets":[{"conditionId":"condition-1","clobTokenIds":"token-1","outcomes":"Alpha","shortOutcomes":"Alpha","negRisk":true,"negRiskOther":false}]}]"#,
    }
}

fn out_of_order_first_page_three_empty() -> ScriptedResponse {
    ScriptedResponse {
        expected_query_fragments: &["active=true", "closed=false", "limit=2", "offset=4"],
        status_line: "200 OK",
        body: r#"[]"#,
    }
}

fn out_of_order_second_page_one_ok() -> ScriptedResponse {
    ScriptedResponse {
        expected_query_fragments: &["active=true", "closed=false", "limit=2", "offset=0"],
        status_line: "200 OK",
        body: r#"[{"id":"event-3","parentEvent":"family-2","negRisk":true,"enableNegRisk":true,"negRiskAugmented":false,"markets":[{"conditionId":"condition-3","clobTokenIds":"token-3","outcomes":"Beta","shortOutcomes":"Beta","negRisk":true,"negRiskOther":false}]},{"id":"event-1","parentEvent":"family-1","negRisk":true,"enableNegRisk":true,"negRiskAugmented":false,"markets":[{"conditionId":"condition-1","clobTokenIds":"token-1","outcomes":"Alpha","shortOutcomes":"Alpha","negRisk":true,"negRiskOther":false}]}]"#,
    }
}

fn out_of_order_second_page_two_ok() -> ScriptedResponse {
    ScriptedResponse {
        expected_query_fragments: &["active=true", "closed=false", "limit=2", "offset=2"],
        status_line: "200 OK",
        body: r#"[{"id":"event-4","parentEvent":"family-2","negRisk":true,"enableNegRisk":true,"negRiskAugmented":false,"markets":[{"conditionId":"condition-4","clobTokenIds":"token-4","outcomes":"Gamma","shortOutcomes":"Gamma","negRisk":true,"negRiskOther":false}]},{"id":"event-2","parentEvent":"family-1","negRisk":true,"enableNegRisk":true,"negRiskAugmented":false,"markets":[{"conditionId":"condition-2","clobTokenIds":"token-2","outcomes":"Other","shortOutcomes":"Other","negRisk":true,"negRiskOther":true}]}]"#,
    }
}

fn out_of_order_second_page_three_empty() -> ScriptedResponse {
    ScriptedResponse {
        expected_query_fragments: &["active=true", "closed=false", "limit=2", "offset=4"],
        status_line: "200 OK",
        body: r#"[]"#,
    }
}

fn array_payload_page_one_ok() -> ScriptedResponse {
    ScriptedResponse {
        expected_query_fragments: &["active=true", "closed=false", "limit=2", "offset=0"],
        status_line: "200 OK",
        body: r#"[{"id":"event-array","title":"Who will win the election?","parentEvent":"family-array","negRisk":true,"enableNegRisk":true,"negRiskAugmented":false,"markets":[{"conditionId":"condition-array","groupItemTitle":"Alice","question":"Will Alice win the election?","clobTokenIds":["token-yes-array","token-no-array"],"outcomes":["Yes","No"],"shortOutcomes":["Yes","No"],"negRisk":true,"negRiskOther":false}]}]"#,
    }
}

fn conflicting_duplicate_page_one_ok() -> ScriptedResponse {
    ScriptedResponse {
        expected_query_fragments: &["active=true", "closed=false", "limit=2", "offset=0"],
        status_line: "200 OK",
        body: r#"[{"id":"event-dup-a","title":"Who will win?","parentEvent":"family-dup","negRisk":true,"enableNegRisk":true,"negRiskAugmented":false,"markets":[{"conditionId":"condition-dup","groupItemTitle":"Alice","question":"Will Alice win?","clobTokenIds":["token-yes-dup","token-no-dup"],"outcomes":["Yes","No"],"shortOutcomes":["Yes","No"],"negRisk":true,"negRiskOther":false}]},{"id":"event-dup-b","title":"Who will win?","parentEvent":"family-dup","negRisk":true,"enableNegRisk":true,"negRiskAugmented":false,"markets":[{"conditionId":"condition-dup","groupItemTitle":"Bob","question":"Will Bob win?","clobTokenIds":["token-yes-dup","token-no-dup"],"outcomes":["Yes","No"],"shortOutcomes":["Yes","No"],"negRisk":true,"negRiskOther":false}]}]"#,
    }
}
