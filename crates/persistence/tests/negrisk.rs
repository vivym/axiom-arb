use std::sync::atomic::{AtomicU64, Ordering};

use chrono::{Duration, Utc};
use observability::{bootstrap_observability, bootstrap_tracing};
use persistence::{
    models::{
        FamilyHaltRow, JournalEntryInput, NegRiskDiscoverySnapshotInput, NegRiskFamilyMemberRow,
        NegRiskFamilyValidationRow,
    },
    persist_discovery_snapshot, reconcile_current_family_view, run_migrations, JournalRepo,
    NegRiskFamilyRepo, NegRiskPersistenceInstrumentation,
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

async fn persistence_reconcile_current_family_view_emits_authoritative_current_view_metrics_case() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();

    let observability = bootstrap_observability("persistence-test");
    let repo = NegRiskFamilyRepo::with_instrumentation(NegRiskPersistenceInstrumentation::enabled(
        observability.recorder(),
    ));

    persist_discovery_snapshot(
        &db.pool,
        sample_discovery_snapshot("rev-7", vec!["family-1", "family-2"]),
    )
    .await
    .unwrap();
    repo.reconcile_current_family_view(&db.pool, 7)
        .await
        .unwrap();

    let snapshot = observability.registry().snapshot();
    assert_eq!(
        snapshot.gauge(observability.metrics().neg_risk_family_included_count.key()),
        Some(0.0)
    );
    assert_eq!(
        snapshot.gauge(observability.metrics().neg_risk_family_excluded_count.key()),
        Some(0.0)
    );
    assert_eq!(
        snapshot.gauge(observability.metrics().neg_risk_family_halt_count.key()),
        Some(0.0)
    );
    assert_eq!(
        snapshot.gauge(
            observability
                .metrics()
                .neg_risk_family_discovered_count
                .key()
        ),
        None
    );

    db.cleanup().await;
}

async fn persistence_upserts_refresh_current_view_metrics_without_reconcile_case() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();

    let observability = bootstrap_observability("persistence-upsert-metrics-test");
    let repo = NegRiskFamilyRepo::with_instrumentation(NegRiskPersistenceInstrumentation::enabled(
        observability.recorder(),
    ));

    persist_discovery_snapshot(
        &db.pool,
        sample_discovery_snapshot("rev-7", vec!["family-1", "family-2"]),
    )
    .await
    .unwrap();

    let included = sample_validation("family-1");
    repo.upsert_validation(&db.pool, &included).await.unwrap();

    let mut excluded = sample_validation("family-2");
    excluded.validation_status = "excluded".to_owned();
    excluded.exclusion_reason = Some("placeholder_outcome".to_owned());
    repo.upsert_validation(&db.pool, &excluded).await.unwrap();
    repo.upsert_halt(&db.pool, &sample_halt("family-1", "sha256:snapshot-a"))
        .await
        .unwrap();

    let snapshot = observability.registry().snapshot();
    assert_eq!(
        snapshot.gauge(observability.metrics().neg_risk_family_included_count.key()),
        Some(1.0)
    );
    assert_eq!(
        snapshot.gauge(observability.metrics().neg_risk_family_excluded_count.key()),
        Some(1.0)
    );
    assert_eq!(
        snapshot.gauge(observability.metrics().neg_risk_family_halt_count.key()),
        Some(1.0)
    );
    assert_eq!(
        snapshot.gauge(
            observability
                .metrics()
                .neg_risk_family_discovered_count
                .key()
        ),
        None
    );

    db.cleanup().await;
}

async fn persistence_repo_instrumentation_is_instance_scoped_case() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();

    let observability_a = bootstrap_observability("persistence-instance-a");
    let repo_a = NegRiskFamilyRepo::with_instrumentation(
        NegRiskPersistenceInstrumentation::enabled(observability_a.recorder()),
    );
    let observability_b = bootstrap_observability("persistence-instance-b");
    let repo_b = NegRiskFamilyRepo::with_instrumentation(
        NegRiskPersistenceInstrumentation::enabled(observability_b.recorder()),
    );

    persist_discovery_snapshot(
        &db.pool,
        sample_discovery_snapshot("rev-7", vec!["family-a", "family-b"]),
    )
    .await
    .unwrap();

    repo_a
        .upsert_validation(&db.pool, &sample_validation("family-a"))
        .await
        .unwrap();
    repo_b
        .upsert_validation(&db.pool, &sample_validation("family-b"))
        .await
        .unwrap();

    let snapshot_a = observability_a.registry().snapshot();
    assert_eq!(
        snapshot_a.gauge(
            observability_a
                .metrics()
                .neg_risk_family_included_count
                .key()
        ),
        Some(1.0)
    );
    assert_eq!(
        snapshot_a.gauge(
            observability_a
                .metrics()
                .neg_risk_family_excluded_count
                .key()
        ),
        Some(0.0)
    );
    assert_eq!(
        snapshot_a.gauge(observability_a.metrics().neg_risk_family_halt_count.key()),
        Some(0.0)
    );

    let snapshot_b = observability_b.registry().snapshot();
    assert_eq!(
        snapshot_b.gauge(
            observability_b
                .metrics()
                .neg_risk_family_included_count
                .key()
        ),
        Some(2.0)
    );
    assert_eq!(
        snapshot_b.gauge(
            observability_b
                .metrics()
                .neg_risk_family_excluded_count
                .key()
        ),
        Some(0.0)
    );
    assert_eq!(
        snapshot_b.gauge(observability_b.metrics().neg_risk_family_halt_count.key()),
        Some(0.0)
    );

    db.cleanup().await;
}

async fn persistence_upserts_without_authoritative_snapshot_do_not_publish_current_view_metrics_case(
) {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();

    let observability = bootstrap_observability("persistence-no-snapshot-test");
    let repo = NegRiskFamilyRepo::with_instrumentation(NegRiskPersistenceInstrumentation::enabled(
        observability.recorder(),
    ));

    repo.upsert_validation(&db.pool, &sample_validation("family-1"))
        .await
        .unwrap();
    repo.upsert_halt(&db.pool, &sample_halt("family-1", "sha256:snapshot-a"))
        .await
        .unwrap();

    let snapshot = observability.registry().snapshot();
    assert_eq!(
        snapshot.gauge(observability.metrics().neg_risk_family_included_count.key()),
        None
    );
    assert_eq!(
        snapshot.gauge(observability.metrics().neg_risk_family_excluded_count.key()),
        None
    );
    assert_eq!(
        snapshot.gauge(observability.metrics().neg_risk_family_halt_count.key()),
        None
    );
    assert_eq!(
        snapshot.gauge(
            observability
                .metrics()
                .neg_risk_family_discovered_count
                .key()
        ),
        None
    );

    db.cleanup().await;
}

#[tokio::test]
async fn stores_family_validation_revision_and_explainability_fields() {
    stores_family_validation_revision_and_explainability_fields_case().await;
}

#[tokio::test]
async fn persistence_reconcile_current_family_view_emits_authoritative_current_view_metrics() {
    persistence_reconcile_current_family_view_emits_authoritative_current_view_metrics_case().await;
}

#[tokio::test]
async fn persistence_upserts_refresh_current_view_metrics_without_reconcile() {
    persistence_upserts_refresh_current_view_metrics_without_reconcile_case().await;
}

#[tokio::test]
async fn persistence_repo_instrumentation_is_instance_scoped() {
    persistence_repo_instrumentation_is_instance_scoped_case().await;
}

#[tokio::test]
async fn persistence_upserts_without_authoritative_snapshot_do_not_publish_current_view_metrics() {
    persistence_upserts_without_authoritative_snapshot_do_not_publish_current_view_metrics_case()
        .await;
}

mod negrisk {
    use super::*;
    use std::process::{Command, Output};

    fn helper_mode() -> Option<String> {
        std::env::var("PERSISTENCE_NEGRISK_HELPER_MODE").ok()
    }

    fn spawn_helper(test_name: &str, helper_mode: &str) -> Output {
        Command::new(std::env::current_exe().expect("current test binary"))
            .arg("--exact")
            .arg(test_name)
            .arg("--nocapture")
            .env("PERSISTENCE_NEGRISK_HELPER_MODE", helper_mode)
            .output()
            .expect("spawn negrisk helper")
    }

    const DISABLED_REFRESH_TEST_NAME: &str =
        "negrisk::disabled_instrumentation_skips_refresh_warning_when_latest_snapshot_is_invalid";

    #[tokio::test]
    async fn stores_family_validation_revision_and_explainability_fields() {
        stores_family_validation_revision_and_explainability_fields_case().await;
    }

    #[tokio::test]
    async fn reconcile_current_family_view_emits_authoritative_current_view_metrics() {
        persistence_reconcile_current_family_view_emits_authoritative_current_view_metrics_case()
            .await;
    }

    #[tokio::test]
    async fn upserts_refresh_current_view_metrics_without_reconcile() {
        persistence_upserts_refresh_current_view_metrics_without_reconcile_case().await;
    }

    #[tokio::test]
    async fn repo_instrumentation_is_instance_scoped() {
        persistence_repo_instrumentation_is_instance_scoped_case().await;
    }

    #[tokio::test]
    async fn upserts_without_authoritative_snapshot_do_not_publish_current_view_metrics() {
        persistence_upserts_without_authoritative_snapshot_do_not_publish_current_view_metrics_case()
            .await;
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
        let observability = bootstrap_observability("persistence-reconcile-test");
        let repo = NegRiskFamilyRepo::with_instrumentation(
            NegRiskPersistenceInstrumentation::enabled(observability.recorder()),
        );

        repo.upsert_validation(&db.pool, &sample_validation("family-1"))
            .await
            .unwrap();
        repo.upsert_validation(&db.pool, &sample_validation("family-2"))
            .await
            .unwrap();
        repo.upsert_halt(&db.pool, &sample_halt("family-1", "sha256:snapshot-a"))
            .await
            .unwrap();
        repo.upsert_halt(&db.pool, &sample_halt("family-2", "sha256:snapshot-a"))
            .await
            .unwrap();

        persist_discovery_snapshot(
            &db.pool,
            sample_discovery_snapshot("rev-7", vec!["family-1", "family-2"]),
        )
        .await
        .unwrap();
        repo.reconcile_current_family_view(&db.pool, 7)
            .await
            .unwrap();

        persist_discovery_snapshot(
            &db.pool,
            sample_discovery_snapshot("rev-8", vec!["family-1"]),
        )
        .await
        .unwrap();
        repo.reconcile_current_family_view(&db.pool, 8)
            .await
            .unwrap();

        let rows = NegRiskFamilyRepo.list_validations(&db.pool).await.unwrap();
        assert!(rows.iter().any(|row| {
            row.event_family_id == "family-1" && row.last_seen_discovery_revision == 7
        }));
        assert!(!rows.iter().any(|row| row.event_family_id == "family-2"));
        let halts = NegRiskFamilyRepo.list_halts(&db.pool).await.unwrap();
        assert!(halts.iter().any(|row| {
            row.event_family_id == "family-1" && row.last_seen_discovery_revision == 7
        }));
        assert!(!halts.iter().any(|row| row.event_family_id == "family-2"));
        let snapshot = observability.registry().snapshot();
        assert_eq!(
            snapshot.gauge(observability.metrics().neg_risk_family_included_count.key()),
            Some(1.0)
        );
        assert_eq!(
            snapshot.gauge(observability.metrics().neg_risk_family_excluded_count.key()),
            Some(0.0)
        );
        assert_eq!(
            snapshot.gauge(observability.metrics().neg_risk_family_halt_count.key()),
            Some(1.0)
        );

        db.cleanup().await;
    }

    #[tokio::test]
    async fn upsert_metrics_use_latest_discovery_snapshot_membership_before_reconcile_deletes_stale_rows(
    ) {
        let db = TestDatabase::new().await;
        run_migrations(&db.pool).await.unwrap();
        let observability = bootstrap_observability("persistence-authoritative-current-view-test");
        let repo = NegRiskFamilyRepo::with_instrumentation(
            NegRiskPersistenceInstrumentation::enabled(observability.recorder()),
        );

        persist_discovery_snapshot(
            &db.pool,
            sample_discovery_snapshot("rev-7", vec!["family-1", "family-2"]),
        )
        .await
        .unwrap();

        repo.upsert_validation(&db.pool, &sample_validation("family-1"))
            .await
            .unwrap();

        let mut excluded = sample_validation("family-2");
        excluded.validation_status = "excluded".to_owned();
        excluded.exclusion_reason = Some("placeholder_outcome".to_owned());
        repo.upsert_validation(&db.pool, &excluded).await.unwrap();
        repo.upsert_halt(&db.pool, &sample_halt("family-2", "sha256:snapshot-a"))
            .await
            .unwrap();

        persist_discovery_snapshot(
            &db.pool,
            sample_discovery_snapshot("rev-8", vec!["family-1"]),
        )
        .await
        .unwrap();

        let mut refreshed = sample_validation("family-1");
        refreshed.last_seen_discovery_revision = 8;
        refreshed.metadata_snapshot_hash = "sha256:snapshot-8".to_owned();
        refreshed.source_event_id = "validation-family-1-rev-8".to_owned();
        repo.upsert_validation(&db.pool, &refreshed).await.unwrap();

        let rows = NegRiskFamilyRepo.list_validations(&db.pool).await.unwrap();
        assert!(rows.iter().any(|row| row.event_family_id == "family-2"));
        let halts = NegRiskFamilyRepo.list_halts(&db.pool).await.unwrap();
        assert!(halts.iter().any(|row| row.event_family_id == "family-2"));

        let snapshot = observability.registry().snapshot();
        assert_eq!(
            snapshot.gauge(observability.metrics().neg_risk_family_included_count.key()),
            Some(1.0)
        );
        assert_eq!(
            snapshot.gauge(observability.metrics().neg_risk_family_excluded_count.key()),
            Some(0.0)
        );
        assert_eq!(
            snapshot.gauge(observability.metrics().neg_risk_family_halt_count.key()),
            Some(0.0)
        );

        db.cleanup().await;
    }

    #[tokio::test]
    async fn zero_family_refresh_replaces_the_previous_current_view() {
        let db = TestDatabase::new().await;
        run_migrations(&db.pool).await.unwrap();
        let observability = bootstrap_observability("persistence-zero-test");
        let repo = NegRiskFamilyRepo::with_instrumentation(
            NegRiskPersistenceInstrumentation::enabled(observability.recorder()),
        );

        repo.upsert_validation(&db.pool, &sample_validation("family-1"))
            .await
            .unwrap();
        repo.upsert_halt(&db.pool, &sample_halt("family-1", "sha256:snapshot-a"))
            .await
            .unwrap();

        persist_discovery_snapshot(
            &db.pool,
            sample_discovery_snapshot("rev-7", vec!["family-1"]),
        )
        .await
        .unwrap();
        repo.reconcile_current_family_view(&db.pool, 7)
            .await
            .unwrap();

        persist_discovery_snapshot(&db.pool, sample_discovery_snapshot("rev-8", vec![]))
            .await
            .unwrap();
        repo.reconcile_current_family_view(&db.pool, 8)
            .await
            .unwrap();

        assert!(NegRiskFamilyRepo
            .list_validations(&db.pool)
            .await
            .unwrap()
            .is_empty());
        assert!(NegRiskFamilyRepo
            .list_halts(&db.pool)
            .await
            .unwrap()
            .is_empty());
        let snapshot = observability.registry().snapshot();
        assert_eq!(
            snapshot.gauge(observability.metrics().neg_risk_family_included_count.key()),
            Some(0.0)
        );
        assert_eq!(
            snapshot.gauge(observability.metrics().neg_risk_family_excluded_count.key()),
            Some(0.0)
        );
        assert_eq!(
            snapshot.gauge(observability.metrics().neg_risk_family_halt_count.key()),
            Some(0.0)
        );

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
            row.event_family_id == "family-1" && row.last_seen_discovery_revision == 7
        }));
        assert!(!rows.iter().any(|row| row.event_family_id == "family-2"));
        let halts = NegRiskFamilyRepo.list_halts(&db.pool).await.unwrap();
        assert!(halts.is_empty());

        db.cleanup().await;
    }

    #[tokio::test]
    async fn post_commit_metric_refresh_failures_do_not_flip_validation_or_halt_results() {
        let db = TestDatabase::new().await;
        run_migrations(&db.pool).await.unwrap();
        let observability = bootstrap_observability("persistence-post-commit-failure-test");
        let repo = NegRiskFamilyRepo::with_instrumentation(
            NegRiskPersistenceInstrumentation::enabled(observability.recorder()),
        );

        JournalRepo
            .append(
                &db.pool,
                &JournalEntryInput {
                    stream: "neg_risk_discovery".to_owned(),
                    source_kind: "test".to_owned(),
                    source_session_id: "session-1".to_owned(),
                    source_event_id: "invalid-discovery".to_owned(),
                    dedupe_key: "invalid-discovery".to_owned(),
                    causal_parent_id: None,
                    event_type: "neg_risk_discovery_snapshot".to_owned(),
                    event_ts: Utc::now(),
                    payload: json!({
                        "discovery_revision": 9,
                        "metadata_snapshot_hash": "sha256:discovery-9",
                        "discovered_family_count": 1,
                        "family_ids": [9],
                        "captured_at": Utc::now().to_rfc3339(),
                    }),
                },
            )
            .await
            .unwrap();

        repo.upsert_validation(&db.pool, &sample_validation("family-1"))
            .await
            .unwrap();
        repo.upsert_halt(&db.pool, &sample_halt("family-1", "sha256:snapshot-a"))
            .await
            .unwrap();

        let rows = NegRiskFamilyRepo.list_validations(&db.pool).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].event_family_id, "family-1");

        let halts = NegRiskFamilyRepo.list_halts(&db.pool).await.unwrap();
        assert_eq!(halts.len(), 1);
        assert_eq!(halts[0].event_family_id, "family-1");

        let journal_rows = JournalRepo.list_after(&db.pool, 0, 100).await.unwrap();
        assert!(journal_rows
            .iter()
            .any(|row| row.event_type == "family_validation"));
        assert!(journal_rows
            .iter()
            .any(|row| row.event_type == "family_halt"));

        db.cleanup().await;
    }

    #[tokio::test]
    async fn disabled_instrumentation_skips_refresh_warning_when_latest_snapshot_is_invalid() {
        if helper_mode().as_deref() == Some("child") {
            bootstrap_tracing("persistence-disabled-refresh-test");

            let db = TestDatabase::new().await;
            run_migrations(&db.pool).await.unwrap();

            JournalRepo
                .append(
                    &db.pool,
                    &JournalEntryInput {
                        stream: "neg_risk_discovery".to_owned(),
                        source_kind: "test".to_owned(),
                        source_session_id: "session-1".to_owned(),
                        source_event_id: "invalid-discovery".to_owned(),
                        dedupe_key: "invalid-discovery".to_owned(),
                        causal_parent_id: None,
                        event_type: "neg_risk_discovery_snapshot".to_owned(),
                        event_ts: Utc::now(),
                        payload: json!({
                            "discovery_revision": 9,
                            "metadata_snapshot_hash": "sha256:discovery-9",
                            "discovered_family_count": 1,
                            "family_ids": [9],
                            "captured_at": Utc::now().to_rfc3339(),
                        }),
                    },
                )
                .await
                .unwrap();

            NegRiskFamilyRepo::with_instrumentation(NegRiskPersistenceInstrumentation::disabled())
                .upsert_validation(&db.pool, &sample_validation("family-1"))
                .await
                .unwrap();

            db.cleanup().await;
            return;
        }

        let output = spawn_helper(DISABLED_REFRESH_TEST_NAME, "child");

        assert!(
            output.status.success(),
            "helper failed: stdout={}\nstderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );

        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            !combined.contains("neg-risk current-view metric refresh failed after durable commit"),
            "disabled instrumentation still emitted a refresh warning: {combined}"
        );
    }
}
