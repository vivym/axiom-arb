use config_schema::{load_raw_config_from_path, ValidatedConfig};
use observability::span_names;
use persistence::{models::RuntimeProgressRow, run_migrations, RuntimeProgressRepo};
use sqlx::{postgres::PgPoolOptions, PgPool};
use std::{
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT_SCHEMA_ID: AtomicU64 = AtomicU64::new(1);

#[test]
fn binary_entrypoint_reads_paper_mode_from_config_file() {
    let output = app_live_output_with_config("fixtures/app-live-paper.toml");

    assert!(output.status.success());
    assert!(combined(&output).contains("app_mode=paper"));
}

#[test]
fn binary_entrypoint_rejects_missing_config_path() {
    let output = Command::new(app_live_binary()).output().unwrap();

    assert!(!output.status.success());
    assert!(combined(&output).contains("--config"));
}

#[test]
fn paper_mode_still_requires_database_url_after_config_load() {
    let output = app_live_output_with_config_and_database("fixtures/app-live-paper.toml", None);

    assert!(!output.status.success());
    assert!(combined(&output).contains("DATABASE_URL"));
}

#[test]
fn binary_entrypoint_emits_structured_bootstrap_log_from_config_file() {
    let output = app_live_output_with_config("fixtures/app-live-paper.toml");

    assert!(output.status.success());

    let combined = combined(&output);
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
}

#[test]
fn live_config_requires_database_url_after_config_parse() {
    let output = app_live_output_with_config_and_database("fixtures/app-live-live.toml", None);

    assert!(!output.status.success());
    assert!(combined(&output).contains("DATABASE_URL"));
}

#[test]
fn smoke_config_surfaces_validated_config_error_before_database_bootstrap() {
    let output = app_live_output_with_config_and_database("fixtures/app-live-smoke.toml", None);

    assert!(!output.status.success());
    assert!(combined(&output).contains("polymarket.signer"));
}

#[test]
fn live_config_persists_operator_target_revision_anchor_during_startup() {
    let database = TestDatabase::new();
    let revision = app_live::NegRiskLiveTargetSet::try_from(
        &ValidatedConfig::new(
            load_raw_config_from_path(&config_fixture_path("fixtures/app-live-live.toml"))
                .expect("config should parse"),
        )
        .expect("config should validate")
        .for_app_live()
        .expect("live view should validate"),
    )
    .expect("targets should parse")
    .revision()
    .to_owned();

    let output = app_live_output_with_config_and_database(
        "fixtures/app-live-live.toml",
        Some(database.database_url()),
    );

    assert!(output.status.success(), "{}", combined(&output));
    let combined = combined(&output);
    assert!(combined.contains("negrisk_mode=Live"), "{combined}");
    assert!(
        combined.contains("neg_risk_live_attempt_count=1"),
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

fn app_live_output_with_config(config_fixture: &str) -> std::process::Output {
    app_live_output_with_config_and_database(config_fixture, Some(default_test_database_url()))
}

fn app_live_output_with_config_and_database(
    config_fixture: &str,
    database_url: Option<&str>,
) -> std::process::Output {
    let mut command = Command::new(app_live_binary());
    command
        .arg("--config")
        .arg(config_fixture_path(config_fixture));
    command.env_remove("AXIOM_MODE");
    command.env_remove("AXIOM_NEG_RISK_LIVE_TARGETS");
    command.env_remove("AXIOM_NEG_RISK_LIVE_APPROVED_FAMILIES");
    command.env_remove("AXIOM_NEG_RISK_LIVE_READY_FAMILIES");
    command.env_remove("AXIOM_LOCAL_SIGNER_CONFIG");
    command.env_remove("AXIOM_REAL_USER_SHADOW_SMOKE");
    command.env_remove("AXIOM_POLYMARKET_SOURCE_CONFIG");
    command.env_remove("DATABASE_URL");
    if let Some(database_url) = database_url {
        command.env("DATABASE_URL", database_url);
    }
    command.output().expect("app-live should run")
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

fn combined(output: &std::process::Output) -> String {
    let stdout = String::from_utf8(output.stdout.clone()).expect("stdout should be utf8");
    let stderr = String::from_utf8(output.stderr.clone()).expect("stderr should be utf8");
    format!("{stdout}{stderr}")
}

fn config_fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("config-schema")
        .join("tests")
        .join(name)
}

fn default_test_database_url() -> &'static str {
    "postgres://axiom:axiom@localhost:5432/axiom_arb"
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
