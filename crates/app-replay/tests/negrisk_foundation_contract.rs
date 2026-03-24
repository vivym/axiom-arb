use std::{
    collections::HashMap,
    collections::VecDeque,
    io::{Read, Write},
    net::TcpListener,
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::Duration,
};

use app_replay::{load_member_vector_from_journal, load_neg_risk_foundation_summary};
use chrono::{DateTime, Utc};
use domain::{FamilyExclusionReason, IdentifierRecord, MarketRoute, NegRiskVariant};
use persistence::{
    models::{
        FamilyHaltRow, JournalEntryInput, NegRiskDiscoverySnapshotInput, NegRiskFamilyMemberRow,
        NegRiskFamilyValidationRow,
    },
    persist_discovery_snapshot, run_migrations, JournalRepo, NegRiskFamilyRepo,
};
use serde_json::json;
use sqlx::{postgres::PgPoolOptions, PgPool};
use strategy_negrisk::{
    build_family_graph, validate_family, FamilyValidationStatus, NegRiskGraphFamily,
};
use venue_polymarket::PolymarketRestClient;

static NEXT_SCHEMA_ID: AtomicU64 = AtomicU64::new(1);

#[tokio::test]
async fn negrisk_foundation_contract() {
    with_test_database(|db| async move {
        run_migrations(&db.pool).await.unwrap();

        let server = spawn_local_listener(sample_contract_payloads());
        let client = test_client(server.base_url());

        let discovered_rows = client.fetch_neg_risk_metadata_rows().await.unwrap();
        let records = sample_identifier_records();
        let graph = build_family_graph(records.clone(), discovered_rows.clone()).unwrap();
        let family = graph
            .families()
            .iter()
            .find(|family| family.family.family_id.as_str() == "family-aug")
            .unwrap();

        let verdict = validate_family(family, 7, &discovered_rows[0].metadata_snapshot_hash);
        assert_eq!(verdict.status, FamilyValidationStatus::Excluded);
        assert_eq!(
            verdict.reason,
            Some(FamilyExclusionReason::AugmentedVariant)
        );

        persist_discovery_snapshot(
            &db.pool,
            NegRiskDiscoverySnapshotInput {
                discovery_revision: 7,
                metadata_snapshot_hash: discovered_rows[0].metadata_snapshot_hash.clone(),
                family_ids: graph
                    .families()
                    .iter()
                    .map(|family| family.family.family_id.as_str().to_owned())
                    .collect(),
                captured_at: ts("2026-03-24T00:00:07Z"),
                source_kind: "test".to_owned(),
                source_session_id: "session-7".to_owned(),
                source_event_id: "discovery-7".to_owned(),
                dedupe_key: "discovery:7".to_owned(),
                extra_payload: json!({}),
            },
        )
        .await
        .unwrap();

        let expected_member_vector = member_vector_for_family(family, &records);

        NegRiskFamilyRepo
            .upsert_validation(
                &db.pool,
                &NegRiskFamilyValidationRow {
                    event_family_id: verdict.family_id.clone(),
                    validation_status: "excluded".to_owned(),
                    exclusion_reason: Some(reason_label(
                        verdict
                            .reason
                            .expect("reason should exist for excluded verdict"),
                    )),
                    metadata_snapshot_hash: verdict.metadata_snapshot_hash.clone(),
                    last_seen_discovery_revision: verdict.discovery_revision,
                    member_count: verdict.member_count as i32,
                    first_seen_at: ts("2026-03-24T00:00:05Z"),
                    last_seen_at: ts("2026-03-24T00:00:06Z"),
                    validated_at: ts("2026-03-24T00:00:06Z"),
                    updated_at: ts("2026-03-24T00:00:06Z"),
                    member_vector: expected_member_vector.clone(),
                    source_kind: "test".to_owned(),
                    source_session_id: "validation-session".to_owned(),
                    source_event_id: "validation-family-aug".to_owned(),
                    event_ts: ts("2026-03-24T00:00:06Z"),
                },
            )
            .await
            .unwrap();

        NegRiskFamilyRepo
            .upsert_halt(
                &db.pool,
                &FamilyHaltRow {
                    event_family_id: "family-aug".to_owned(),
                    halted: true,
                    reason: Some("augmented_variant".to_owned()),
                    blocks_new_risk: true,
                    metadata_snapshot_hash: Some(verdict.metadata_snapshot_hash.clone()),
                    last_seen_discovery_revision: 7,
                    set_at: ts("2026-03-24T00:00:06Z"),
                    updated_at: ts("2026-03-24T00:00:06Z"),
                    member_vector: expected_member_vector.clone(),
                    source_kind: "test".to_owned(),
                    source_session_id: "halt-session".to_owned(),
                    source_event_id: "halt-family-aug".to_owned(),
                    event_ts: ts("2026-03-24T00:00:06Z"),
                },
            )
            .await
            .unwrap();

        let failed_refresh = client.try_fetch_neg_risk_metadata_rows().await;
        assert!(failed_refresh.is_err());

        let after_failed_refresh = client.fetch_neg_risk_metadata_rows().await.unwrap();
        assert_eq!(
            discovered_rows
                .iter()
                .map(|row| row.discovery_revision)
                .max()
                .unwrap(),
            after_failed_refresh
                .iter()
                .map(|row| row.discovery_revision)
                .max()
                .unwrap()
        );
        assert_eq!(
            discovered_rows
                .iter()
                .map(|row| row.metadata_snapshot_hash.clone())
                .max()
                .unwrap(),
            after_failed_refresh
                .iter()
                .map(|row| row.metadata_snapshot_hash.clone())
                .max()
                .unwrap()
        );

        append_failed_attempt_events(
            &db.pool,
            "family-aug",
            sample_member_vector("failed-family-aug"),
        )
        .await;

        let summary = load_neg_risk_foundation_summary(&db.pool).await.unwrap();
        assert_eq!(summary.discovered_family_count, 1);
        assert_eq!(summary.validated_family_count, 1);
        assert_eq!(summary.excluded_family_count, 1);
        assert_eq!(summary.halted_family_count, 1);
        assert_eq!(summary.recent_validation_event_count, 1);
        assert_eq!(summary.recent_halt_event_count, 1);
        assert_eq!(summary.latest_discovery_revision, 7);
        assert_eq!(summary.families.len(), 1);
        assert_eq!(summary.families[0].event_family_id, "family-aug");

        let family_summary = summary
            .families
            .iter()
            .find(|family| family.event_family_id == "family-aug")
            .unwrap();
        assert_eq!(
            family_summary.exclusion_reason.as_deref(),
            Some("augmented_variant")
        );
        assert_eq!(
            family_summary.validation_metadata_snapshot_hash.as_deref(),
            Some(verdict.metadata_snapshot_hash.as_str())
        );
        assert_eq!(
            family_summary.halt_metadata_snapshot_hash.as_deref(),
            Some(verdict.metadata_snapshot_hash.as_str())
        );

        let validation_path = family_summary
            .validation_member_vector_path
            .as_ref()
            .unwrap();
        let validation_members = load_member_vector_from_journal(&db.pool, validation_path)
            .await
            .unwrap();
        assert_eq!(validation_members, expected_member_vector);

        let halt_path = family_summary.halt_member_vector_path.as_ref().unwrap();
        let halt_members = load_member_vector_from_journal(&db.pool, halt_path)
            .await
            .unwrap();
        assert_eq!(halt_members, expected_member_vector);
    })
    .await;
}

async fn append_failed_attempt_events(
    pool: &PgPool,
    family_id: &str,
    member_vector: Vec<NegRiskFamilyMemberRow>,
) {
    JournalRepo
        .append(
            pool,
            &JournalEntryInput {
                stream: format!("neg_risk_family:{family_id}"),
                source_kind: "test".to_owned(),
                source_session_id: "failed-refresh".to_owned(),
                source_event_id: "failed-validation-family-aug".to_owned(),
                dedupe_key: format!("failed-validation:{family_id}:sha256:failed-attempt"),
                causal_parent_id: None,
                event_type: "family_validation".to_owned(),
                event_ts: ts("2026-03-24T00:00:08Z"),
                payload: json!({
                    "event_family_id": family_id,
                    "validation_status": "excluded",
                    "exclusion_reason": "augmented_variant",
                    "metadata_snapshot_hash": "sha256:failed-attempt",
                    "discovery_revision": 8,
                    "member_count": member_vector.len(),
                    "first_seen_at": "2026-03-24T00:00:01Z",
                    "last_seen_at": "2026-03-24T00:00:05Z",
                    "validated_at": "2026-03-24T00:00:08Z",
                    "member_vector": member_vector_to_json(&member_vector),
                }),
            },
        )
        .await
        .unwrap();

    JournalRepo
        .append(
            pool,
            &JournalEntryInput {
                stream: format!("neg_risk_family:{family_id}"),
                source_kind: "test".to_owned(),
                source_session_id: "failed-refresh".to_owned(),
                source_event_id: "failed-halt-family-aug".to_owned(),
                dedupe_key: format!("failed-halt:{family_id}:sha256:failed-attempt"),
                causal_parent_id: None,
                event_type: "family_halt".to_owned(),
                event_ts: ts("2026-03-24T00:00:08Z"),
                payload: json!({
                    "event_family_id": family_id,
                    "halted": true,
                    "reason": "failed_refresh_attempt",
                    "blocks_new_risk": true,
                    "metadata_snapshot_hash": "sha256:failed-attempt",
                    "discovery_revision": 8,
                    "set_at": "2026-03-24T00:00:08Z",
                    "member_vector": member_vector_to_json(&member_vector),
                }),
            },
        )
        .await
        .unwrap();
}

fn sample_identifier_records() -> Vec<IdentifierRecord> {
    vec![
        IdentifierRecord {
            event_id: "event-aug-1".into(),
            event_family_id: "family-aug".into(),
            market_id: "market-aug-1".into(),
            condition_id: "condition-aug-1".into(),
            token_id: "token-aug-1".into(),
            outcome_label: "Alice".to_owned(),
            route: MarketRoute::NegRisk,
        },
        IdentifierRecord {
            event_id: "event-aug-2".into(),
            event_family_id: "family-aug".into(),
            market_id: "market-aug-2".into(),
            condition_id: "condition-aug-2".into(),
            token_id: "token-aug-2".into(),
            outcome_label: "Bob".to_owned(),
            route: MarketRoute::NegRisk,
        },
    ]
}

fn member_vector_for_family(
    family: &NegRiskGraphFamily,
    records: &[IdentifierRecord],
) -> Vec<NegRiskFamilyMemberRow> {
    let condition_by_token: HashMap<_, _> = records
        .iter()
        .map(|record| (record.token_id.clone(), record.condition_id.clone()))
        .collect();

    family
        .family
        .members
        .iter()
        .map(|member| NegRiskFamilyMemberRow {
            condition_id: condition_by_token
                .get(&member.token_id)
                .unwrap()
                .as_str()
                .to_owned(),
            token_id: member.token_id.as_str().to_owned(),
            outcome_label: member.outcome_label.clone(),
            is_placeholder: member.is_placeholder,
            is_other: member.is_other,
            neg_risk_variant: variant_label(family.neg_risk_variant).to_owned(),
        })
        .collect()
}

fn sample_member_vector(family_id: &str) -> Vec<NegRiskFamilyMemberRow> {
    vec![
        NegRiskFamilyMemberRow {
            condition_id: format!("condition-{family_id}-1"),
            token_id: format!("token-{family_id}-1"),
            outcome_label: "Stale".to_owned(),
            is_placeholder: false,
            is_other: false,
            neg_risk_variant: "augmented".to_owned(),
        },
        NegRiskFamilyMemberRow {
            condition_id: format!("condition-{family_id}-2"),
            token_id: format!("token-{family_id}-2"),
            outcome_label: "Stale-Other".to_owned(),
            is_placeholder: false,
            is_other: true,
            neg_risk_variant: "augmented".to_owned(),
        },
    ]
}

fn member_vector_to_json(member_vector: &[NegRiskFamilyMemberRow]) -> serde_json::Value {
    serde_json::Value::Array(
        member_vector
            .iter()
            .map(|member| {
                json!({
                    "condition_id": member.condition_id,
                    "token_id": member.token_id,
                    "outcome_label": member.outcome_label,
                    "is_placeholder": member.is_placeholder,
                    "is_other": member.is_other,
                    "neg_risk_variant": member.neg_risk_variant,
                })
            })
            .collect(),
    )
}

fn reason_label(reason: FamilyExclusionReason) -> String {
    match reason {
        FamilyExclusionReason::PlaceholderOutcome => "placeholder_outcome",
        FamilyExclusionReason::OtherOutcome => "other_outcome",
        FamilyExclusionReason::AugmentedVariant => "augmented_variant",
        FamilyExclusionReason::MissingNamedOutcomes => "missing_named_outcomes",
        FamilyExclusionReason::NonNegRiskRoute => "non_negrisk_route",
    }
    .to_owned()
}

fn variant_label(variant: NegRiskVariant) -> &'static str {
    match variant {
        NegRiskVariant::Standard => "standard",
        NegRiskVariant::Augmented => "augmented",
        NegRiskVariant::Unknown => "unknown",
    }
}

fn test_client(base_url: String) -> PolymarketRestClient {
    let http = reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("test client");

    PolymarketRestClient::with_http_client(
        http,
        base_url.parse().expect("base url"),
        base_url.parse().expect("base url"),
        base_url.parse().expect("base url"),
    )
}

fn spawn_local_listener(scripted_responses: Vec<ScriptedResponse>) -> ScriptedServer {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
    listener
        .set_nonblocking(true)
        .expect("set listener nonblocking");
    let address = listener.local_addr().expect("server addr");
    let handle = thread::spawn(move || {
        let mut scripted_responses = VecDeque::from(scripted_responses);
        let mut idle_ticks = 0usize;

        while !scripted_responses.is_empty() {
            let (mut stream, _) = match listener.accept() {
                Ok(value) => {
                    idle_ticks = 0;
                    value
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    idle_ticks += 1;
                    if idle_ticks > 300 {
                        break;
                    }
                    thread::sleep(Duration::from_millis(10));
                    continue;
                }
                Err(error) => panic!("accept request: {error}"),
            };
            stream
                .set_nonblocking(false)
                .expect("set accepted stream blocking");
            let response = scripted_responses
                .pop_front()
                .expect("scripted response should exist");
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
        base_url: format!("http://{address}/"),
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
    base_url: String,
    handle: Option<thread::JoinHandle<()>>,
}

impl ScriptedServer {
    fn base_url(&self) -> String {
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

fn sample_contract_payloads() -> Vec<ScriptedResponse> {
    vec![
        first_success_page_one_ok(),
        first_success_page_two_empty(),
        failed_refresh_page_one_ok(),
        failed_refresh_page_two_unavailable(),
        failed_refresh_page_two_unavailable(),
        failed_refresh_page_one_ok(),
        failed_refresh_page_two_unavailable(),
        failed_refresh_page_two_unavailable(),
    ]
}

fn first_success_page_one_ok() -> ScriptedResponse {
    ScriptedResponse {
        expected_query_fragments: &["active=true", "closed=false", "limit=2", "offset=0"],
        status_line: "200 OK",
        body: r#"[{"id":"event-aug-1","parentEvent":"family-aug","negRisk":true,"enableNegRisk":true,"negRiskAugmented":true,"markets":[{"conditionId":"condition-aug-1","clobTokenIds":"token-aug-1","outcomes":"Alice","shortOutcomes":"Alice","negRisk":true,"negRiskOther":false}]},{"id":"event-aug-2","parentEvent":"family-aug","negRisk":true,"enableNegRisk":true,"negRiskAugmented":false,"markets":[{"conditionId":"condition-aug-2","clobTokenIds":"token-aug-2","outcomes":"Bob","shortOutcomes":"Bob","negRisk":true,"negRiskOther":false}]}]"#,
    }
}

fn first_success_page_two_empty() -> ScriptedResponse {
    ScriptedResponse {
        expected_query_fragments: &["active=true", "closed=false", "limit=2", "offset=2"],
        status_line: "200 OK",
        body: "[]",
    }
}

fn failed_refresh_page_one_ok() -> ScriptedResponse {
    ScriptedResponse {
        expected_query_fragments: &["active=true", "closed=false", "limit=2", "offset=0"],
        status_line: "200 OK",
        body: r#"[{"id":"event-aug-1","parentEvent":"family-aug","negRisk":true,"enableNegRisk":true,"negRiskAugmented":false,"markets":[{"conditionId":"condition-aug-1","clobTokenIds":"token-aug-1","outcomes":"Alice-Changed","shortOutcomes":"Alice-Changed","negRisk":true,"negRiskOther":false}]},{"id":"event-aug-2","parentEvent":"family-aug","negRisk":true,"enableNegRisk":true,"negRiskAugmented":false,"markets":[{"conditionId":"condition-aug-2","clobTokenIds":"token-aug-2","outcomes":"Bob-Changed","shortOutcomes":"Bob-Changed","negRisk":true,"negRiskOther":false}]}]"#,
    }
}

fn failed_refresh_page_two_unavailable() -> ScriptedResponse {
    ScriptedResponse {
        expected_query_fragments: &["active=true", "closed=false", "limit=2", "offset=2"],
        status_line: "503 Service Unavailable",
        body: r#"{"error":"temporary metadata outage"}"#,
    }
}

#[derive(Clone)]
struct TestDatabase {
    admin_pool: PgPool,
    pool: PgPool,
    schema: String,
}

impl TestDatabase {
    async fn new(database_url: &str) -> Self {
        let admin_pool = PgPoolOptions::new()
            .max_connections(2)
            .connect(database_url)
            .await
            .expect("test database should connect");

        let schema = format!(
            "app_replay_negrisk_contract_{}_{}",
            std::process::id(),
            NEXT_SCHEMA_ID.fetch_add(1, Ordering::Relaxed)
        );

        sqlx::query(&format!(r#"CREATE SCHEMA "{schema}""#))
            .execute(&admin_pool)
            .await
            .expect("schema should create");

        let search_path_sql = format!(r#"SET search_path TO "{schema}""#);
        let pool = PgPoolOptions::new()
            .max_connections(4)
            .after_connect(move |conn, _meta| {
                let search_path_sql = search_path_sql.clone();
                Box::pin(async move {
                    sqlx::query(&search_path_sql).execute(conn).await?;
                    Ok(())
                })
            })
            .connect(database_url)
            .await
            .expect("isolated pool should connect");

        Self {
            admin_pool,
            pool,
            schema,
        }
    }

    async fn cleanup(self) {
        self.pool.close().await;
        sqlx::query(&format!(
            r#"DROP SCHEMA IF EXISTS "{schema}" CASCADE"#,
            schema = self.schema
        ))
        .execute(&self.admin_pool)
        .await
        .expect("schema should drop");
        self.admin_pool.close().await;
    }
}

async fn with_test_database<F, Fut>(test: F)
where
    F: FnOnce(TestDatabase) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    let database_url = std::env::var("DATABASE_URL")
        .expect("DATABASE_URL must be set for app-replay neg-risk foundation contract tests");
    let db = TestDatabase::new(&database_url).await;
    test(db.clone()).await;
    db.cleanup().await;
}

fn ts(value: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value)
        .expect("timestamp should parse")
        .with_timezone(&Utc)
}
