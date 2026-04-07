use std::sync::atomic::{AtomicU64, Ordering};

use chrono::Utc;
use persistence::{
    models::{OperatorStrategyAdoptionHistoryRow, OperatorTargetAdoptionHistoryRow},
    run_migrations, OperatorStrategyAdoptionHistoryRepo, OperatorTargetAdoptionHistoryRepo,
};
use sqlx::{postgres::PgPoolOptions, PgPool};

static NEXT_SCHEMA_ID: AtomicU64 = AtomicU64::new(1);
static NEXT_ADOPTION_ID: AtomicU64 = AtomicU64::new(1);

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
            "persistence_operator_target_adoption_test_{}_{}",
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

async fn apply_operator_target_adoption_history_migration_sql(pool: &PgPool, sql: &str) {
    sqlx::raw_sql(sql).execute(pool).await.unwrap();
}

fn sample_adoption(
    operator_target_revision: &str,
    previous_operator_target_revision: Option<&str>,
) -> OperatorTargetAdoptionHistoryRow {
    sample_adoption_at(
        operator_target_revision,
        previous_operator_target_revision,
        &Utc::now().to_rfc3339(),
    )
}

fn sample_adoption_at(
    operator_target_revision: &str,
    previous_operator_target_revision: Option<&str>,
    adopted_at: &str,
) -> OperatorTargetAdoptionHistoryRow {
    OperatorTargetAdoptionHistoryRow {
        adoption_id: format!(
            "adoption-{operator_target_revision}-{}",
            NEXT_ADOPTION_ID.fetch_add(1, Ordering::Relaxed)
        ),
        action_kind: "adopt".to_owned(),
        operator_target_revision: operator_target_revision.to_owned(),
        previous_operator_target_revision: previous_operator_target_revision.map(str::to_owned),
        adoptable_revision: Some(format!("adoptable-{operator_target_revision}")),
        candidate_revision: Some(format!("candidate-{operator_target_revision}")),
        adopted_at: chrono::DateTime::parse_from_rfc3339(adopted_at)
            .unwrap()
            .with_timezone(&Utc),
    }
}

fn sample_adoption_with_id_at(
    adoption_id: &str,
    action_kind: &str,
    operator_target_revision: &str,
    previous_operator_target_revision: Option<&str>,
    adoptable_revision: Option<&str>,
    candidate_revision: Option<&str>,
    adopted_at: &str,
) -> OperatorTargetAdoptionHistoryRow {
    OperatorTargetAdoptionHistoryRow {
        adoption_id: adoption_id.to_owned(),
        action_kind: action_kind.to_owned(),
        operator_target_revision: operator_target_revision.to_owned(),
        previous_operator_target_revision: previous_operator_target_revision.map(str::to_owned),
        adoptable_revision: adoptable_revision.map(str::to_owned),
        candidate_revision: candidate_revision.map(str::to_owned),
        adopted_at: chrono::DateTime::parse_from_rfc3339(adopted_at)
            .unwrap()
            .with_timezone(&Utc),
    }
}

fn sample_strategy_adoption_with_id_at(
    adoption_id: &str,
    action_kind: &str,
    operator_strategy_revision: &str,
    previous_operator_strategy_revision: Option<&str>,
    adoptable_strategy_revision: Option<&str>,
    strategy_candidate_revision: Option<&str>,
    adopted_at: &str,
) -> OperatorStrategyAdoptionHistoryRow {
    OperatorStrategyAdoptionHistoryRow {
        adoption_id: adoption_id.to_owned(),
        action_kind: action_kind.to_owned(),
        operator_strategy_revision: operator_strategy_revision.to_owned(),
        previous_operator_strategy_revision: previous_operator_strategy_revision.map(str::to_owned),
        adoptable_strategy_revision: adoptable_strategy_revision.map(str::to_owned),
        strategy_candidate_revision: strategy_candidate_revision.map(str::to_owned),
        adopted_at: chrono::DateTime::parse_from_rfc3339(adopted_at)
            .unwrap()
            .with_timezone(&Utc),
    }
}

#[tokio::test]
async fn adoption_history_returns_previous_distinct_operator_target_revision() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();

    let repo = OperatorTargetAdoptionHistoryRepo;
    repo.append(&db.pool, &sample_adoption("targets-rev-1", None))
        .await
        .unwrap();
    repo.append(
        &db.pool,
        &sample_adoption("targets-rev-2", Some("targets-rev-1")),
    )
    .await
    .unwrap();
    repo.append(
        &db.pool,
        &sample_adoption("targets-rev-2", Some("targets-rev-2")),
    )
    .await
    .unwrap();

    let previous = repo
        .previous_distinct_revision(&db.pool, "targets-rev-2")
        .await
        .unwrap();
    assert_eq!(previous.as_deref(), Some("targets-rev-1"));

    db.cleanup().await;
}

#[tokio::test]
async fn adoption_history_upgrade_backfills_history_seq_in_adopted_at_then_adoption_id_order() {
    let db = TestDatabase::new().await;

    apply_operator_target_adoption_history_migration_sql(
        &db.pool,
        include_str!("../../../migrations/0012_operator_target_adoption_history.sql"),
    )
    .await;

    let repo = OperatorTargetAdoptionHistoryRepo;
    let earlier_adoption_id = sample_adoption_with_id_at(
        "z-first",
        "adopt",
        "targets-rev-40",
        Some("targets-rev-39"),
        Some("adoptable-40"),
        Some("candidate-40"),
        "2026-03-30T11:00:00Z",
    );
    let later_adoption_id = sample_adoption_with_id_at(
        "a-second",
        "adopt",
        "targets-rev-40",
        Some("targets-rev-38"),
        Some("adoptable-40-b"),
        Some("candidate-40-b"),
        "2026-03-30T11:00:00Z",
    );

    repo.append(&db.pool, &earlier_adoption_id).await.unwrap();
    repo.append(&db.pool, &later_adoption_id).await.unwrap();

    apply_operator_target_adoption_history_migration_sql(
        &db.pool,
        include_str!("../../../migrations/0013_operator_target_adoption_history_constraints.sql"),
    )
    .await;

    let latest = repo.latest(&db.pool).await.unwrap();
    assert_eq!(latest, Some(earlier_adoption_id.clone()));

    let previous = repo
        .previous_distinct_revision(&db.pool, "targets-rev-40")
        .await
        .unwrap();
    assert_eq!(previous.as_deref(), Some("targets-rev-39"));

    db.cleanup().await;
}

#[tokio::test]
async fn adoption_history_latest_returns_newest_row() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();

    let repo = OperatorTargetAdoptionHistoryRepo;
    let older = sample_adoption_at(
        "targets-rev-8",
        Some("targets-rev-7"),
        "2026-03-30T09:00:00Z",
    );
    let newer = sample_adoption_at(
        "targets-rev-9",
        Some("targets-rev-8"),
        "2026-03-30T09:05:00Z",
    );

    repo.append(&db.pool, &older).await.unwrap();
    repo.append(&db.pool, &newer).await.unwrap();

    let latest = repo.latest(&db.pool).await.unwrap();
    assert_eq!(latest, Some(newer));

    db.cleanup().await;
}

#[tokio::test]
async fn adoption_history_latest_uses_append_order_when_timestamps_match() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();

    let repo = OperatorTargetAdoptionHistoryRepo;
    let first = sample_adoption_with_id_at(
        "z-first",
        "adopt",
        "targets-rev-10",
        Some("targets-rev-9"),
        Some("adoptable-10"),
        Some("candidate-10"),
        "2026-03-30T10:00:00Z",
    );
    let second = sample_adoption_with_id_at(
        "a-second",
        "adopt",
        "targets-rev-11",
        Some("targets-rev-10"),
        Some("adoptable-11"),
        Some("candidate-11"),
        "2026-03-30T10:00:00Z",
    );

    repo.append(&db.pool, &first).await.unwrap();
    repo.append(&db.pool, &second).await.unwrap();

    let latest = repo.latest(&db.pool).await.unwrap();
    assert_eq!(latest, Some(second));

    db.cleanup().await;
}

#[tokio::test]
async fn adoption_history_previous_distinct_revision_uses_append_order_when_timestamps_match() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();

    let repo = OperatorTargetAdoptionHistoryRepo;
    let first = sample_adoption_with_id_at(
        "z-first",
        "adopt",
        "targets-rev-12",
        Some("targets-rev-10"),
        Some("adoptable-12-a"),
        Some("candidate-12-a"),
        "2026-03-30T10:10:00Z",
    );
    let second = sample_adoption_with_id_at(
        "a-second",
        "adopt",
        "targets-rev-12",
        Some("targets-rev-11"),
        Some("adoptable-12-b"),
        Some("candidate-12-b"),
        "2026-03-30T10:10:00Z",
    );

    repo.append(&db.pool, &first).await.unwrap();
    repo.append(&db.pool, &second).await.unwrap();

    let previous = repo
        .previous_distinct_revision(&db.pool, "targets-rev-12")
        .await
        .unwrap();
    assert_eq!(previous.as_deref(), Some("targets-rev-11"));

    db.cleanup().await;
}

#[tokio::test]
async fn strategy_adoption_history_latest_prefers_newer_neutral_rows_over_legacy_rows() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();

    let legacy_repo = OperatorTargetAdoptionHistoryRepo;
    legacy_repo
        .append(
            &db.pool,
            &sample_adoption_with_id_at(
                "legacy-adopt",
                "adopt",
                "targets-rev-30",
                Some("targets-rev-29"),
                Some("adoptable-30"),
                Some("candidate-30"),
                "2026-03-30T10:00:00Z",
            ),
        )
        .await
        .unwrap();

    let strategy_repo = OperatorStrategyAdoptionHistoryRepo;
    let newer = sample_strategy_adoption_with_id_at(
        "strategy-rollback",
        "rollback",
        "targets-rev-29",
        Some("targets-rev-30"),
        None,
        None,
        "2026-03-30T10:05:00Z",
    );
    strategy_repo.append(&db.pool, &newer).await.unwrap();

    let latest = strategy_repo.latest(&db.pool).await.unwrap();
    assert_eq!(latest, Some(newer));

    db.cleanup().await;
}

#[tokio::test]
async fn strategy_adoption_history_previous_distinct_revision_prefers_newer_neutral_rows() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();

    let legacy_repo = OperatorTargetAdoptionHistoryRepo;
    legacy_repo
        .append(
            &db.pool,
            &sample_adoption_with_id_at(
                "legacy-adopt",
                "adopt",
                "targets-rev-31",
                Some("targets-rev-29"),
                Some("adoptable-31"),
                Some("candidate-31"),
                "2026-03-30T10:00:00Z",
            ),
        )
        .await
        .unwrap();

    let strategy_repo = OperatorStrategyAdoptionHistoryRepo;
    strategy_repo
        .append(
            &db.pool,
            &sample_strategy_adoption_with_id_at(
                "strategy-adopt",
                "adopt",
                "targets-rev-31",
                Some("targets-rev-30"),
                Some("adoptable-31-b"),
                Some("candidate-31-b"),
                "2026-03-30T10:05:00Z",
            ),
        )
        .await
        .unwrap();

    let previous = strategy_repo
        .previous_distinct_revision(&db.pool, "targets-rev-31")
        .await
        .unwrap();
    assert_eq!(previous.as_deref(), Some("targets-rev-30"));

    db.cleanup().await;
}

#[tokio::test]
async fn adoption_history_rejects_invalid_action_kind() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();

    let repo = OperatorTargetAdoptionHistoryRepo;
    let invalid = sample_adoption_with_id_at(
        "invalid-action",
        "promote",
        "targets-rev-20",
        Some("targets-rev-19"),
        Some("adoptable-20"),
        Some("candidate-20"),
        "2026-03-30T10:20:00Z",
    );

    assert!(repo.append(&db.pool, &invalid).await.is_err());

    db.cleanup().await;
}

#[tokio::test]
async fn adoption_history_rejects_adopt_rows_missing_provenance_links() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();

    let repo = OperatorTargetAdoptionHistoryRepo;
    let invalid = sample_adoption_with_id_at(
        "invalid-adopt",
        "adopt",
        "targets-rev-21",
        Some("targets-rev-20"),
        None,
        None,
        "2026-03-30T10:21:00Z",
    );

    assert!(repo.append(&db.pool, &invalid).await.is_err());

    db.cleanup().await;
}

#[tokio::test]
async fn adoption_history_accepts_rollback_rows_with_null_candidate_links() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();

    let repo = OperatorTargetAdoptionHistoryRepo;
    let rollback = sample_adoption_with_id_at(
        "rollback-1",
        "rollback",
        "targets-rev-30",
        Some("targets-rev-29"),
        None,
        None,
        "2026-03-30T10:30:00Z",
    );

    repo.append(&db.pool, &rollback).await.unwrap();

    let latest = repo.latest(&db.pool).await.unwrap();
    assert_eq!(latest, Some(rollback));

    db.cleanup().await;
}
