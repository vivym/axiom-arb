use std::sync::atomic::{AtomicU64, Ordering};

use chrono::{Duration, Utc};
use domain::ExecutionMode;
use persistence::{
    models::ExecutionAttemptRow, run_migrations, ExecutionAttemptRepo,
    LatestRelevantRunSessionQuery, PersistenceError, RunSessionRepo, RunSessionRow,
    RunSessionState, RuntimeProgressRepo,
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
        configured_operator_strategy_revision: Some("strategy-rev-default".to_owned()),
        active_operator_strategy_revision_at_start: Some("strategy-rev-default".to_owned()),
        rollout_state_at_start: Some("ready".to_owned()),
        real_user_shadow_smoke: false,
    }
}

fn sample_starting_session_for_target(
    run_session_id: &str,
    config_path: &str,
    config_fingerprint: &str,
    startup_target_revision_at_start: &str,
    configured_operator_target_revision: Option<&str>,
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
        configured_operator_target_revision: configured_operator_target_revision.map(str::to_owned),
        active_operator_target_revision_at_start: configured_operator_target_revision
            .map(str::to_owned),
        configured_operator_strategy_revision: configured_operator_target_revision
            .map(|value| value.replacen("targets-rev-", "strategy-rev-", 1)),
        active_operator_strategy_revision_at_start: configured_operator_target_revision
            .map(|value| value.replacen("targets-rev-", "strategy-rev-", 1)),
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
    assert!(progress_columns
        .iter()
        .any(|name| name == "operator_strategy_revision"));

    let attempt_columns: Vec<String> = sqlx::query_scalar(
        "select column_name from information_schema.columns where table_schema = current_schema() and table_name = 'execution_attempts'",
    )
    .fetch_all(&db.pool)
    .await
    .unwrap();
    assert!(attempt_columns.iter().any(|name| name == "run_session_id"));

    let run_session_columns: Vec<String> = sqlx::query_scalar(
        "select column_name from information_schema.columns where table_schema = current_schema() and table_name = 'run_sessions'",
    )
    .fetch_all(&db.pool)
    .await
    .unwrap();
    assert!(run_session_columns
        .iter()
        .any(|name| name == "configured_operator_strategy_revision"));
    assert!(run_session_columns
        .iter()
        .any(|name| name == "active_operator_strategy_revision_at_start"));

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
                Some("targets-rev-1"),
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
                Some("targets-rev-2"),
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
            LatestRelevantRunSessionQuery {
                mode: "live",
                config_path: "config/axiom-arb.local.toml",
                config_fingerprint: "fp-new",
                configured_target: Some("targets-rev-2"),
                startup_target_revision_at_start: "startup-target-2",
                rollout_state: Some("ready"),
                stale_after: Duration::minutes(5),
            },
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(relevant.run_session_id, "rs-new");

    RuntimeProgressRepo
        .record_progress(
            &db.pool,
            41,
            7,
            Some("snapshot-7"),
            Some("targets-rev-1"),
            None,
        )
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

#[tokio::test]
async fn run_session_repo_latest_relevant_treats_overdue_running_session_as_stale_for_ranking() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();

    let stale_started_at = Utc::now() - Duration::minutes(20);
    let fresh_started_at = Utc::now() - Duration::minutes(2);

    RunSessionRepo
        .create_starting(
            &db.pool,
            &sample_starting_session_for_target(
                "rs-stale-running",
                "config/axiom-arb.local.toml",
                "fp-shared",
                "startup-target-shared",
                Some("targets-rev-shared"),
                stale_started_at,
            ),
        )
        .await
        .unwrap();
    RunSessionRepo
        .mark_running(&db.pool, "rs-stale-running", stale_started_at)
        .await
        .unwrap();

    RunSessionRepo
        .create_starting(
            &db.pool,
            &sample_starting_session_for_target(
                "rs-fresh-exited",
                "config/axiom-arb.local.toml",
                "fp-shared",
                "startup-target-shared",
                Some("targets-rev-shared"),
                fresh_started_at,
            ),
        )
        .await
        .unwrap();
    RunSessionRepo
        .mark_running(&db.pool, "rs-fresh-exited", fresh_started_at)
        .await
        .unwrap();
    RunSessionRepo
        .mark_exited(
            &db.pool,
            "rs-fresh-exited",
            fresh_started_at + Duration::seconds(30),
            "success",
            None,
        )
        .await
        .unwrap();

    let relevant = RunSessionRepo
        .latest_relevant(
            &db.pool,
            LatestRelevantRunSessionQuery {
                mode: "live",
                config_path: "config/axiom-arb.local.toml",
                config_fingerprint: "fp-shared",
                configured_target: Some("targets-rev-shared"),
                startup_target_revision_at_start: "startup-target-shared",
                rollout_state: Some("ready"),
                stale_after: Duration::minutes(5),
            },
        )
        .await
        .unwrap()
        .unwrap();

    assert_eq!(relevant.run_session_id, "rs-fresh-exited");

    db.cleanup().await;
}

#[tokio::test]
async fn run_session_repo_latest_relevant_supports_explicit_target_sessions_without_configured_target(
) {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();

    let started_at = Utc::now() - Duration::minutes(1);
    let mut row = sample_starting_session_for_target(
        "rs-explicit",
        "config/axiom-arb.local.toml",
        "fp-explicit",
        "startup-target-explicit",
        None,
        started_at,
    );
    row.target_source_kind = "explicit".to_owned();
    row.active_operator_target_revision_at_start = None;
    row.active_operator_strategy_revision_at_start = None;

    RunSessionRepo
        .create_starting(&db.pool, &row)
        .await
        .unwrap();
    RunSessionRepo
        .mark_running(&db.pool, "rs-explicit", started_at)
        .await
        .unwrap();

    let relevant = RunSessionRepo
        .latest_relevant(
            &db.pool,
            LatestRelevantRunSessionQuery {
                mode: "live",
                config_path: "config/axiom-arb.local.toml",
                config_fingerprint: "fp-explicit",
                configured_target: None,
                startup_target_revision_at_start: "startup-target-explicit",
                rollout_state: Some("ready"),
                stale_after: Duration::minutes(5),
            },
        )
        .await
        .unwrap()
        .unwrap();

    assert_eq!(relevant.run_session_id, "rs-explicit");
    assert_eq!(relevant.configured_operator_target_revision, None);

    db.cleanup().await;
}

#[tokio::test]
async fn run_session_repo_refresh_last_seen_rejects_terminal_sessions() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();

    let started_at = Utc::now() - Duration::minutes(2);
    RunSessionRepo
        .create_starting(
            &db.pool,
            &sample_starting_session_for_target(
                "rs-terminal",
                "config/axiom-arb.local.toml",
                "fp-terminal",
                "startup-target-terminal",
                Some("targets-rev-terminal"),
                started_at,
            ),
        )
        .await
        .unwrap();
    RunSessionRepo
        .mark_running(&db.pool, "rs-terminal", started_at)
        .await
        .unwrap();
    let ended_at = started_at + Duration::seconds(30);
    RunSessionRepo
        .mark_exited(&db.pool, "rs-terminal", ended_at, "success", None)
        .await
        .unwrap();

    let err = RunSessionRepo
        .refresh_last_seen(&db.pool, "rs-terminal", ended_at + Duration::seconds(5))
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        PersistenceError::InvalidRunSessionTransition { .. }
    ));

    let row = RunSessionRepo
        .get(&db.pool, "rs-terminal")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.last_seen_at, ended_at);

    db.cleanup().await;
}

#[tokio::test]
async fn run_session_repo_refresh_last_seen_is_monotonic_for_active_sessions() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();

    let started_at = Utc::now() - Duration::minutes(1);
    RunSessionRepo
        .create_starting(
            &db.pool,
            &sample_starting_session_for_target(
                "rs-monotonic",
                "config/axiom-arb.local.toml",
                "fp-monotonic",
                "startup-target-monotonic",
                Some("targets-rev-monotonic"),
                started_at,
            ),
        )
        .await
        .unwrap();
    RunSessionRepo
        .mark_running(&db.pool, "rs-monotonic", started_at)
        .await
        .unwrap();

    let forward_seen_at = started_at + Duration::seconds(20);
    RunSessionRepo
        .refresh_last_seen(&db.pool, "rs-monotonic", forward_seen_at)
        .await
        .unwrap();
    RunSessionRepo
        .refresh_last_seen(&db.pool, "rs-monotonic", started_at + Duration::seconds(5))
        .await
        .unwrap();

    let row = RunSessionRepo
        .get(&db.pool, "rs-monotonic")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.last_seen_at, forward_seen_at);

    db.cleanup().await;
}

#[tokio::test]
async fn run_session_repo_terminal_state_cannot_be_overwritten_by_later_active_transition() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();

    let started_at = Utc::now() - Duration::minutes(1);
    RunSessionRepo
        .create_starting(
            &db.pool,
            &sample_starting_session_for_target(
                "rs-no-overwrite",
                "config/axiom-arb.local.toml",
                "fp-no-overwrite",
                "startup-target-no-overwrite",
                Some("targets-rev-no-overwrite"),
                started_at,
            ),
        )
        .await
        .unwrap();
    RunSessionRepo
        .mark_running(&db.pool, "rs-no-overwrite", started_at)
        .await
        .unwrap();

    let ended_at = started_at + Duration::seconds(15);
    RunSessionRepo
        .mark_failed(&db.pool, "rs-no-overwrite", ended_at, "boom")
        .await
        .unwrap();

    let err = RunSessionRepo
        .mark_exited(
            &db.pool,
            "rs-no-overwrite",
            ended_at + Duration::seconds(10),
            "success",
            None,
        )
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        PersistenceError::InvalidRunSessionTransition { .. }
    ));

    let row = RunSessionRepo
        .get(&db.pool, "rs-no-overwrite")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.state, RunSessionState::Failed);
    assert_eq!(row.exit_reason.as_deref(), Some("boom"));
    assert_eq!(row.last_seen_at, ended_at);

    db.cleanup().await;
}

#[tokio::test]
async fn run_session_repo_rejects_malformed_adopted_target_snapshot() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();

    let mut row = sample_starting_session_for_target(
        "rs-bad-adopted",
        "config/axiom-arb.local.toml",
        "fp-bad-adopted",
        "startup-target-bad-adopted",
        None,
        Utc::now(),
    );
    row.target_source_kind = "adopted".to_owned();

    let err = RunSessionRepo
        .create_starting(&db.pool, &row)
        .await
        .unwrap_err();
    assert!(matches!(err, PersistenceError::InvalidValue { .. }));

    db.cleanup().await;
}
