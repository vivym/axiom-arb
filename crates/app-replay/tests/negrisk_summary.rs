use std::sync::atomic::{AtomicU64, Ordering};

use app_replay::{load_member_vector_from_journal, load_neg_risk_foundation_summary};
use chrono::{DateTime, Utc};
use persistence::{
    models::{
        FamilyHaltRow, NegRiskDiscoverySnapshotInput, NegRiskFamilyMemberRow,
        NegRiskFamilyValidationRow,
    },
    persist_discovery_snapshot, run_migrations, NegRiskFamilyRepo,
};
use serde_json::json;
use sqlx::{postgres::PgPoolOptions, PgPool};

static NEXT_SCHEMA_ID: AtomicU64 = AtomicU64::new(1);

#[tokio::test]
async fn foundation_summary_reports_family_validation_and_halts() {
    if database_url().is_none() {
        return;
    }

    with_test_database(|db| async move {
        run_migrations(&db.pool).await.unwrap();
        seed_foundation_rows(&db.pool).await;

        let summary = load_neg_risk_foundation_summary(&db.pool).await.unwrap();

        assert_eq!(summary.discovered_family_count, 3);
        assert_eq!(summary.validated_family_count, 2);
        assert_eq!(summary.excluded_family_count, 1);
        assert_eq!(summary.halted_family_count, 1);
        assert_eq!(summary.recent_validation_event_count, 2);
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
async fn foundation_summary_uses_latest_discovery_snapshot_as_authoritative_source() {
    if database_url().is_none() {
        return;
    }

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
        assert_eq!(summary.validated_family_count, 2);
        assert_eq!(summary.excluded_family_count, 1);
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
