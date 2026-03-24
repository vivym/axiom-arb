use std::sync::atomic::{AtomicU64, Ordering};

use chrono::{Duration, Utc};
use persistence::{
    models::{
        FamilyHaltRow, NegRiskDiscoverySnapshotInput, NegRiskFamilyMemberRow,
        NegRiskFamilyValidationRow,
    },
    persist_discovery_snapshot, reconcile_current_family_view, run_migrations, JournalRepo,
    NegRiskFamilyRepo,
};
use serde_json::json;
use sqlx::{postgres::PgPoolOptions, PgPool};

static NEXT_SCHEMA_ID: AtomicU64 = AtomicU64::new(1);

struct TestDatabase {
    admin_pool: PgPool,
    pool: PgPool,
    schema: String,
}

impl TestDatabase {
    async fn new() -> Self {
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set for persistence tests");

        let admin_pool = PgPoolOptions::new()
            .max_connections(2)
            .connect(&database_url)
            .await
            .expect("test database should connect");

        let schema = format!(
            "persistence_negrisk_test_{}_{}",
            std::process::id(),
            NEXT_SCHEMA_ID.fetch_add(1, Ordering::Relaxed)
        );
        let create_schema = format!(r#"CREATE SCHEMA "{schema}""#);

        sqlx::query(&create_schema)
            .execute(&admin_pool)
            .await
            .expect("test schema should create");

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
            .connect(&database_url)
            .await
            .expect("isolated test pool should connect");

        Self {
            admin_pool,
            pool,
            schema,
        }
    }

    async fn cleanup(self) {
        self.pool.close().await;

        let drop_schema = format!(
            r#"DROP SCHEMA IF EXISTS "{schema}" CASCADE"#,
            schema = self.schema
        );
        sqlx::query(&drop_schema)
            .execute(&self.admin_pool)
            .await
            .expect("test schema should drop");

        self.admin_pool.close().await;
    }
}

fn sample_validation(event_family_id: &str) -> NegRiskFamilyValidationRow {
    let now = Utc::now();

    NegRiskFamilyValidationRow {
        event_family_id: event_family_id.to_owned(),
        validation_status: "included".to_owned(),
        exclusion_reason: None,
        metadata_snapshot_hash: "sha256:snapshot-7".to_owned(),
        last_seen_discovery_revision: 7,
        member_count: 3,
        first_seen_at: now - Duration::minutes(3),
        last_seen_at: now - Duration::minutes(1),
        validated_at: now,
        updated_at: now,
        member_vector: vec![
            NegRiskFamilyMemberRow {
                condition_id: "condition-1".to_owned(),
                token_id: "token-1".to_owned(),
                outcome_label: "YES".to_owned(),
                is_placeholder: false,
                is_other: false,
                neg_risk_variant: "standard".to_owned(),
            },
            NegRiskFamilyMemberRow {
                condition_id: "condition-2".to_owned(),
                token_id: "token-2".to_owned(),
                outcome_label: "NO".to_owned(),
                is_placeholder: false,
                is_other: false,
                neg_risk_variant: "standard".to_owned(),
            },
            NegRiskFamilyMemberRow {
                condition_id: "condition-3".to_owned(),
                token_id: "token-3".to_owned(),
                outcome_label: "OTHER".to_owned(),
                is_placeholder: false,
                is_other: true,
                neg_risk_variant: "standard".to_owned(),
            },
        ],
        source_kind: "test".to_owned(),
        source_session_id: "session-1".to_owned(),
        source_event_id: format!("validation-{event_family_id}-rev-7"),
        event_ts: now,
    }
}

fn sample_halt(event_family_id: &str, snapshot_hash: &str) -> FamilyHaltRow {
    let now = Utc::now();

    FamilyHaltRow {
        event_family_id: event_family_id.to_owned(),
        halted: true,
        reason: Some("operator review".to_owned()),
        blocks_new_risk: true,
        metadata_snapshot_hash: Some(snapshot_hash.to_owned()),
        last_seen_discovery_revision: 7,
        set_at: now,
        updated_at: now,
        member_vector: sample_validation(event_family_id).member_vector,
        source_kind: "test".to_owned(),
        source_session_id: "session-1".to_owned(),
        source_event_id: format!("halt-{event_family_id}-{snapshot_hash}"),
        event_ts: now,
    }
}

fn sample_discovery_snapshot(
    source_event_id: &str,
    family_ids: Vec<&str>,
) -> NegRiskDiscoverySnapshotInput {
    let discovery_revision = source_event_id
        .rsplit('-')
        .next()
        .expect("revision suffix should exist")
        .parse::<i64>()
        .expect("revision suffix should be numeric");
    let family_ids: Vec<String> = family_ids.into_iter().map(str::to_owned).collect();

    NegRiskDiscoverySnapshotInput {
        discovery_revision,
        metadata_snapshot_hash: format!("sha256:discovery-{discovery_revision}"),
        family_ids,
        captured_at: Utc::now(),
        source_kind: "test".to_owned(),
        source_session_id: "session-1".to_owned(),
        source_event_id: source_event_id.to_owned(),
        dedupe_key: format!("discovery-{source_event_id}"),
        extra_payload: json!({}),
    }
}

fn sample_discovery_snapshot_with_extra_payload(
    source_event_id: &str,
    family_ids: Vec<&str>,
    extra_payload: serde_json::Value,
) -> NegRiskDiscoverySnapshotInput {
    let mut snapshot = sample_discovery_snapshot(source_event_id, family_ids);
    snapshot.extra_payload = extra_payload;
    snapshot
}

async fn stores_family_validation_revision_and_explainability_fields_case() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();

    NegRiskFamilyRepo
        .upsert_validation(&db.pool, &sample_validation("family-1"))
        .await
        .unwrap();

    let row = NegRiskFamilyRepo
        .list_validations(&db.pool)
        .await
        .unwrap()
        .pop()
        .unwrap();
    assert_eq!(row.member_count, 3);
    assert_eq!(row.last_seen_discovery_revision, 7);
    assert!(row.metadata_snapshot_hash.starts_with("sha256:"));

    db.cleanup().await;
}

#[tokio::test]
async fn stores_family_validation_revision_and_explainability_fields() {
    stores_family_validation_revision_and_explainability_fields_case().await;
}

mod negrisk {
    use super::*;

    #[tokio::test]
    async fn stores_family_validation_revision_and_explainability_fields() {
        stores_family_validation_revision_and_explainability_fields_case().await;
    }

    #[tokio::test]
    async fn validation_and_halt_updates_are_journaled_for_explainability() {
        let db = TestDatabase::new().await;
        run_migrations(&db.pool).await.unwrap();

        NegRiskFamilyRepo
            .upsert_validation(&db.pool, &sample_validation("family-1"))
            .await
            .unwrap();
        NegRiskFamilyRepo
            .upsert_halt(&db.pool, &sample_halt("family-1", "sha256:snapshot-a"))
            .await
            .unwrap();

        let rows = JournalRepo.list_after(&db.pool, 0, 100).await.unwrap();
        assert!(rows.iter().any(|row| row.event_type == "family_validation"));
        assert!(rows.iter().any(|row| row.event_type == "family_halt"));

        db.cleanup().await;
    }

    #[tokio::test]
    async fn validation_journal_payload_preserves_the_exact_member_vector() {
        let db = TestDatabase::new().await;
        run_migrations(&db.pool).await.unwrap();

        NegRiskFamilyRepo
            .upsert_validation(&db.pool, &sample_validation("family-1"))
            .await
            .unwrap();

        let row = JournalRepo
            .list_after(&db.pool, 0, 100)
            .await
            .unwrap()
            .into_iter()
            .find(|row| row.event_type == "family_validation")
            .unwrap();
        assert!(row.payload.to_string().contains("\"member_vector\""));
        assert!(row.payload.to_string().contains("condition-1"));
        assert!(row.payload.to_string().contains("token-1"));

        db.cleanup().await;
    }

    #[tokio::test]
    async fn halt_state_records_the_snapshot_hash_it_applies_to() {
        let db = TestDatabase::new().await;
        run_migrations(&db.pool).await.unwrap();

        NegRiskFamilyRepo
            .upsert_halt(&db.pool, &sample_halt("family-1", "sha256:snapshot-a"))
            .await
            .unwrap();

        let row = NegRiskFamilyRepo
            .list_halts(&db.pool)
            .await
            .unwrap()
            .pop()
            .unwrap();
        assert_eq!(
            row.metadata_snapshot_hash.as_deref(),
            Some("sha256:snapshot-a")
        );

        db.cleanup().await;
    }

    #[tokio::test]
    async fn repeated_halt_updates_append_multiple_halt_journal_events() {
        let db = TestDatabase::new().await;
        run_migrations(&db.pool).await.unwrap();

        NegRiskFamilyRepo
            .upsert_halt(&db.pool, &sample_halt("family-1", "sha256:snapshot-a"))
            .await
            .unwrap();
        NegRiskFamilyRepo
            .upsert_halt(&db.pool, &sample_halt("family-1", "sha256:snapshot-b"))
            .await
            .unwrap();

        let rows = JournalRepo.list_after(&db.pool, 0, 100).await.unwrap();
        let halt_rows = rows
            .into_iter()
            .filter(|row| row.event_type == "family_halt")
            .count();
        assert_eq!(halt_rows, 2);

        db.cleanup().await;
    }

    #[tokio::test]
    async fn successful_refresh_reconciles_current_view_and_drops_missing_families_from_current_counts(
    ) {
        let db = TestDatabase::new().await;
        run_migrations(&db.pool).await.unwrap();

        NegRiskFamilyRepo
            .upsert_validation(&db.pool, &sample_validation("family-1"))
            .await
            .unwrap();
        NegRiskFamilyRepo
            .upsert_validation(&db.pool, &sample_validation("family-2"))
            .await
            .unwrap();
        NegRiskFamilyRepo
            .upsert_halt(&db.pool, &sample_halt("family-2", "sha256:snapshot-a"))
            .await
            .unwrap();

        persist_discovery_snapshot(
            &db.pool,
            sample_discovery_snapshot("rev-7", vec!["family-1", "family-2"]),
        )
        .await
        .unwrap();
        reconcile_current_family_view(&db.pool, 7).await.unwrap();

        persist_discovery_snapshot(
            &db.pool,
            sample_discovery_snapshot("rev-8", vec!["family-1"]),
        )
        .await
        .unwrap();
        reconcile_current_family_view(&db.pool, 8).await.unwrap();

        let rows = NegRiskFamilyRepo.list_validations(&db.pool).await.unwrap();
        assert!(rows.iter().any(|row| {
            row.event_family_id == "family-1" && row.last_seen_discovery_revision == 8
        }));
        assert!(!rows.iter().any(|row| row.event_family_id == "family-2"));
        let halts = NegRiskFamilyRepo.list_halts(&db.pool).await.unwrap();
        assert!(!halts.iter().any(|row| row.event_family_id == "family-2"));

        db.cleanup().await;
    }

    #[tokio::test]
    async fn discovery_snapshot_preserves_mandatory_payload_fields() {
        let db = TestDatabase::new().await;
        run_migrations(&db.pool).await.unwrap();

        persist_discovery_snapshot(
            &db.pool,
            sample_discovery_snapshot_with_extra_payload(
                "rev-9",
                vec!["family-1"],
                json!({
                    "discovery_revision": 1,
                    "metadata_snapshot_hash": "sha256:wrong",
                    "discovered_family_count": 99,
                    "family_ids": ["family-x"],
                    "captured_at": "2000-01-01T00:00:00Z",
                    "extra_field": "kept"
                }),
            ),
        )
        .await
        .unwrap();

        let row = JournalRepo
            .list_after(&db.pool, 0, 100)
            .await
            .unwrap()
            .into_iter()
            .find(|row| row.event_type == "neg_risk_discovery_snapshot")
            .unwrap();

        assert_eq!(row.payload["discovery_revision"], json!(9));
        assert_eq!(
            row.payload["metadata_snapshot_hash"],
            json!("sha256:discovery-9")
        );
        assert_eq!(row.payload["discovered_family_count"], json!(1));
        assert_eq!(row.payload["family_ids"], json!(["family-1"]));
        assert_eq!(row.payload["extra_field"], json!("kept"));
        assert_ne!(row.payload["captured_at"], json!("2000-01-01T00:00:00Z"));

        db.cleanup().await;
    }

    #[tokio::test]
    async fn stale_requested_revision_cannot_override_newer_authoritative_snapshot() {
        let db = TestDatabase::new().await;
        run_migrations(&db.pool).await.unwrap();

        NegRiskFamilyRepo
            .upsert_validation(&db.pool, &sample_validation("family-1"))
            .await
            .unwrap();
        NegRiskFamilyRepo
            .upsert_validation(&db.pool, &sample_validation("family-2"))
            .await
            .unwrap();
        NegRiskFamilyRepo
            .upsert_halt(&db.pool, &sample_halt("family-2", "sha256:snapshot-a"))
            .await
            .unwrap();

        persist_discovery_snapshot(
            &db.pool,
            sample_discovery_snapshot("rev-7", vec!["family-1", "family-2"]),
        )
        .await
        .unwrap();
        persist_discovery_snapshot(
            &db.pool,
            sample_discovery_snapshot("rev-8", vec!["family-1"]),
        )
        .await
        .unwrap();

        reconcile_current_family_view(&db.pool, 7).await.unwrap();

        let rows = NegRiskFamilyRepo.list_validations(&db.pool).await.unwrap();
        assert!(rows.iter().any(|row| {
            row.event_family_id == "family-1" && row.last_seen_discovery_revision == 8
        }));
        assert!(!rows.iter().any(|row| row.event_family_id == "family-2"));
        let halts = NegRiskFamilyRepo.list_halts(&db.pool).await.unwrap();
        assert!(halts.is_empty());

        db.cleanup().await;
    }
}
