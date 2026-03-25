use std::{
    borrow::Cow,
    sync::atomic::{AtomicU64, Ordering},
};

use domain::ExecutionMode;
use persistence::{
    models::{ExecutionAttemptRow, LiveExecutionArtifactRow, LiveSubmissionRecordRow},
    run_migrations, ExecutionAttemptRepo, LiveArtifactRepo, LiveSubmissionRepo,
};
use serde_json::json;
use sqlx::migrate::{Migration, MigrationType, Migrator};
use sqlx::{postgres::PgPoolOptions, PgPool};

use app_replay::{
    load_negrisk_live_attempt_artifacts, load_negrisk_live_submission_records,
    NegRiskLiveAttemptArtifacts, NegRiskLiveSubmissionRecord,
};

static NEXT_SCHEMA_ID: AtomicU64 = AtomicU64::new(1);

struct TestDatabase {
    admin_pool: PgPool,
    pool: PgPool,
    schema: String,
}

impl TestDatabase {
    async fn new() -> Self {
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set for app-replay tests");

        let admin_pool = PgPoolOptions::new()
            .max_connections(2)
            .connect(&database_url)
            .await
            .expect("test database should connect");

        let schema = format!(
            "app_replay_negrisk_live_contract_{}_{}",
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

fn sample_attempt(
    attempt_id: &str,
    mode: ExecutionMode,
    plan_id: &str,
    route: &str,
) -> ExecutionAttemptRow {
    ExecutionAttemptRow {
        attempt_id: attempt_id.to_owned(),
        plan_id: plan_id.to_owned(),
        snapshot_id: "snapshot-7".to_owned(),
        route: route.to_owned(),
        scope: "family:family-a".to_owned(),
        matched_rule_id: Some("rule-negrisk-live".to_owned()),
        execution_mode: mode,
        attempt_no: 1,
        idempotency_key: format!("idem-{attempt_id}"),
    }
}

fn request_bound_plan_id(request_id: &str, inner_plan_id: &str) -> String {
    format!(
        "request-bound:{}:{}:{}",
        request_id.len(),
        request_id,
        inner_plan_id
    )
}

fn artifact(attempt_id: &str, stream: &str, kind: &str) -> LiveExecutionArtifactRow {
    LiveExecutionArtifactRow {
        attempt_id: attempt_id.to_owned(),
        stream: stream.to_owned(),
        payload: json!({
            "kind": kind,
            "attempt_id": attempt_id,
        }),
    }
}

fn submission_record(attempt_id: &str, submission_ref: &str) -> LiveSubmissionRecordRow {
    LiveSubmissionRecordRow {
        submission_ref: submission_ref.to_owned(),
        attempt_id: attempt_id.to_owned(),
        route: "neg-risk".to_owned(),
        scope: "family-c".to_owned(),
        provider: "venue-polymarket".to_owned(),
        state: "pending_reconcile".to_owned(),
        payload: json!({
            "submission_ref": submission_ref,
            "family_id": "family-c",
            "route": "neg-risk",
            "reason": "awaiting_resolve",
        }),
    }
}

async fn create_legacy_live_schema(pool: &PgPool) {
    sqlx::query(
        r#"
        CREATE TABLE execution_attempts (
            attempt_id TEXT PRIMARY KEY,
            plan_id TEXT NOT NULL,
            snapshot_id TEXT NOT NULL,
            execution_mode TEXT NOT NULL CHECK (
                execution_mode IN ('disabled', 'shadow', 'live', 'reduce_only', 'recovery_only')
            ),
            attempt_no INTEGER NOT NULL,
            idempotency_key TEXT NOT NULL,
            outcome TEXT,
            payload JSONB NOT NULL DEFAULT '{}'::JSONB,
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        )
        "#,
    )
    .execute(pool)
    .await
    .unwrap();

    sqlx::query(
        r#"
        CREATE TABLE live_execution_artifacts (
            attempt_id TEXT NOT NULL REFERENCES execution_attempts (attempt_id),
            stream TEXT NOT NULL,
            payload JSONB NOT NULL,
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            PRIMARY KEY (attempt_id, stream)
        )
        "#,
    )
    .execute(pool)
    .await
    .unwrap();
}

fn audit_anchor_migrator() -> Migrator {
    Migrator {
        migrations: Cow::Owned(vec![Migration::new(
            8,
            Cow::Borrowed("execution_attempt_audit_anchor"),
            MigrationType::Simple,
            Cow::Borrowed(include_str!(
                "../../../migrations/0008_execution_attempt_audit_anchor.sql"
            )),
            false,
        )]),
        ignore_missing: false,
        locking: true,
        no_tx: false,
    }
}

async fn apply_audit_anchor_migration(pool: &PgPool) {
    audit_anchor_migrator().run(pool).await.unwrap();
}

#[tokio::test]
async fn negrisk_live_contract_lists_only_negrisk_live_attempts_with_artifacts() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();

    ExecutionAttemptRepo
        .append(
            &db.pool,
            &sample_attempt(
                "attempt-negrisk-live-1",
                ExecutionMode::Live,
                &request_bound_plan_id(
                    "req-1",
                    "negrisk-submit-family:family-a:condition-1:token-1:0.4:5",
                ),
                "neg-risk",
            ),
        )
        .await
        .unwrap();

    ExecutionAttemptRepo
        .append(
            &db.pool,
            &sample_attempt(
                "attempt-non-negrisk-live-1",
                ExecutionMode::Live,
                &request_bound_plan_id("req-2", "fullset-buy-merge:condition-1"),
                "fullset",
            ),
        )
        .await
        .unwrap();

    ExecutionAttemptRepo
        .append(
            &db.pool,
            &sample_attempt(
                "attempt-negrisk-route-only-live-1",
                ExecutionMode::Live,
                &request_bound_plan_id("req-2b", "fullset-buy-merge:condition-9"),
                "neg-risk",
            ),
        )
        .await
        .unwrap();

    ExecutionAttemptRepo
        .append(
            &db.pool,
            &sample_attempt(
                "attempt-negrisk-plan-only-live-1",
                ExecutionMode::Live,
                &request_bound_plan_id(
                    "req-2c",
                    "negrisk-submit-family:family-z:condition-9:token-9:0.4:5",
                ),
                "fullset",
            ),
        )
        .await
        .unwrap();

    ExecutionAttemptRepo
        .append(
            &db.pool,
            &sample_attempt(
                "attempt-negrisk-shadow-1",
                ExecutionMode::Shadow,
                &request_bound_plan_id(
                    "req-3",
                    "negrisk-submit-family:family-b:condition-2:token-2:0.4:5",
                ),
                "neg-risk",
            ),
        )
        .await
        .unwrap();

    LiveArtifactRepo
        .append(
            &db.pool,
            artifact("attempt-negrisk-live-1", "negrisk.live", "planned_order"),
        )
        .await
        .unwrap();
    LiveArtifactRepo
        .append(
            &db.pool,
            artifact(
                "attempt-negrisk-route-only-live-1",
                "negrisk.audit",
                "audit_trail",
            ),
        )
        .await
        .unwrap();

    let rows = load_negrisk_live_attempt_artifacts(&db.pool).await.unwrap();

    assert_eq!(
        rows,
        vec![
            NegRiskLiveAttemptArtifacts {
                attempt: sample_attempt(
                    "attempt-negrisk-live-1",
                    ExecutionMode::Live,
                    &request_bound_plan_id(
                        "req-1",
                        "negrisk-submit-family:family-a:condition-1:token-1:0.4:5",
                    ),
                    "neg-risk",
                ),
                artifacts: vec![artifact(
                    "attempt-negrisk-live-1",
                    "negrisk.live",
                    "planned_order"
                )],
            },
            NegRiskLiveAttemptArtifacts {
                attempt: sample_attempt(
                    "attempt-negrisk-route-only-live-1",
                    ExecutionMode::Live,
                    &request_bound_plan_id("req-2b", "fullset-buy-merge:condition-9"),
                    "neg-risk",
                ),
                artifacts: vec![artifact(
                    "attempt-negrisk-route-only-live-1",
                    "negrisk.audit",
                    "audit_trail"
                )],
            },
        ]
    );

    db.cleanup().await;
}

#[tokio::test]
async fn negrisk_live_contract_returns_single_row_per_attempt_stream_after_duplicate_append() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();

    let plan_id = request_bound_plan_id(
        "req-4",
        "negrisk-submit-family:family-c:condition-3:token-3:0.4:5",
    );
    let attempt = sample_attempt(
        "attempt-negrisk-live-dup-1",
        ExecutionMode::Live,
        &plan_id,
        "neg-risk",
    );

    ExecutionAttemptRepo
        .append(&db.pool, &attempt)
        .await
        .unwrap();

    LiveArtifactRepo
        .append(
            &db.pool,
            artifact(
                "attempt-negrisk-live-dup-1",
                "negrisk.live",
                "planned_order",
            ),
        )
        .await
        .unwrap();
    LiveArtifactRepo
        .append(
            &db.pool,
            artifact(
                "attempt-negrisk-live-dup-1",
                "negrisk.live",
                "planned_order",
            ),
        )
        .await
        .unwrap();

    let rows = load_negrisk_live_attempt_artifacts(&db.pool).await.unwrap();

    assert_eq!(
        rows,
        vec![NegRiskLiveAttemptArtifacts {
            attempt,
            artifacts: vec![artifact(
                "attempt-negrisk-live-dup-1",
                "negrisk.live",
                "planned_order",
            )],
        }]
    );

    db.cleanup().await;
}

#[tokio::test]
async fn negrisk_live_contract_keeps_legacy_negrisk_attempts_replay_visible_after_backfill() {
    let db = TestDatabase::new().await;
    create_legacy_live_schema(&db.pool).await;

    let attempt_id = "attempt-legacy-live-contract-1";
    let plan_id = "negrisk-submit-family:family-c:condition-3:token-3:0.4:5";

    sqlx::query(
        r#"
        INSERT INTO execution_attempts (
            attempt_id,
            plan_id,
            snapshot_id,
            execution_mode,
            attempt_no,
            idempotency_key
        )
        VALUES ($1, $2, $3, $4, $5, $6)
        "#,
    )
    .bind(attempt_id)
    .bind(plan_id)
    .bind("snapshot-7")
    .bind("live")
    .bind(1_i32)
    .bind("idem-legacy-live-contract-1")
    .execute(&db.pool)
    .await
    .unwrap();

    sqlx::query(
        r#"
        INSERT INTO live_execution_artifacts (attempt_id, stream, payload)
        VALUES ($1, $2, $3)
        "#,
    )
    .bind(attempt_id)
    .bind("negrisk.live")
    .bind(json!({
        "attempt_id": attempt_id,
        "kind": "planned_order",
    }))
    .execute(&db.pool)
    .await
    .unwrap();

    apply_audit_anchor_migration(&db.pool).await;

    let rows = load_negrisk_live_attempt_artifacts(&db.pool).await.unwrap();

    assert_eq!(
        rows,
        vec![NegRiskLiveAttemptArtifacts {
            attempt: ExecutionAttemptRow {
                attempt_id: attempt_id.to_owned(),
                plan_id: plan_id.to_owned(),
                snapshot_id: "snapshot-7".to_owned(),
                route: "neg-risk".to_owned(),
                scope: "family-c".to_owned(),
                matched_rule_id: None,
                execution_mode: ExecutionMode::Live,
                attempt_no: 1,
                idempotency_key: "idem-legacy-live-contract-1".to_owned(),
            },
            artifacts: vec![artifact(attempt_id, "negrisk.live", "planned_order")],
        }]
    );

    db.cleanup().await;
}

#[tokio::test]
async fn negrisk_live_contract_loads_live_submission_records_for_replay_resume_truth() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();

    let attempt = sample_attempt(
        "attempt-negrisk-live-submit-1",
        ExecutionMode::Live,
        &request_bound_plan_id(
            "req-5",
            "negrisk-submit-family:family-c:condition-3:token-3:0.4:5",
        ),
        "neg-risk",
    );
    ExecutionAttemptRepo
        .append(&db.pool, &attempt)
        .await
        .unwrap();

    let row = submission_record("attempt-negrisk-live-submit-1", "submission-ref-1");
    LiveSubmissionRepo
        .append(&db.pool, row.clone())
        .await
        .unwrap();

    let rows = load_negrisk_live_submission_records(&db.pool)
        .await
        .unwrap();

    assert_eq!(
        rows,
        vec![NegRiskLiveSubmissionRecord {
            attempt,
            submissions: vec![row],
        }]
    );

    db.cleanup().await;
}
