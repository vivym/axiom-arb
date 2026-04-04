use std::sync::atomic::{AtomicU64, Ordering};

use chrono::{Duration, Utc};
use domain::ExecutionMode;
use persistence::{
    models::ExecutionAttemptRow, run_migrations, ExecutionAttemptRepo, PersistenceError,
    RunSessionRepo, RunSessionRow, RunSessionState, RuntimeProgressRepo,
};
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
            "persistence_run_session_test_{}_{}",
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

fn sample_starting_session(run_session_id: &str) -> RunSessionRow {
    RunSessionRow {
        run_session_id: run_session_id.to_owned(),
        invoked_by: "run".to_owned(),
        mode: "live".to_owned(),
        state: RunSessionState::Starting,
        started_at: Utc::now(),
        last_seen_at: Utc::now(),
        ended_at: None,
        exit_status: None,
        exit_reason: None,
        config_path: "config/axiom-arb.local.toml".to_owned(),
        config_fingerprint: "fp-default".to_owned(),
        target_source_kind: "adopted".to_owned(),
        startup_target_revision_at_start: "startup-target-default".to_owned(),
        configured_operator_target_revision: Some("targets-rev-default".to_owned()),
        active_operator_target_revision_at_start: Some("targets-rev-default".to_owned()),
        rollout_state_at_start: Some("ready".to_owned()),
        real_user_shadow_smoke: false,
    }
}

fn sample_starting_session_for_target(
    run_session_id: &str,
    config_path: &str,
    config_fingerprint: &str,
    startup_target_revision_at_start: &str,
    configured_operator_target_revision: &str,
    started_at: chrono::DateTime<Utc>,
) -> RunSessionRow {
    RunSessionRow {
        run_session_id: run_session_id.to_owned(),
        invoked_by: "run".to_owned(),
        mode: "live".to_owned(),
        state: RunSessionState::Starting,
        started_at,
        last_seen_at: started_at,
        ended_at: None,
        exit_status: None,
        exit_reason: None,
        config_path: config_path.to_owned(),
        config_fingerprint: config_fingerprint.to_owned(),
        target_source_kind: "adopted".to_owned(),
        startup_target_revision_at_start: startup_target_revision_at_start.to_owned(),
        configured_operator_target_revision: Some(configured_operator_target_revision.to_owned()),
        active_operator_target_revision_at_start: Some(
            configured_operator_target_revision.to_owned(),
        ),
        rollout_state_at_start: Some("ready".to_owned()),
        real_user_shadow_smoke: false,
    }
}

fn sample_attempt(attempt_id: &str, run_session_id: Option<&str>) -> ExecutionAttemptRow {
    ExecutionAttemptRow {
        attempt_id: attempt_id.to_owned(),
        plan_id: format!("plan-{attempt_id}"),
        snapshot_id: "snapshot-7".to_owned(),
        route: "neg-risk".to_owned(),
        scope: "family:family-1".to_owned(),
        matched_rule_id: Some("rule-family-anchor".to_owned()),
        execution_mode: ExecutionMode::Live,
        attempt_no: 1,
        idempotency_key: format!("idem-{attempt_id}"),
        run_session_id: run_session_id.map(str::to_owned),
    }
}

#[tokio::test]
async fn run_sessions_migration_creates_table_and_session_link_columns() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();

    let tables: Vec<String> = sqlx::query_scalar(
        "select table_name from information_schema.tables where table_schema = current_schema()",
    )
    .fetch_all(&db.pool)
    .await
    .unwrap();

    assert!(tables.iter().any(|name| name == "run_sessions"));

    let progress_columns: Vec<String> = sqlx::query_scalar(
        "select column_name from information_schema.columns where table_schema = current_schema() and table_name = 'runtime_apply_progress'",
    )
    .fetch_all(&db.pool)
    .await
    .unwrap();
    assert!(progress_columns
        .iter()
        .any(|name| name == "active_run_session_id"));

    let attempt_columns: Vec<String> = sqlx::query_scalar(
        "select column_name from information_schema.columns where table_schema = current_schema() and table_name = 'execution_attempts'",
    )
    .fetch_all(&db.pool)
    .await
    .unwrap();
    assert!(attempt_columns.iter().any(|name| name == "run_session_id"));

    db.cleanup().await;
}

#[test]
fn run_session_state_labels_are_stable() {
    assert_eq!(RunSessionState::Starting.as_str(), "starting");
    assert_eq!(RunSessionState::Running.as_str(), "running");
    assert_eq!(RunSessionState::Exited.as_str(), "exited");
    assert_eq!(RunSessionState::Failed.as_str(), "failed");
}

#[tokio::test]
async fn run_sessions_reject_invalid_state_labels() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();

    let err = sqlx::query(
        r#"
        INSERT INTO run_sessions (
            run_session_id,
            invoked_by,
            mode,
            state,
            started_at,
            last_seen_at,
            ended_at,
            exit_status,
            exit_reason,
            config_path,
            config_fingerprint,
            target_source_kind,
            startup_target_revision_at_start,
            configured_operator_target_revision,
            active_operator_target_revision_at_start,
            rollout_state_at_start,
            real_user_shadow_smoke
        )
        VALUES (
            $1,
            $2,
            $3,
            $4,
            NOW(),
            NOW(),
            NULL,
            NULL,
            NULL,
            $5,
            $6,
            $7,
            $8,
            NULL,
            NULL,
            NULL,
            false
        )
        "#,
    )
    .bind("run-session-invalid-1")
    .bind("tester")
    .bind("daemon")
    .bind("paused")
    .bind("/tmp/run-session-config.toml")
    .bind("fingerprint-1")
    .bind("source")
    .bind("rev-start")
    .execute(&db.pool)
    .await
    .unwrap_err();

    assert!(
        err.to_string().contains("state") || err.to_string().contains("check constraint"),
        "unexpected database error: {err}"
    );

    db.cleanup().await;
}

#[tokio::test]
async fn run_session_repo_round_trips_starting_running_and_terminal_states() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();

    RunSessionRepo
        .create_starting(&db.pool, &sample_starting_session("rs-1"))
        .await
        .unwrap();

    let running_at = Utc::now();
    RunSessionRepo
        .mark_running(&db.pool, "rs-1", running_at)
        .await
        .unwrap();

    let exited_at = running_at + Duration::seconds(10);
    RunSessionRepo
        .mark_exited(&db.pool, "rs-1", exited_at, "success", None)
        .await
        .unwrap();

    let row = RunSessionRepo.get(&db.pool, "rs-1").await.unwrap().unwrap();
    assert_eq!(row.state, RunSessionState::Exited);
    assert_eq!(row.exit_status.as_deref(), Some("success"));
    assert_eq!(row.exit_reason, None);
    assert_eq!(row.ended_at, Some(exited_at));
    assert_eq!(row.last_seen_at, exited_at);

    let err = RunSessionRepo
        .mark_running(&db.pool, "rs-1", exited_at + Duration::seconds(1))
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        PersistenceError::InvalidRunSessionTransition { .. }
    ));

    db.cleanup().await;
}

#[tokio::test]
async fn run_session_repo_projects_stale_from_freshness_without_writing_stale_state() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();

    let mut row = sample_starting_session("rs-old");
    row.started_at = Utc::now() - Duration::minutes(10);
    row.last_seen_at = row.started_at;

    RunSessionRepo
        .create_starting(&db.pool, &row)
        .await
        .unwrap();
    RunSessionRepo
        .mark_running(&db.pool, "rs-old", row.last_seen_at)
        .await
        .unwrap();

    let projected = RunSessionRepo
        .load_with_projected_state(&db.pool, "rs-old", Duration::minutes(5))
        .await
        .unwrap()
        .unwrap();

    assert_eq!(projected.state_label, "stale");
    assert!(projected.is_stale);

    let raw = RunSessionRepo
        .get(&db.pool, "rs-old")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(raw.state, RunSessionState::Running);

    let stored_state: String =
        sqlx::query_scalar("SELECT state FROM run_sessions WHERE run_session_id = $1")
            .bind("rs-old")
            .fetch_one(&db.pool)
            .await
            .unwrap();
    assert_eq!(stored_state, "running");

    db.cleanup().await;
}

#[tokio::test]
async fn run_session_repo_selects_latest_relevant_and_conflicting_active_sessions() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();

    let old_started_at = Utc::now() - Duration::minutes(30);
    let new_started_at = Utc::now() - Duration::minutes(5);

    RunSessionRepo
        .create_starting(
            &db.pool,
            &sample_starting_session_for_target(
                "rs-old",
                "config/axiom-arb.local.toml",
                "fp-old",
                "startup-target-1",
                "targets-rev-1",
                old_started_at,
            ),
        )
        .await
        .unwrap();
    RunSessionRepo
        .mark_running(&db.pool, "rs-old", old_started_at)
        .await
        .unwrap();

    RunSessionRepo
        .create_starting(
            &db.pool,
            &sample_starting_session_for_target(
                "rs-new",
                "config/axiom-arb.local.toml",
                "fp-new",
                "startup-target-2",
                "targets-rev-2",
                new_started_at,
            ),
        )
        .await
        .unwrap();
    RunSessionRepo
        .mark_running(&db.pool, "rs-new", new_started_at)
        .await
        .unwrap();
    RunSessionRepo
        .mark_exited(
            &db.pool,
            "rs-new",
            new_started_at + Duration::minutes(1),
            "success",
            None,
        )
        .await
        .unwrap();

    let relevant = RunSessionRepo
        .latest_relevant(
            &db.pool,
            "live",
            "config/axiom-arb.local.toml",
            "fp-new",
            "targets-rev-2",
            "startup-target-2",
            Some("ready"),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(relevant.run_session_id, "rs-new");

    RuntimeProgressRepo
        .record_progress(&db.pool, 41, 7, Some("snapshot-7"), Some("targets-rev-1"))
        .await
        .unwrap();
    RuntimeProgressRepo
        .set_active_run_session_id(&db.pool, "rs-old")
        .await
        .unwrap();

    let conflicting = RunSessionRepo
        .conflicting_active_for_run_session(&db.pool, "rs-old")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(conflicting.run_session_id, "rs-old");

    ExecutionAttemptRepo
        .append(&db.pool, &sample_attempt("attempt-1", Some("rs-new")))
        .await
        .unwrap();

    let resolved = RunSessionRepo
        .resolve_unique_for_attempt_id(&db.pool, "attempt-1")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(resolved.run_session_id, "rs-new");

    db.cleanup().await;
}
