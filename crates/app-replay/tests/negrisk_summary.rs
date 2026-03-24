use std::sync::atomic::{AtomicU64, Ordering};

use app_replay::{load_member_vector_from_journal, load_neg_risk_foundation_summary};
use chrono::{DateTime, Utc};
use persistence::{
    models::{
        FamilyHaltRow, JournalEntryInput, NegRiskDiscoverySnapshotInput, NegRiskFamilyMemberRow,
        NegRiskFamilyValidationRow,
    },
    persist_discovery_snapshot, run_migrations, JournalRepo, NegRiskFamilyRepo,
};
use serde_json::json;
use sqlx::{postgres::PgPoolOptions, PgPool};

static NEXT_SCHEMA_ID: AtomicU64 = AtomicU64::new(1);

#[tokio::test]
async fn negrisk_summary_reports_family_validation_halt_and_recent_event_counts() {
    with_test_database(|db| async move {
        run_migrations(&db.pool).await.unwrap();
        seed_foundation_rows(&db.pool).await;

        let summary = load_neg_risk_foundation_summary(&db.pool).await.unwrap();

        assert_eq!(summary.discovered_family_count, 3);
        assert_eq!(summary.validated_family_count, 2);
        assert_eq!(summary.excluded_family_count, 1);
        assert_eq!(summary.halted_family_count, 1);
        assert_eq!(summary.recent_validation_event_count, 2);
        assert_eq!(summary.recent_halt_event_count, 1);
        assert_eq!(summary.latest_discovery_revision, 7);

        let family_2 = summary
            .families
            .iter()
            .find(|family| family.event_family_id == "family-2")
            .unwrap();
        assert_eq!(
            family_2.exclusion_reason.as_deref(),
            Some("augmented_variant")
        );
        assert_eq!(
            family_2.halt_metadata_snapshot_hash.as_deref(),
            Some("sha256:snapshot-7")
        );

        let member_path = family_2.validation_member_vector_path.as_ref().unwrap();
        let members = load_member_vector_from_journal(&db.pool, member_path)
            .await
            .unwrap();
        assert_eq!(members, sample_member_vector("family-2"));
    })
    .await;
}

#[tokio::test]
async fn negrisk_summary_uses_latest_discovery_family_set_without_counting_stale_state_rows() {
    with_test_database(|db| async move {
        run_migrations(&db.pool).await.unwrap();
        seed_foundation_rows(&db.pool).await;

        persist_discovery_snapshot(
            &db.pool,
            NegRiskDiscoverySnapshotInput {
                discovery_revision: 8,
                metadata_snapshot_hash: "sha256:discovery-8".to_owned(),
                family_ids: vec!["family-1".to_owned()],
                captured_at: ts("2026-03-24T00:00:08Z"),
                source_kind: "test".to_owned(),
                source_session_id: "session-8".to_owned(),
                source_event_id: "discovery-8".to_owned(),
                dedupe_key: "discovery:8".to_owned(),
                extra_payload: json!({}),
            },
        )
        .await
        .unwrap();

        let summary = load_neg_risk_foundation_summary(&db.pool).await.unwrap();

        assert_eq!(summary.discovered_family_count, 1);
        assert_eq!(summary.latest_discovery_revision, 8);
        assert_eq!(summary.validated_family_count, 0);
        assert_eq!(summary.excluded_family_count, 0);
        assert_eq!(summary.halted_family_count, 0);
        assert_eq!(summary.families.len(), 1);
        assert_eq!(summary.families[0].event_family_id, "family-1");
        assert_eq!(summary.families[0].validation_status, None);
        assert_eq!(summary.families[0].validation_metadata_snapshot_hash, None);
    })
    .await;
}

#[tokio::test]
async fn negrisk_summary_keeps_authoritative_families_visible_even_when_current_state_is_stale() {
    with_test_database(|db| async move {
        run_migrations(&db.pool).await.unwrap();
        seed_foundation_rows(&db.pool).await;

        persist_discovery_snapshot(
            &db.pool,
            NegRiskDiscoverySnapshotInput {
                discovery_revision: 8,
                metadata_snapshot_hash: "sha256:discovery-8".to_owned(),
                family_ids: vec!["family-1".to_owned(), "family-2".to_owned()],
                captured_at: ts("2026-03-24T00:00:08Z"),
                source_kind: "test".to_owned(),
                source_session_id: "session-8".to_owned(),
                source_event_id: "discovery-8".to_owned(),
                dedupe_key: "discovery:8".to_owned(),
                extra_payload: json!({}),
            },
        )
        .await
        .unwrap();

        let summary = load_neg_risk_foundation_summary(&db.pool).await.unwrap();

        assert_eq!(summary.discovered_family_count, 2);
        assert_eq!(summary.latest_discovery_revision, 8);
        assert_eq!(summary.validated_family_count, 0);
        assert_eq!(summary.excluded_family_count, 0);
        assert_eq!(summary.halted_family_count, 0);
        assert_eq!(
            summary
                .families
                .iter()
                .map(|family| family.event_family_id.as_str())
                .collect::<Vec<_>>(),
            vec!["family-1", "family-2"]
        );
        assert!(summary
            .families
            .iter()
            .all(|family| family.validation_status.is_none()));
        assert!(summary.families.iter().all(|family| !family.halted));
    })
    .await;
}

#[tokio::test]
async fn negrisk_summary_member_vector_path_matches_current_row_state_not_latest_event_only() {
    with_test_database(|db| async move {
        run_migrations(&db.pool).await.unwrap();
        seed_foundation_rows(&db.pool).await;

        let stale_vector = sample_member_vector("stale-family-2");
        append_validation_journal_event(
            &db.pool,
            "family-2",
            "sha256:snapshot-9",
            9,
            stale_vector.clone(),
            "historical-validation-family-2",
        )
        .await;
        append_halt_journal_event(
            &db.pool,
            "family-2",
            "sha256:snapshot-9",
            9,
            stale_vector.clone(),
            "historical-halt-family-2",
        )
        .await;

        let summary = load_neg_risk_foundation_summary(&db.pool).await.unwrap();
        assert_eq!(summary.recent_validation_event_count, 2);
        assert_eq!(summary.recent_halt_event_count, 1);

        let family_2 = summary
            .families
            .iter()
            .find(|family| family.event_family_id == "family-2")
            .unwrap();

        let validation_path = family_2.validation_member_vector_path.as_ref().unwrap();
        let validation_members = load_member_vector_from_journal(&db.pool, validation_path)
            .await
            .unwrap();
        assert_eq!(validation_members, sample_member_vector("family-2"));

        let halt_path = family_2.halt_member_vector_path.as_ref().unwrap();
        let halt_members = load_member_vector_from_journal(&db.pool, halt_path)
            .await
            .unwrap();
        assert_eq!(halt_members, sample_member_vector("family-2"));
    })
    .await;
}

async fn seed_foundation_rows(pool: &PgPool) {
    persist_discovery_snapshot(
        pool,
        NegRiskDiscoverySnapshotInput {
            discovery_revision: 7,
            metadata_snapshot_hash: "sha256:discovery-7".to_owned(),
            family_ids: vec![
                "family-1".to_owned(),
                "family-2".to_owned(),
                "family-3".to_owned(),
            ],
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

    NegRiskFamilyRepo
        .upsert_validation(
            pool,
            &NegRiskFamilyValidationRow {
                event_family_id: "family-1".to_owned(),
                validation_status: "included".to_owned(),
                exclusion_reason: None,
                metadata_snapshot_hash: "sha256:snapshot-7".to_owned(),
                last_seen_discovery_revision: 7,
                member_count: 2,
                first_seen_at: ts("2026-03-24T00:00:01Z"),
                last_seen_at: ts("2026-03-24T00:00:05Z"),
                validated_at: ts("2026-03-24T00:00:06Z"),
                updated_at: ts("2026-03-24T00:00:06Z"),
                member_vector: sample_member_vector("family-1"),
                source_kind: "test".to_owned(),
                source_session_id: "validation-session-1".to_owned(),
                source_event_id: "validation-family-1".to_owned(),
                event_ts: ts("2026-03-24T00:00:06Z"),
            },
        )
        .await
        .unwrap();

    NegRiskFamilyRepo
        .upsert_validation(
            pool,
            &NegRiskFamilyValidationRow {
                event_family_id: "family-2".to_owned(),
                validation_status: "excluded".to_owned(),
                exclusion_reason: Some("augmented_variant".to_owned()),
                metadata_snapshot_hash: "sha256:snapshot-7".to_owned(),
                last_seen_discovery_revision: 7,
                member_count: 2,
                first_seen_at: ts("2026-03-24T00:00:02Z"),
                last_seen_at: ts("2026-03-24T00:00:05Z"),
                validated_at: ts("2026-03-24T00:00:06Z"),
                updated_at: ts("2026-03-24T00:00:06Z"),
                member_vector: sample_member_vector("family-2"),
                source_kind: "test".to_owned(),
                source_session_id: "validation-session-2".to_owned(),
                source_event_id: "validation-family-2".to_owned(),
                event_ts: ts("2026-03-24T00:00:06Z"),
            },
        )
        .await
        .unwrap();

    NegRiskFamilyRepo
        .upsert_halt(
            pool,
            &FamilyHaltRow {
                event_family_id: "family-2".to_owned(),
                halted: true,
                reason: Some("operator_review".to_owned()),
                blocks_new_risk: true,
                metadata_snapshot_hash: Some("sha256:snapshot-7".to_owned()),
                last_seen_discovery_revision: 7,
                set_at: ts("2026-03-24T00:00:06Z"),
                updated_at: ts("2026-03-24T00:00:06Z"),
                member_vector: sample_member_vector("family-2"),
                source_kind: "test".to_owned(),
                source_session_id: "halt-session-2".to_owned(),
                source_event_id: "halt-family-2".to_owned(),
                event_ts: ts("2026-03-24T00:00:06Z"),
            },
        )
        .await
        .unwrap();
}

async fn append_validation_journal_event(
    pool: &PgPool,
    family_id: &str,
    metadata_snapshot_hash: &str,
    discovery_revision: i64,
    member_vector: Vec<NegRiskFamilyMemberRow>,
    source_event_id: &str,
) {
    JournalRepo
        .append(
            pool,
            &JournalEntryInput {
                stream: format!("neg_risk_family:{family_id}"),
                source_kind: "test".to_owned(),
                source_session_id: "historical-validation".to_owned(),
                source_event_id: source_event_id.to_owned(),
                dedupe_key: format!("historical-validation:{family_id}:{metadata_snapshot_hash}"),
                causal_parent_id: None,
                event_type: "family_validation".to_owned(),
                event_ts: ts("2026-03-24T00:00:09Z"),
                payload: json!({
                    "event_family_id": family_id,
                    "validation_status": "excluded",
                    "exclusion_reason": "augmented_variant",
                    "metadata_snapshot_hash": metadata_snapshot_hash,
                    "discovery_revision": discovery_revision,
                    "member_count": member_vector.len(),
                    "first_seen_at": "2026-03-24T00:00:01Z",
                    "last_seen_at": "2026-03-24T00:00:05Z",
                    "validated_at": "2026-03-24T00:00:09Z",
                    "member_vector": member_vector_to_json(&member_vector),
                }),
            },
        )
        .await
        .unwrap();
}

async fn append_halt_journal_event(
    pool: &PgPool,
    family_id: &str,
    metadata_snapshot_hash: &str,
    discovery_revision: i64,
    member_vector: Vec<NegRiskFamilyMemberRow>,
    source_event_id: &str,
) {
    JournalRepo
        .append(
            pool,
            &JournalEntryInput {
                stream: format!("neg_risk_family:{family_id}"),
                source_kind: "test".to_owned(),
                source_session_id: "historical-halt".to_owned(),
                source_event_id: source_event_id.to_owned(),
                dedupe_key: format!("historical-halt:{family_id}:{metadata_snapshot_hash}"),
                causal_parent_id: None,
                event_type: "family_halt".to_owned(),
                event_ts: ts("2026-03-24T00:00:09Z"),
                payload: json!({
                    "event_family_id": family_id,
                    "halted": true,
                    "reason": "operator_review",
                    "blocks_new_risk": true,
                    "metadata_snapshot_hash": metadata_snapshot_hash,
                    "discovery_revision": discovery_revision,
                    "set_at": "2026-03-24T00:00:09Z",
                    "member_vector": member_vector_to_json(&member_vector),
                }),
            },
        )
        .await
        .unwrap();
}

fn sample_member_vector(family_id: &str) -> Vec<NegRiskFamilyMemberRow> {
    vec![
        NegRiskFamilyMemberRow {
            condition_id: format!("condition-{family_id}-1"),
            token_id: format!("token-{family_id}-1"),
            outcome_label: "Alice".to_owned(),
            is_placeholder: false,
            is_other: false,
            neg_risk_variant: "standard".to_owned(),
        },
        NegRiskFamilyMemberRow {
            condition_id: format!("condition-{family_id}-2"),
            token_id: format!("token-{family_id}-2"),
            outcome_label: "Other".to_owned(),
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
            "app_replay_negrisk_summary_{}_{}",
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
        .expect("DATABASE_URL must be set for app-replay neg-risk summary tests");
    let db = TestDatabase::new(&database_url).await;
    test(db.clone()).await;
    db.cleanup().await;
}

fn ts(value: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value)
        .expect("timestamp should parse")
        .with_timezone(&Utc)
}
