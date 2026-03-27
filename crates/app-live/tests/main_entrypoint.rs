use app_live::load_neg_risk_live_targets;
use domain::ExecutionMode;
use observability::span_names;
use persistence::{
    models::{ExecutionAttemptRow, LiveSubmissionRecordRow, RuntimeProgressRow},
    run_migrations, ExecutionAttemptRepo, LiveSubmissionRepo, RuntimeProgressRepo,
};
use sqlx::{postgres::PgPoolOptions, PgPool};
use std::{
    ffi::OsString,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

#[cfg(unix)]
use std::os::unix::ffi::OsStringExt;

static NEXT_SCHEMA_ID: AtomicU64 = AtomicU64::new(1);

#[test]
fn binary_entrypoint_emits_structured_bootstrap_log() {
    let output = app_live_output("paper", None);

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("stdout should be utf8");
    let stderr = String::from_utf8(output.stderr).expect("stderr should be utf8");
    let combined = format!("{stdout}{stderr}");

    assert!(
        !combined
            .lines()
            .any(|line| line.starts_with("app-live starting ")),
        "legacy success line should no longer be printed: {combined}"
    );
    assert!(
        combined.contains(span_names::APP_BOOTSTRAP_COMPLETE),
        "{combined}"
    );
    assert!(
        combined.contains("app-live bootstrap complete"),
        "{combined}"
    );
    assert!(combined.contains("app_mode=paper"), "{combined}");
    assert!(combined.contains("bootstrap_status=Ready"), "{combined}");
    assert!(
        combined.contains("promoted_from_bootstrap=true"),
        "{combined}"
    );
    assert!(combined.contains("runtime_mode=Healthy"), "{combined}");
    assert!(combined.contains("fullset_mode=Live"), "{combined}");
    assert!(combined.contains("negrisk_mode=Shadow"), "{combined}");
    assert!(combined.contains("pending_reconcile_count=0"), "{combined}");
    assert!(combined.contains("global_posture=healthy"), "{combined}");
    assert!(combined.contains("ingress_backlog=0"), "{combined}");
    assert!(combined.contains("follow_up_backlog=0"), "{combined}");
    assert!(
        combined.contains("published_snapshot_id=snapshot-0"),
        "{combined}"
    );
}

#[test]
fn binary_entrypoint_emits_structured_bootstrap_log_after_metric_removal() {
    let output = app_live_output("paper", None);
    let combined = format!(
        "{}{}",
        String::from_utf8(output.stdout).unwrap(),
        String::from_utf8(output.stderr).unwrap()
    );

    assert!(combined.contains("app-live bootstrap complete"));
    assert!(combined.contains("neg_risk_live_attempt_count"));
}

#[test]
fn binary_entrypoint_emits_structured_error_log_for_invalid_mode() {
    let output = app_live_output("invalid-mode", None);

    assert!(
        !output.status.success(),
        "binary should fail for invalid AXIOM_MODE"
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout should be utf8");
    let stderr = String::from_utf8(output.stderr).expect("stderr should be utf8");
    let combined = format!("{stdout}{stderr}");

    assert!(
        combined.contains("ERROR app-live bootstrap failed"),
        "{combined}"
    );
    assert!(
        combined.contains("unsupported AXIOM_MODE 'invalid-mode'"),
        "{combined}"
    );
}

#[test]
fn paper_entrypoint_ignores_invalid_neg_risk_target_config() {
    let output = app_live_output(
        "paper",
        Some(
            r#"
            [
              {
                "family_id": "family-a",
                "members": [
                  { "condition_id": "condition-1", "token_id": "token-1", "price": "0.43", "quantity": "5" }
                ]
              },
            ]
            "#,
        ),
    );

    assert!(
        output.status.success(),
        "paper mode should ignore live config"
    );
}

#[test]
fn paper_entrypoint_ignores_invalid_local_signer_config() {
    let output = app_live_output_raw_env_with_signer(
        "paper",
        None,
        None,
        None,
        Some(OsString::from("{")),
        None,
    );

    assert!(
        output.status.success(),
        "paper mode should ignore live signer config"
    );
}

#[test]
fn live_entrypoint_rejects_invalid_neg_risk_target_config() {
    let output = app_live_output(
        "live",
        Some(
            r#"
            [
              {
                "family_id": "family-a",
                "members": [
                  { "condition_id": "condition-1", "token_id": "token-1", "price": "0.43", "quantity": "5" }
                ]
              },
            ]
            "#,
        ),
    );

    assert!(
        !output.status.success(),
        "binary should fail for invalid neg-risk live target config"
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout should be utf8");
    let stderr = String::from_utf8(output.stderr).expect("stderr should be utf8");
    let combined = format!("{stdout}{stderr}");

    assert!(
        combined.contains("ERROR app-live bootstrap failed"),
        "{combined}"
    );
    assert!(
        combined.contains("invalid neg-risk live target config"),
        "{combined}"
    );
}

#[test]
fn live_entrypoint_rejects_duplicate_neg_risk_target_config() {
    let output = app_live_output(
        "live",
        Some(
            r#"
            [
              {
                "family_id": "family-a",
                "members": [
                  { "condition_id": "condition-1", "token_id": "token-1", "price": "0.43", "quantity": "5" }
                ]
              },
              {
                "family_id": "family-a",
                "members": [
                  { "condition_id": "condition-2", "token_id": "token-2", "price": "0.41", "quantity": "5" }
                ]
              }
            ]
            "#,
        ),
    );

    assert!(
        !output.status.success(),
        "binary should fail for duplicate neg-risk family ids"
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout should be utf8");
    let stderr = String::from_utf8(output.stderr).expect("stderr should be utf8");
    let combined = format!("{stdout}{stderr}");

    assert!(
        combined.contains("duplicate neg-risk family_id in live target config"),
        "{combined}"
    );
}

#[test]
fn live_entrypoint_rejects_blank_neg_risk_target_config() {
    let output = app_live_output("live", Some(""));

    assert!(
        !output.status.success(),
        "binary should fail for blank neg-risk live target config"
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout should be utf8");
    let stderr = String::from_utf8(output.stderr).expect("stderr should be utf8");
    let combined = format!("{stdout}{stderr}");

    assert!(
        combined.contains("invalid neg-risk live target config"),
        "{combined}"
    );
}

#[cfg(unix)]
#[test]
fn live_entrypoint_rejects_non_utf8_neg_risk_target_config() {
    let output = app_live_output_raw_env(
        "live",
        Some(OsString::from_vec(vec![0xff, 0xfe, 0xfd])),
        Option::<OsString>::None,
        Option::<OsString>::None,
    );

    assert!(
        !output.status.success(),
        "binary should fail for non-UTF-8 neg-risk live target config"
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout should be utf8");
    let stderr = String::from_utf8(output.stderr).expect("stderr should be utf8");
    let combined = format!("{stdout}{stderr}");

    assert!(
        combined.contains("invalid value for AXIOM_NEG_RISK_LIVE_TARGETS"),
        "{combined}"
    );
}

#[cfg(unix)]
#[test]
fn paper_entrypoint_ignores_non_utf8_live_only_env_vars() {
    let cases = [
        (
            Some(OsString::from_vec(vec![0xff, 0xfe, 0xfd])),
            Option::<OsString>::None,
            Option::<OsString>::None,
        ),
        (
            Option::<OsString>::None,
            Some(OsString::from_vec(vec![0xff, 0xfe, 0xfd])),
            Option::<OsString>::None,
        ),
        (
            Option::<OsString>::None,
            Option::<OsString>::None,
            Some(OsString::from_vec(vec![0xff, 0xfe, 0xfd])),
        ),
    ];

    for (targets, approved, ready) in cases {
        let output = app_live_output_raw_env("paper", targets, approved, ready);
        assert!(
            output.status.success(),
            "paper mode should ignore live-only env vars"
        );
    }
}

#[test]
fn live_entrypoint_boots_without_neg_risk_target_config() {
    let output = app_live_output("live", None);

    assert!(
        !output.status.success(),
        "live mode should fail fast without durable store inputs"
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout should be utf8");
    let stderr = String::from_utf8(output.stderr).expect("stderr should be utf8");
    let combined = format!("{stdout}{stderr}");

    assert!(combined.contains("DATABASE_URL"), "{combined}");
}

#[test]
fn live_entrypoint_persists_operator_target_revision_anchor_during_startup() {
    let database = TestDatabase::new();
    let neg_risk_live_targets = valid_neg_risk_live_targets_json();
    let revision = load_neg_risk_live_targets(Some(neg_risk_live_targets))
        .expect("targets should parse")
        .revision()
        .to_owned();
    let output = app_live_output_with_operator_inputs_and_signer_and_database_url(
        "live",
        Some(neg_risk_live_targets),
        Some("family-a"),
        Some("family-a"),
        Some(valid_local_signer_config_json()),
        Some(database.database_url()),
    );

    assert!(
        output.status.success(),
        "live mode should boot with explicit operator inputs"
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout should be utf8");
    let stderr = String::from_utf8(output.stderr).expect("stderr should be utf8");
    let combined = format!("{stdout}{stderr}");

    assert!(combined.contains("negrisk_mode=Live"), "{combined}");
    assert!(
        combined.contains("neg_risk_live_attempt_count=1"),
        "{combined}"
    );
    assert!(
        combined.contains("neg_risk_live_state_source=\"synthetic_bootstrap\""),
        "{combined}"
    );
    assert!(
        combined.contains("evidence_source=\"bootstrap\""),
        "{combined}"
    );

    let progress = database
        .runtime_progress()
        .expect("startup should persist runtime progress");
    assert_eq!(
        progress.operator_target_revision.as_deref(),
        Some(revision.as_str())
    );
    assert_eq!(progress.last_snapshot_id.as_deref(), Some("snapshot-0"));

    database.cleanup();
}

#[test]
fn live_entrypoint_requires_matching_operator_target_revision_anchor() {
    let database = TestDatabase::new();
    database.seed_durable_live_execution_record(Some("targets-rev-stale"));
    let output = app_live_output_with_operator_inputs_and_signer_and_database_url(
        "live",
        Some(valid_neg_risk_live_targets_json()),
        Some("family-a"),
        Some("family-a"),
        Some(valid_local_signer_config_json()),
        Some(database.database_url()),
    );

    assert!(
        !output.status.success(),
        "live mode should fail closed when operator targets do not match the persisted revision anchor"
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout should be utf8");
    let stderr = String::from_utf8(output.stderr).expect("stderr should be utf8");
    let combined = format!("{stdout}{stderr}");

    assert!(combined.contains("operator target revision"), "{combined}");

    database.cleanup();
}

#[test]
fn live_entrypoint_rejects_missing_operator_target_revision_anchor() {
    let database = TestDatabase::new();
    database.seed_durable_live_execution_record(None);
    let output = app_live_output_with_operator_inputs_and_signer_and_database_url(
        "live",
        Some(valid_neg_risk_live_targets_json()),
        Some("family-a"),
        Some("family-a"),
        Some(valid_local_signer_config_json()),
        Some(database.database_url()),
    );

    assert!(
        !output.status.success(),
        "live mode should fail closed when durable follow-up work exists but the persisted operator target revision anchor is missing"
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout should be utf8");
    let stderr = String::from_utf8(output.stderr).expect("stderr should be utf8");
    let combined = format!("{stdout}{stderr}");

    assert!(
        combined.contains("operator target revision anchor is required"),
        "{combined}"
    );

    database.cleanup();
}

#[test]
fn live_entrypoint_restores_durable_live_state_from_non_empty_store() {
    let database = TestDatabase::new();
    database.seed_durable_live_execution_record(None);
    let output = app_live_output_raw_env_with_signer_and_database_url(
        "live",
        None,
        None,
        None,
        None,
        Some(database.database_url()),
    );
    database.cleanup();

    assert!(
        output.status.success(),
        "live mode should restore durable live truth from a non-empty store"
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout should be utf8");
    let stderr = String::from_utf8(output.stderr).expect("stderr should be utf8");
    let combined = format!("{stdout}{stderr}");

    assert!(combined.contains("negrisk_mode=Live"), "{combined}");
    assert!(
        combined.contains("neg_risk_live_attempt_count=1"),
        "{combined}"
    );
    assert!(
        combined.contains("neg_risk_live_state_source=\"durable_restore\""),
        "{combined}"
    );
    assert!(
        combined.contains("evidence_source=\"snapshot\""),
        "{combined}"
    );
}

#[test]
fn live_entrypoint_requires_database_url_even_when_operator_inputs_request_live_work() {
    let output = app_live_output_raw_env_with_signer_and_database_url(
        "live",
        Some(
            r#"
            [
              {
                "family_id": "family-a",
                "members": [
                  { "condition_id": "condition-1", "token_id": "token-1", "price": "0.43", "quantity": "5" }
                ]
              }
            ]
            "#
            .into(),
        ),
        Some("family-a".into()),
        Some("family-a".into()),
        Some(valid_local_signer_config_json().into()),
        None,
    );

    assert!(
        !output.status.success(),
        "live mode should fail closed without durable store access even when operator inputs request live work"
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout should be utf8");
    let stderr = String::from_utf8(output.stderr).expect("stderr should be utf8");
    let combined = format!("{stdout}{stderr}");

    assert!(combined.contains("DATABASE_URL"), "{combined}");
}

#[test]
fn live_entrypoint_rejects_missing_local_signer_config_when_live_work_is_requested() {
    let output = app_live_output_with_operator_inputs(
        "live",
        Some(
            r#"
            [
              {
                "family_id": "family-a",
                "members": [
                  { "condition_id": "condition-1", "token_id": "token-1", "price": "0.43", "quantity": "5" }
                ]
              }
            ]
            "#,
        ),
        Some("family-a"),
        Some("family-a"),
    );

    assert!(
        !output.status.success(),
        "binary should fail when live neg-risk work is requested without signer config"
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout should be utf8");
    let stderr = String::from_utf8(output.stderr).expect("stderr should be utf8");
    let combined = format!("{stdout}{stderr}");

    assert!(
        combined.contains("missing local signer config"),
        "{combined}"
    );
}

#[test]
fn live_entrypoint_rejects_invalid_local_signer_config_when_live_work_is_requested() {
    let output = app_live_output_raw_env_with_signer(
        "live",
        Some(
            r#"
            [
              {
                "family_id": "family-a",
                "members": [
                  { "condition_id": "condition-1", "token_id": "token-1", "price": "0.43", "quantity": "5" }
                ]
              }
            ]
            "#
            .into(),
        ),
        Some("family-a".into()),
        Some("family-a".into()),
        Some(OsString::from("{")),
        None,
    );

    assert!(
        !output.status.success(),
        "binary should fail when live neg-risk work is requested with invalid signer config"
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout should be utf8");
    let stderr = String::from_utf8(output.stderr).expect("stderr should be utf8");
    let combined = format!("{stdout}{stderr}");

    assert!(
        combined.contains("invalid local signer config"),
        "{combined}"
    );
}

fn app_live_output(app_mode: &str, neg_risk_live_targets: Option<&str>) -> std::process::Output {
    app_live_output_with_operator_inputs(app_mode, neg_risk_live_targets, None, None)
}

fn app_live_output_with_operator_inputs(
    app_mode: &str,
    neg_risk_live_targets: Option<&str>,
    approved_families: Option<&str>,
    ready_families: Option<&str>,
) -> std::process::Output {
    app_live_output_raw_env_with_signer(
        app_mode,
        neg_risk_live_targets.map(OsString::from),
        approved_families.map(OsString::from),
        ready_families.map(OsString::from),
        None,
        None,
    )
}

fn app_live_output_raw_env(
    app_mode: &str,
    neg_risk_live_targets: Option<OsString>,
    approved_families: Option<OsString>,
    ready_families: Option<OsString>,
) -> std::process::Output {
    app_live_output_raw_env_with_signer(
        app_mode,
        neg_risk_live_targets,
        approved_families,
        ready_families,
        None,
        None,
    )
}

fn app_live_output_with_operator_inputs_and_signer_and_database_url(
    app_mode: &str,
    neg_risk_live_targets: Option<&str>,
    approved_families: Option<&str>,
    ready_families: Option<&str>,
    local_signer_config: Option<&str>,
    database_url: Option<&str>,
) -> std::process::Output {
    app_live_output_raw_env_with_signer(
        app_mode,
        neg_risk_live_targets.map(OsString::from),
        approved_families.map(OsString::from),
        ready_families.map(OsString::from),
        local_signer_config.map(OsString::from),
        database_url,
    )
}

fn app_live_output_raw_env_with_signer(
    app_mode: &str,
    neg_risk_live_targets: Option<OsString>,
    approved_families: Option<OsString>,
    ready_families: Option<OsString>,
    local_signer_config: Option<OsString>,
    database_url: Option<&str>,
) -> std::process::Output {
    let needs_database_url = app_mode == "live"
        && (neg_risk_live_targets.is_some()
            || approved_families.is_some()
            || ready_families.is_some()
            || local_signer_config.is_some());
    app_live_output_raw_env_with_signer_and_database_url(
        app_mode,
        neg_risk_live_targets,
        approved_families,
        ready_families,
        local_signer_config,
        database_url.or_else(|| needs_database_url.then_some(default_test_database_url())),
    )
}

fn app_live_output_raw_env_with_signer_and_database_url(
    app_mode: &str,
    neg_risk_live_targets: Option<OsString>,
    approved_families: Option<OsString>,
    ready_families: Option<OsString>,
    local_signer_config: Option<OsString>,
    database_url: Option<&str>,
) -> std::process::Output {
    let mut command = Command::new(app_live_binary());
    command.env("AXIOM_MODE", app_mode);
    command.env_remove("AXIOM_NEG_RISK_LIVE_TARGETS");
    command.env_remove("AXIOM_NEG_RISK_LIVE_APPROVED_FAMILIES");
    command.env_remove("AXIOM_NEG_RISK_LIVE_READY_FAMILIES");
    command.env_remove("AXIOM_LOCAL_SIGNER_CONFIG");
    command.env_remove("DATABASE_URL");
    if let Some(value) = neg_risk_live_targets {
        command.env("AXIOM_NEG_RISK_LIVE_TARGETS", value);
    }
    if let Some(value) = approved_families {
        command.env("AXIOM_NEG_RISK_LIVE_APPROVED_FAMILIES", value);
    }
    if let Some(value) = ready_families {
        command.env("AXIOM_NEG_RISK_LIVE_READY_FAMILIES", value);
    }
    if let Some(value) = local_signer_config {
        command.env("AXIOM_LOCAL_SIGNER_CONFIG", value);
    }
    if let Some(database_url) = database_url {
        command.env("DATABASE_URL", database_url);
    }
    command.output().expect("app-live should run")
}

fn default_test_database_url() -> &'static str {
    "postgres://axiom:axiom@localhost:5432/axiom_arb"
}

struct TestDatabase {
    admin_pool: PgPool,
    pool: PgPool,
    schema: String,
    database_url: String,
}

impl TestDatabase {
    fn new() -> Self {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime should build")
            .block_on(async {
                let admin_database_url = std::env::var("DATABASE_URL")
                    .unwrap_or_else(|_| default_test_database_url().to_owned());
                let admin_pool = PgPoolOptions::new()
                    .max_connections(2)
                    .connect(&admin_database_url)
                    .await
                    .expect("test database should connect");
                let schema = format!(
                    "app_live_main_entrypoint_{}_{}",
                    std::process::id(),
                    NEXT_SCHEMA_ID.fetch_add(1, Ordering::Relaxed)
                );
                let create_schema = format!(r#"CREATE SCHEMA "{schema}""#);
                sqlx::query(&create_schema)
                    .execute(&admin_pool)
                    .await
                    .expect("test schema should create");

                let database_url = schema_scoped_database_url(&admin_database_url, &schema);
                let pool = PgPoolOptions::new()
                    .max_connections(2)
                    .connect(&database_url)
                    .await
                    .expect("schema-scoped test pool should connect");
                run_migrations(&pool)
                    .await
                    .expect("test migrations should run");

                Self {
                    admin_pool,
                    pool,
                    schema,
                    database_url,
                }
            })
    }

    fn database_url(&self) -> &str {
        &self.database_url
    }

    fn seed_durable_live_execution_record(&self, operator_target_revision: Option<&str>) {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime should build")
            .block_on(async {
                self.seed_runtime_progress_async(operator_target_revision)
                    .await;
                ExecutionAttemptRepo
                    .append(
                        &self.pool,
                        &ExecutionAttemptRow {
                            attempt_id: "attempt-live-main-1".to_owned(),
                            plan_id: "request-bound:7:req-1:negrisk-submit-family:family-a"
                                .to_owned(),
                            snapshot_id: "snapshot-7".to_owned(),
                            route: "neg-risk".to_owned(),
                            scope: "family-a".to_owned(),
                            matched_rule_id: Some("family-a-live".to_owned()),
                            execution_mode: ExecutionMode::Live,
                            attempt_no: 1,
                            idempotency_key: "idem-attempt-live-main-1".to_owned(),
                        },
                    )
                    .await
                    .expect("execution attempt should persist");
                LiveSubmissionRepo
                    .append(
                        &self.pool,
                        LiveSubmissionRecordRow {
                            submission_ref: "submission-live-main-1".to_owned(),
                            attempt_id: "attempt-live-main-1".to_owned(),
                            route: "neg-risk".to_owned(),
                            scope: "family-a".to_owned(),
                            provider: "venue-polymarket".to_owned(),
                            state: "submitted".to_owned(),
                            payload: serde_json::json!({
                                "submission_ref": "submission-live-main-1",
                                "family_id": "family-a",
                                "route": "neg-risk",
                                "reason": "submitted_for_execution",
                            }),
                        },
                    )
                    .await
                    .expect("live submission record should persist");
            });
    }

    fn runtime_progress(&self) -> Option<RuntimeProgressRow> {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime should build")
            .block_on(async {
                RuntimeProgressRepo
                    .current(&self.pool)
                    .await
                    .expect("runtime progress lookup should succeed")
            })
    }

    async fn seed_runtime_progress_async(&self, operator_target_revision: Option<&str>) {
        RuntimeProgressRepo
            .record_progress(
                &self.pool,
                41,
                7,
                Some("snapshot-7"),
                operator_target_revision,
            )
            .await
            .expect("runtime progress should persist");
    }

    fn cleanup(self) {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime should build")
            .block_on(async {
                self.pool.close().await;
                let drop_schema = format!(
                    r#"DROP SCHEMA IF EXISTS "{schema}" CASCADE"#,
                    schema = self.schema
                );
                let _ = sqlx::query(&drop_schema).execute(&self.admin_pool).await;
                self.admin_pool.close().await;
            });
    }
}

fn schema_scoped_database_url(base: &str, schema: &str) -> String {
    let options = format!("options=-csearch_path%3D{schema}");
    if base.contains('?') {
        format!("{base}&{options}")
    } else {
        format!("{base}?{options}")
    }
}

fn valid_local_signer_config_json() -> &'static str {
    r#"
    {
      "signer": {
        "address": "0x1111111111111111111111111111111111111111",
        "funder_address": "0x2222222222222222222222222222222222222222",
        "signature_type": "Eoa",
        "wallet_route": "Eoa"
      },
      "l2_auth": {
        "api_key": "poly-api-key-1",
        "passphrase": "poly-passphrase-1",
        "timestamp": "1700000000",
        "signature": "poly-signature-1"
      },
      "relayer_auth": {
        "kind": "builder_api_key",
        "api_key": "builder-api-key-1",
        "timestamp": "1700000001",
        "passphrase": "builder-passphrase-1",
        "signature": "builder-signature-1"
      }
    }
    "#
}

fn valid_neg_risk_live_targets_json() -> &'static str {
    r#"
    [
      {
        "family_id": "family-a",
        "members": [
          { "condition_id": "condition-1", "token_id": "token-1", "price": "0.43", "quantity": "5" }
        ]
      }
    ]
    "#
}

fn app_live_binary() -> PathBuf {
    if let Some(path) = std::env::var_os("CARGO_BIN_EXE_app-live") {
        return PathBuf::from(path);
    }

    let mut path = std::env::current_exe().expect("current test executable path");
    path.pop();
    if path.ends_with("deps") {
        path.pop();
    }
    path.push("app-live");
    if cfg!(windows) {
        path.set_extension("exe");
    }

    path
}
