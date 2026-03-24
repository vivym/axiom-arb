use std::{
    collections::HashMap,
    sync::atomic::{AtomicU64, Ordering},
};

use app_replay::{load_member_vector_from_journal, load_neg_risk_foundation_summary};
use chrono::{DateTime, Utc};
use domain::{FamilyExclusionReason, IdentifierRecord, MarketRoute, NegRiskVariant};
use persistence::{
    models::{
        FamilyHaltRow, NegRiskDiscoverySnapshotInput, NegRiskFamilyMemberRow,
        NegRiskFamilyValidationRow,
    },
    persist_discovery_snapshot, run_migrations, NegRiskFamilyRepo,
};
use serde_json::json;
use sqlx::{postgres::PgPoolOptions, PgPool};
use strategy_negrisk::{
    build_family_graph, validate_family, FamilyValidationStatus, NegRiskGraphFamily,
};
use venue_polymarket::NegRiskMarketMetadata;

static NEXT_SCHEMA_ID: AtomicU64 = AtomicU64::new(1);

#[tokio::test]
async fn negrisk_foundation_contract() {
    if database_url().is_none() {
        return;
    }

    with_test_database(|db| async move {
        run_migrations(&db.pool).await.unwrap();

        let records = sample_identifier_records();
        let metadata = fetch_sample_paginated_metadata();
        let graph = build_family_graph(records.clone(), metadata).unwrap();
        let family = graph
            .families()
            .iter()
            .find(|family| family.family.family_id.as_str() == "family-aug")
            .unwrap();

        let verdict = validate_family(family, 7, "sha256:snapshot-a");
        assert_eq!(verdict.status, FamilyValidationStatus::Excluded);
        assert_eq!(
            verdict.reason,
            Some(FamilyExclusionReason::AugmentedVariant)
        );

        persist_discovery_snapshot(
            &db.pool,
            NegRiskDiscoverySnapshotInput {
                discovery_revision: 7,
                metadata_snapshot_hash: "sha256:discovery-7".to_owned(),
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
                    metadata_snapshot_hash: Some("sha256:snapshot-a".to_owned()),
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

        let summary = load_neg_risk_foundation_summary(&db.pool).await.unwrap();
        assert_eq!(summary.discovered_family_count, 1);
        assert_eq!(summary.validated_family_count, 1);
        assert_eq!(summary.excluded_family_count, 1);
        assert_eq!(summary.halted_family_count, 1);
        assert_eq!(summary.latest_discovery_revision, 7);

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
            family_summary.validation_metadata_snapshot_hash,
            Some("sha256:snapshot-a".to_owned())
        );
        assert_eq!(
            family_summary.halt_metadata_snapshot_hash.as_deref(),
            Some("sha256:snapshot-a")
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

fn fetch_sample_paginated_metadata() -> Vec<NegRiskMarketMetadata> {
    sample_metadata_pages().into_iter().flatten().collect()
}

fn sample_metadata_pages() -> Vec<Vec<NegRiskMarketMetadata>> {
    vec![
        vec![metadata_row(
            "family-aug",
            "event-1",
            "condition-1",
            "token-1",
            "Alice",
            NegRiskVariant::Augmented,
        )],
        vec![metadata_row(
            "family-aug",
            "event-2",
            "condition-2",
            "token-2",
            "Bob",
            NegRiskVariant::Standard,
        )],
    ]
}

fn sample_identifier_records() -> Vec<IdentifierRecord> {
    vec![
        IdentifierRecord {
            event_id: "event-1".into(),
            event_family_id: "family-aug".into(),
            market_id: "market-1".into(),
            condition_id: "condition-1".into(),
            token_id: "token-1".into(),
            outcome_label: "Alice".to_owned(),
            route: MarketRoute::NegRisk,
        },
        IdentifierRecord {
            event_id: "event-2".into(),
            event_family_id: "family-aug".into(),
            market_id: "market-2".into(),
            condition_id: "condition-2".into(),
            token_id: "token-2".into(),
            outcome_label: "Bob".to_owned(),
            route: MarketRoute::NegRisk,
        },
    ]
}

fn metadata_row(
    event_family_id: &str,
    event_id: &str,
    condition_id: &str,
    token_id: &str,
    outcome_label: &str,
    neg_risk_variant: NegRiskVariant,
) -> NegRiskMarketMetadata {
    NegRiskMarketMetadata {
        event_family_id: event_family_id.to_owned(),
        event_id: event_id.to_owned(),
        condition_id: condition_id.to_owned(),
        token_id: token_id.to_owned(),
        outcome_label: outcome_label.to_owned(),
        route: MarketRoute::NegRisk,
        enable_neg_risk: Some(true),
        neg_risk_augmented: Some(matches!(neg_risk_variant, NegRiskVariant::Augmented)),
        neg_risk_variant,
        is_placeholder: false,
        is_other: false,
        discovery_revision: 7,
        metadata_snapshot_hash: "sha256:snapshot-a".to_owned(),
    }
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

fn database_url() -> Option<String> {
    std::env::var("DATABASE_URL").ok()
}

async fn with_test_database<F, Fut>(test: F)
where
    F: FnOnce(TestDatabase) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    let database_url = database_url().expect("DATABASE_URL must be set for app-replay tests");
    let db = TestDatabase::new(&database_url).await;
    test(db.clone()).await;
    db.cleanup().await;
}

fn ts(value: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value)
        .expect("timestamp should parse")
        .with_timezone(&Utc)
}
