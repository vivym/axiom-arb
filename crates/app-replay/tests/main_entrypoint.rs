use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

use chrono::{DateTime, Utc};
use domain::ExecutionMode;
use observability::span_names;
use persistence::{
    models::{
        ExecutionAttemptRow, NegRiskDiscoverySnapshotInput, NegRiskFamilyMemberRow,
        NegRiskFamilyValidationRow, ShadowExecutionArtifactRow,
    },
    persist_discovery_snapshot, run_migrations, ExecutionAttemptRepo, NegRiskFamilyRepo,
    ShadowArtifactRepo,
};
use serde_json::json;
use sqlx::{postgres::PgPoolOptions, PgPool};

static NEXT_SCHEMA_ID: AtomicU64 = AtomicU64::new(1);

#[test]
fn binary_entrypoint_emits_structured_replay_summary() {
    let Some(database_url) = std::env::var_os("DATABASE_URL") else {
        return;
    };
    let config = config_fixture("app-replay.toml");

    let output = Command::new(app_replay_binary())
        .env("DATABASE_URL", database_url)
        .arg("--config")
        .arg(&config)
        .args(["--from-seq", "0", "--limit", "1"])
        .output()
        .expect("app-replay should run");

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("stdout should be utf8");
    let stderr = String::from_utf8(output.stderr).expect("stderr should be utf8");
    let combined = format!("{stdout}{stderr}");

    assert!(
        !combined
            .lines()
            .any(|line| line.starts_with("app-replay processed_count=")),
        "legacy success line should no longer be printed: {combined}"
    );
    assert!(combined.contains(span_names::REPLAY_RUN), "{combined}");
    assert!(combined.contains("after_seq=0"), "{combined}");
    assert!(combined.contains(span_names::REPLAY_SUMMARY), "{combined}");
    assert!(combined.contains("processed_count="), "{combined}");
    assert!(combined.contains("last_journal_seq="), "{combined}");
    assert!(combined.contains("app-replay summary"), "{combined}");
    assert!(
        !combined.contains("replay summary emitted"),
        "legacy success message should no longer be printed: {combined}"
    );
}

#[test]
fn replay_binary_still_accepts_the_new_shared_config_fixture() {
    let Some(database_url) = std::env::var_os("DATABASE_URL") else {
        return;
    };
    let config = config_fixture("app-replay-ux.toml");

    let output = Command::new(app_replay_binary())
        .env("DATABASE_URL", database_url)
        .arg("--config")
        .arg(&config)
        .args(["--from-seq", "0", "--limit", "1"])
        .output()
        .expect("app-replay should run");

    assert!(output.status.success());
}

#[test]
fn binary_entrypoint_runs_with_malformed_live_only_config_sections() {
    let Some(database_url) = std::env::var_os("DATABASE_URL") else {
        return;
    };
    let config = config_fixture("app-replay-malformed-live.toml");

    let output = Command::new(app_replay_binary())
        .env("DATABASE_URL", database_url)
        .arg("--config")
        .arg(&config)
        .args(["--from-seq", "0", "--limit", "1"])
        .output()
        .expect("app-replay should run");

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("stdout should be utf8");
    let stderr = String::from_utf8(output.stderr).expect("stderr should be utf8");
    let combined = format!("{stdout}{stderr}");

    assert!(combined.contains(span_names::REPLAY_RUN), "{combined}");
    assert!(combined.contains("app-replay summary"), "{combined}");
}

#[test]
fn binary_entrypoint_emits_structured_error_log_for_invalid_database_url() {
    let config = config_fixture("app-replay.toml");
    let output = Command::new(app_replay_binary())
        .env("DATABASE_URL", "not-a-valid-postgres-url")
        .arg("--config")
        .arg(&config)
        .args(["--from-seq", "0", "--limit", "1"])
        .output()
        .expect("app-replay should run");

    assert!(
        !output.status.success(),
        "binary should fail for invalid DATABASE_URL"
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout should be utf8");
    let stderr = String::from_utf8(output.stderr).expect("stderr should be utf8");
    let combined = format!("{stdout}{stderr}");

    assert!(combined.contains("app-replay replay failed"), "{combined}");
    assert!(combined.contains(span_names::REPLAY_RUN), "{combined}");
    assert!(combined.contains("after_seq=0"), "{combined}");
    assert!(combined.contains("error with configuration"), "{combined}");
}

#[test]
fn binary_entrypoint_rejects_missing_database_url_even_with_valid_config() {
    let config = config_fixture("app-replay.toml");
    let output = Command::new(app_replay_binary())
        .env_remove("DATABASE_URL")
        .arg("--config")
        .arg(&config)
        .args(["--from-seq", "0", "--limit", "1"])
        .output()
        .expect("app-replay should run");

    assert!(
        !output.status.success(),
        "binary should fail when DATABASE_URL is missing"
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout should be utf8");
    let stderr = String::from_utf8(output.stderr).expect("stderr should be utf8");
    let combined = format!("{stdout}{stderr}");

    assert!(combined.contains("app-replay replay failed"), "{combined}");
    assert!(combined.contains(span_names::REPLAY_RUN), "{combined}");
    assert!(combined.contains("DATABASE_URL"), "{combined}");
}

#[test]
fn binary_entrypoint_emits_structured_error_log_for_invalid_cli_args() {
    let config = config_fixture("app-replay.toml");
    let output = Command::new(app_replay_binary())
        .arg("--config")
        .arg(&config)
        .args(["--limit", "1"])
        .output()
        .expect("app-replay should run");

    assert!(
        !output.status.success(),
        "binary should fail for missing replay arguments"
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout should be utf8");
    let stderr = String::from_utf8(output.stderr).expect("stderr should be utf8");
    let combined = format!("{stdout}{stderr}");

    assert!(combined.contains("app-replay replay failed"), "{combined}");
    assert!(combined.contains(span_names::REPLAY_RUN), "{combined}");
    assert!(combined.contains("--from-seq <FROM_SEQ>"), "{combined}");
    assert!(
        combined.contains("Usage: app-replay --config <CONFIG>"),
        "{combined}"
    );
}

#[tokio::test]
async fn app_replay_main_emits_operator_facing_negrisk_summary_without_new_metrics() {
    let Some(database_url) = std::env::var_os("DATABASE_URL") else {
        return;
    };
    let database_url = database_url
        .into_string()
        .expect("DATABASE_URL should be valid utf8");
    let db = TestDatabase::new(&database_url).await;
    run_migrations(&db.pool).await.unwrap();
    seed_negrisk_summary_rows(&db.pool).await;
    let config = config_fixture("app-replay.toml");

    let output = Command::new(app_replay_binary())
        .env("DATABASE_URL", &database_url)
        .env("PGOPTIONS", format!("-c search_path={}", db.schema))
        .arg("--config")
        .arg(&config)
        .args(["--from-seq", "0", "--limit", "10"])
        .output()
        .expect("app-replay should run");

    db.cleanup().await;

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("stdout should be utf8");
    let stderr = String::from_utf8(output.stderr).expect("stderr should be utf8");
    let combined = format!("{stdout}{stderr}");

    assert!(
        combined.contains(span_names::REPLAY_NEGRISK_SUMMARY),
        "{combined}"
    );
    assert!(
        combined.contains("app-replay neg-risk summary"),
        "{combined}"
    );
    assert!(
        combined.contains("latest_metadata_snapshot_hash"),
        "{combined}"
    );
    assert!(combined.contains("sha256:discovery-7"), "{combined}");
}

#[tokio::test]
async fn app_replay_main_emits_operator_facing_negrisk_shadow_smoke_summary() {
    let Some(database_url) = std::env::var_os("DATABASE_URL") else {
        return;
    };
    let database_url = database_url
        .into_string()
        .expect("DATABASE_URL should be valid utf8");
    let db = TestDatabase::new(&database_url).await;
    run_migrations(&db.pool).await.unwrap();
    seed_negrisk_shadow_smoke_rows(&db.pool).await;
    let config = config_fixture("app-replay.toml");

    let output = Command::new(app_replay_binary())
        .env("DATABASE_URL", &database_url)
        .env("PGOPTIONS", format!("-c search_path={}", db.schema))
        .arg("--config")
        .arg(&config)
        .args(["--from-seq", "0", "--limit", "10"])
        .output()
        .expect("app-replay should run");

    db.cleanup().await;

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("stdout should be utf8");
    let stderr = String::from_utf8(output.stderr).expect("stderr should be utf8");
    let combined = format!("{stdout}{stderr}");

    assert!(
        combined.contains("app-replay neg-risk shadow smoke"),
        "{combined}"
    );
    assert!(combined.contains("shadow_attempt_count=1"), "{combined}");
    assert!(combined.contains("shadow_artifact_count=2"), "{combined}");
    assert!(
        combined.contains("app-replay.negrisk_shadow_smoke"),
        "{combined}"
    );
}

fn app_replay_binary() -> PathBuf {
    if let Some(path) = std::env::var_os("CARGO_BIN_EXE_app-replay") {
        return PathBuf::from(path);
    }

    let mut path = std::env::current_exe().expect("current test executable path");
    path.pop();
    if path.ends_with("deps") {
        path.pop();
    }
    path.push("app-replay");
    if cfg!(windows) {
        path.set_extension("exe");
    }

    path
}

fn config_fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("config-schema")
        .join("tests")
        .join("fixtures")
        .join(name)
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
            "app_replay_main_{}_{}",
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

async fn seed_negrisk_summary_rows(pool: &PgPool) {
    persist_discovery_snapshot(
        pool,
        NegRiskDiscoverySnapshotInput {
            discovery_revision: 7,
            metadata_snapshot_hash: "sha256:discovery-7".to_owned(),
            family_ids: vec!["family-1".to_owned()],
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
}

async fn seed_negrisk_shadow_smoke_rows(pool: &PgPool) {
    ExecutionAttemptRepo
        .append(
            pool,
            &ExecutionAttemptRow {
                attempt_id: "attempt-shadow-1".to_owned(),
                plan_id: "request-bound:5:req-1:negrisk-shadow-family:family-a".to_owned(),
                snapshot_id: "snapshot-7".to_owned(),
                route: "neg-risk".to_owned(),
                scope: "family:family-a".to_owned(),
                matched_rule_id: Some("family-a-live".to_owned()),
                execution_mode: ExecutionMode::Shadow,
                attempt_no: 1,
                idempotency_key: "idem-attempt-shadow-1".to_owned(),
                run_session_id: None,
            },
        )
        .await
        .unwrap();

    ShadowArtifactRepo
        .append(
            pool,
            ShadowExecutionArtifactRow {
                attempt_id: "attempt-shadow-1".to_owned(),
                stream: "neg-risk-shadow-plan".to_owned(),
                payload: json!({
                    "attempt_id": "attempt-shadow-1",
                    "plan_id": "request-bound:5:req-1:negrisk-shadow-family:family-a",
                    "snapshot_id": "snapshot-7",
                    "route": "neg-risk",
                    "scope": "family:family-a",
                    "matched_rule_id": "family-a-live",
                }),
            },
        )
        .await
        .unwrap();

    ShadowArtifactRepo
        .append(
            pool,
            ShadowExecutionArtifactRow {
                attempt_id: "attempt-shadow-1".to_owned(),
                stream: "neg-risk-shadow-result".to_owned(),
                payload: json!({
                    "attempt_id": "attempt-shadow-1",
                    "status": "shadow_recorded",
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
            outcome_label: "Bob".to_owned(),
            is_placeholder: false,
            is_other: false,
            neg_risk_variant: "standard".to_owned(),
        },
    ]
}

fn ts(value: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value)
        .expect("timestamp should parse")
        .with_timezone(&Utc)
}
