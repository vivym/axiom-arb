use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

use persistence::{
    models::{
        AdoptableTargetRevisionRow, CandidateAdoptionProvenanceRow, CandidateTargetSetRow,
        OperatorTargetAdoptionHistoryRow,
    },
    run_migrations, CandidateAdoptionRepo, CandidateArtifactRepo,
    OperatorTargetAdoptionHistoryRepo, RuntimeProgressRepo,
};
use serde_json::json;
use sqlx::{postgres::PgPoolOptions, PgPool};

static NEXT_SCHEMA_ID: AtomicU64 = AtomicU64::new(1);

const MINIMAL_LIVE_CONFIG: &str = r#"
[runtime]
mode = "live"

[negrisk.target_source]
source = "adopted"
operator_target_revision = "targets-rev-9"
"#;

const EXPLICIT_TARGET_LIVE_CONFIG: &str = r#"
[runtime]
mode = "live"
real_user_shadow_smoke = false

[polymarket.source]
clob_host = "https://clob.polymarket.com"
data_api_host = "https://data-api.polymarket.com"
relayer_host = "https://relayer-v2.polymarket.com"
market_ws_url = "wss://ws-subscriptions-clob.polymarket.com/ws/market"
user_ws_url = "wss://ws-subscriptions-clob.polymarket.com/ws/user"
heartbeat_interval_seconds = 15
relayer_poll_interval_seconds = 5
metadata_refresh_interval_seconds = 60

[polymarket.signer]
address = "0x1111111111111111111111111111111111111111"
funder_address = "0x2222222222222222222222222222222222222222"
signature_type = "eoa"
wallet_route = "eoa"
api_key = "poly-api-key-1"
passphrase = "poly-passphrase-1"
timestamp = "1700000000"
signature = "poly-signature-1"

[polymarket.relayer_auth]
kind = "builder_api_key"
api_key = "builder-api-key-1"
timestamp = "1700000001"
passphrase = "builder-passphrase-1"
signature = "builder-signature-1"

[negrisk.rollout]
approved_families = ["family-a"]
ready_families = ["family-a"]

[[negrisk.targets]]
family_id = "family-a"

[[negrisk.targets.members]]
condition_id = "condition-1"
token_id = "token-1"
price = "0.43"
quantity = "5"
"#;

const FULL_LIVE_CONFIG: &str = r#"
[runtime]
mode = "live"
real_user_shadow_smoke = false

[polymarket.source]
clob_host = "https://clob.polymarket.com"
data_api_host = "https://data-api.polymarket.com"
relayer_host = "https://relayer-v2.polymarket.com"
market_ws_url = "wss://ws-subscriptions-clob.polymarket.com/ws/market"
user_ws_url = "wss://ws-subscriptions-clob.polymarket.com/ws/user"
heartbeat_interval_seconds = 15
relayer_poll_interval_seconds = 5
metadata_refresh_interval_seconds = 60

[polymarket.account]
address = "0x1111111111111111111111111111111111111111"
funder_address = "0x2222222222222222222222222222222222222222"
signature_type = "eoa"
wallet_route = "eoa"
api_key = "poly-api-key"
secret = "poly-secret"
passphrase = "poly-passphrase"

[polymarket.relayer_auth]
kind = "builder_api_key"
api_key = "builder-api-key"
timestamp = "1700000001"
passphrase = "builder-passphrase"
signature = "builder-signature"

[negrisk.target_source]
source = "adopted"
operator_target_revision = "targets-rev-9"
"#;

#[test]
fn targets_status_reports_configured_revision_and_unavailable_active_state() {
    let database = TestDatabase::new();
    database.seed_targets_catalog();
    let config = temp_config(MINIMAL_LIVE_CONFIG);

    let output = Command::new(app_live_binary())
        .arg("targets")
        .arg("status")
        .arg("--config")
        .arg(&config)
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("app-live targets status should execute");

    let text = combined(&output);
    assert!(output.status.success(), "{text}");
    assert!(
        text.contains("configured_operator_target_revision = targets-rev-9"),
        "{text}"
    );
    assert!(
        text.contains("active_operator_target_revision = unavailable"),
        "{text}"
    );

    database.cleanup();
    let _ = fs::remove_file(config);
}

#[test]
fn targets_candidates_labels_advisory_adoptable_and_adopted_rows() {
    let database = TestDatabase::new();
    database.seed_targets_catalog();
    let config = temp_config(MINIMAL_LIVE_CONFIG);

    let output = Command::new(app_live_binary())
        .arg("targets")
        .arg("candidates")
        .arg("--config")
        .arg(&config)
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("app-live targets candidates should execute");

    let text = combined(&output);
    assert!(output.status.success(), "{text}");
    assert!(text.contains("advisory"), "{text}");
    assert!(text.contains("candidate-8"), "{text}");
    assert!(text.contains("adoptable"), "{text}");
    assert!(text.contains("adoptable-9"), "{text}");
    assert!(text.contains("adopted"), "{text}");
    assert!(text.contains("targets-rev-9"), "{text}");

    database.cleanup();
    let _ = fs::remove_file(config);
}

#[test]
fn targets_candidates_prints_recommended_adoptable_and_non_adoptable_summary() {
    let database = TestDatabase::new();
    database.seed_targets_catalog();
    let config = temp_config(MINIMAL_LIVE_CONFIG);

    let output = Command::new(app_live_binary())
        .arg("targets")
        .arg("candidates")
        .arg("--config")
        .arg(&config)
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("app-live targets candidates should execute");

    let text = combined(&output);
    assert!(output.status.success(), "{text}");
    assert!(
        text.contains(
            "summary advisory_candidate_count = 1 adoptable_revision_count = 1 deferred_target_count = 1 excluded_target_count = 0"
        ),
        "{text}"
    );
    assert!(
        text.contains("recommended_adoptable_revision = adoptable-9"),
        "{text}"
    );
    assert!(
        text.contains(
            "non_adoptable_reason = candidate generation deferred until discovery backfill completes"
        ),
        "{text}"
    );

    database.cleanup();
    let _ = fs::remove_file(config);
}

#[test]
fn targets_status_fails_when_configured_revision_has_no_durable_provenance() {
    let database = TestDatabase::new();
    let config = temp_config(MINIMAL_LIVE_CONFIG);

    let output = Command::new(app_live_binary())
        .arg("targets")
        .arg("status")
        .arg("--config")
        .arg(&config)
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("app-live targets status should execute");

    let text = combined(&output);
    assert!(!output.status.success(), "{text}");
    assert!(!text.contains("adopted operator_target_revision"), "{text}");

    database.cleanup();
    let _ = fs::remove_file(config);
}

#[test]
fn targets_status_fails_when_runtime_progress_row_lacks_operator_target_revision_anchor() {
    let database = TestDatabase::new();
    database.seed_targets_catalog_with_runtime_progress_without_anchor();
    let config = temp_config(MINIMAL_LIVE_CONFIG);

    let output = Command::new(app_live_binary())
        .arg("targets")
        .arg("status")
        .arg("--config")
        .arg(&config)
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("app-live targets status should execute");

    let text = combined(&output);
    assert!(!output.status.success(), "{text}");
    assert!(
        !text.contains("active_operator_target_revision = unavailable"),
        "{text}"
    );
    assert!(!text.contains("restart_needed = unknown"), "{text}");

    database.cleanup();
    let _ = fs::remove_file(config);
}

#[test]
fn targets_status_reports_restart_required_when_active_runtime_progress_revision_has_no_durable_provenance(
) {
    let database = TestDatabase::new();
    database.seed_targets_catalog_with_unprovenanced_active_revision();
    let config = temp_config(MINIMAL_LIVE_CONFIG);

    let output = Command::new(app_live_binary())
        .arg("targets")
        .arg("status")
        .arg("--config")
        .arg(&config)
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("app-live targets status should execute");

    let text = combined(&output);
    assert!(output.status.success(), "{text}");
    assert!(
        text.contains("configured_operator_target_revision = targets-rev-9"),
        "{text}"
    );
    assert!(
        text.contains("active_operator_target_revision = targets-rev-10"),
        "{text}"
    );
    assert!(text.contains("restart_needed = true"), "{text}");

    database.cleanup();
    let _ = fs::remove_file(config);
}

#[test]
fn doctor_fails_when_runtime_progress_row_lacks_operator_target_revision_anchor() {
    let database = TestDatabase::new();
    database.seed_targets_catalog_with_runtime_progress_without_anchor();
    let config = temp_config(FULL_LIVE_CONFIG);

    let output = Command::new(app_live_binary())
        .arg("doctor")
        .arg("--config")
        .arg(&config)
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("app-live doctor should execute");

    let text = combined(&output);
    assert!(!output.status.success(), "{text}");
    assert!(text.contains("TargetSourceError"), "{text}");
    assert!(
        text.contains("runtime progress row exists without operator_target_revision anchor"),
        "{text}"
    );

    database.cleanup();
    let _ = fs::remove_file(config);
}

#[test]
fn doctor_reports_explicit_targets_with_local_resolution_and_control_plane_skip() {
    let database = TestDatabase::new();
    database.seed_targets_catalog_with_unprovenanced_active_revision();
    let config = temp_config(EXPLICIT_TARGET_LIVE_CONFIG);

    let output = Command::new(app_live_binary())
        .arg("doctor")
        .arg("--config")
        .arg(&config)
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("app-live doctor should execute");

    let text = combined(&output);
    assert!(
        text.contains("[OK] startup target resolution succeeded"),
        "{text}"
    );
    assert!(
        text.contains("[SKIP] control-plane checks not required for explicit targets"),
        "{text}"
    );
    assert!(!text.contains("TargetSourceError"), "{text}");
    assert!(!text.contains("restart required"), "{text}");
    assert!(!text.contains("runtime progress"), "{text}");

    database.cleanup();
    let _ = fs::remove_file(config);
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
                    .max_connections(8)
                    .connect(&admin_database_url)
                    .await
                    .expect("test database should connect");
                let schema = format!(
                    "app_live_targets_read_{}_{}",
                    std::process::id(),
                    NEXT_SCHEMA_ID.fetch_add(1, Ordering::Relaxed)
                );
                sqlx::query(&format!(r#"CREATE SCHEMA "{schema}""#))
                    .execute(&admin_pool)
                    .await
                    .expect("test schema should create");

                let database_url = schema_scoped_database_url(&admin_database_url, &schema);
                let pool = PgPoolOptions::new()
                    .max_connections(8)
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

    fn seed_targets_catalog(&self) {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime should build")
            .block_on(async {
                let artifacts = CandidateArtifactRepo;

                artifacts
                    .upsert_candidate_target_set(
                        &self.pool,
                        &CandidateTargetSetRow {
                            candidate_revision: "candidate-8".to_owned(),
                            snapshot_id: "snapshot-8".to_owned(),
                            source_revision: "discovery-8".to_owned(),
                            payload: json!({
                                "candidate_revision": "candidate-8",
                                "targets": [
                                    {
                                        "target_id": "candidate-target-8",
                                        "family_id": "family-a",
                                        "validation": {
                                            "status": "deferred",
                                            "reason": "candidate generation deferred until discovery backfill completes"
                                        }
                                    }
                                ]
                            }),
                        },
                    )
                    .await
                    .expect("advisory candidate should seed");

                artifacts
                    .upsert_candidate_target_set(
                        &self.pool,
                        &CandidateTargetSetRow {
                            candidate_revision: "candidate-9".to_owned(),
                            snapshot_id: "snapshot-9".to_owned(),
                            source_revision: "discovery-9".to_owned(),
                            payload: json!({
                                "candidate_revision": "candidate-9",
                            }),
                        },
                    )
                    .await
                    .expect("candidate should seed");

                artifacts
                    .upsert_adoptable_target_revision(
                        &self.pool,
                        &AdoptableTargetRevisionRow {
                            adoptable_revision: "adoptable-9".to_owned(),
                            candidate_revision: "candidate-9".to_owned(),
                            rendered_operator_target_revision: "targets-rev-9".to_owned(),
                            payload: json!({
                                "adoptable_revision": "adoptable-9",
                                "candidate_revision": "candidate-9",
                                "rendered_operator_target_revision": "targets-rev-9",
                                "rendered_live_targets": sample_rendered_live_targets_json(),
                            }),
                        },
                    )
                    .await
                    .expect("adoptable row should seed");

                CandidateAdoptionRepo
                    .upsert_provenance(
                        &self.pool,
                        &CandidateAdoptionProvenanceRow {
                            operator_target_revision: "targets-rev-9".to_owned(),
                            adoptable_revision: "adoptable-9".to_owned(),
                            candidate_revision: "candidate-9".to_owned(),
                        },
                    )
                    .await
                    .expect("adoption provenance should seed");

                OperatorTargetAdoptionHistoryRepo
                    .append(
                        &self.pool,
                        &OperatorTargetAdoptionHistoryRow {
                            adoption_id: "adoption-9".to_owned(),
                            action_kind: "adopt".to_owned(),
                            operator_target_revision: "targets-rev-9".to_owned(),
                            previous_operator_target_revision: Some("targets-rev-7".to_owned()),
                            adoptable_revision: Some("adoptable-9".to_owned()),
                            candidate_revision: Some("candidate-9".to_owned()),
                            adopted_at: chrono::Utc::now(),
                        },
                    )
                    .await
                    .expect("adoption history should seed");
            });
    }

    fn seed_targets_catalog_with_runtime_progress_without_anchor(&self) {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime should build")
            .block_on(async {
                let artifacts = CandidateArtifactRepo;

                artifacts
                    .upsert_candidate_target_set(
                        &self.pool,
                        &CandidateTargetSetRow {
                            candidate_revision: "candidate-8".to_owned(),
                            snapshot_id: "snapshot-8".to_owned(),
                            source_revision: "discovery-8".to_owned(),
                            payload: json!({
                                "candidate_revision": "candidate-8",
                            }),
                        },
                    )
                    .await
                    .expect("advisory candidate should seed");

                artifacts
                    .upsert_candidate_target_set(
                        &self.pool,
                        &CandidateTargetSetRow {
                            candidate_revision: "candidate-9".to_owned(),
                            snapshot_id: "snapshot-9".to_owned(),
                            source_revision: "discovery-9".to_owned(),
                            payload: json!({
                                "candidate_revision": "candidate-9",
                            }),
                        },
                    )
                    .await
                    .expect("candidate should seed");

                artifacts
                    .upsert_adoptable_target_revision(
                        &self.pool,
                        &AdoptableTargetRevisionRow {
                            adoptable_revision: "adoptable-9".to_owned(),
                            candidate_revision: "candidate-9".to_owned(),
                            rendered_operator_target_revision: "targets-rev-9".to_owned(),
                            payload: json!({
                                "adoptable_revision": "adoptable-9",
                                "candidate_revision": "candidate-9",
                                "rendered_operator_target_revision": "targets-rev-9",
                                "rendered_live_targets": sample_rendered_live_targets_json(),
                            }),
                        },
                    )
                    .await
                    .expect("adoptable row should seed");

                CandidateAdoptionRepo
                    .upsert_provenance(
                        &self.pool,
                        &CandidateAdoptionProvenanceRow {
                            operator_target_revision: "targets-rev-9".to_owned(),
                            adoptable_revision: "adoptable-9".to_owned(),
                            candidate_revision: "candidate-9".to_owned(),
                        },
                    )
                    .await
                    .expect("adoption provenance should seed");

                OperatorTargetAdoptionHistoryRepo
                    .append(
                        &self.pool,
                        &OperatorTargetAdoptionHistoryRow {
                            adoption_id: "adoption-9".to_owned(),
                            action_kind: "adopt".to_owned(),
                            operator_target_revision: "targets-rev-9".to_owned(),
                            previous_operator_target_revision: Some("targets-rev-7".to_owned()),
                            adoptable_revision: Some("adoptable-9".to_owned()),
                            candidate_revision: Some("candidate-9".to_owned()),
                            adopted_at: chrono::Utc::now(),
                        },
                    )
                    .await
                    .expect("adoption history should seed");

                RuntimeProgressRepo
                    .record_progress(&self.pool, 41, 7, Some("snapshot-7"), None, None)
                    .await
                    .expect("runtime progress should seed without anchor");
            });
    }

    fn seed_targets_catalog_with_unprovenanced_active_revision(&self) {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime should build")
            .block_on(async {
                let artifacts = CandidateArtifactRepo;

                artifacts
                    .upsert_candidate_target_set(
                        &self.pool,
                        &CandidateTargetSetRow {
                            candidate_revision: "candidate-8".to_owned(),
                            snapshot_id: "snapshot-8".to_owned(),
                            source_revision: "discovery-8".to_owned(),
                            payload: json!({
                                "candidate_revision": "candidate-8",
                            }),
                        },
                    )
                    .await
                    .expect("advisory candidate should seed");

                artifacts
                    .upsert_candidate_target_set(
                        &self.pool,
                        &CandidateTargetSetRow {
                            candidate_revision: "candidate-9".to_owned(),
                            snapshot_id: "snapshot-9".to_owned(),
                            source_revision: "discovery-9".to_owned(),
                            payload: json!({
                                "candidate_revision": "candidate-9",
                            }),
                        },
                    )
                    .await
                    .expect("candidate should seed");

                artifacts
                    .upsert_adoptable_target_revision(
                        &self.pool,
                        &AdoptableTargetRevisionRow {
                            adoptable_revision: "adoptable-9".to_owned(),
                            candidate_revision: "candidate-9".to_owned(),
                            rendered_operator_target_revision: "targets-rev-9".to_owned(),
                            payload: json!({
                                "adoptable_revision": "adoptable-9",
                                "candidate_revision": "candidate-9",
                                "rendered_operator_target_revision": "targets-rev-9",
                                "rendered_live_targets": sample_rendered_live_targets_json(),
                            }),
                        },
                    )
                    .await
                    .expect("adoptable row should seed");

                CandidateAdoptionRepo
                    .upsert_provenance(
                        &self.pool,
                        &CandidateAdoptionProvenanceRow {
                            operator_target_revision: "targets-rev-9".to_owned(),
                            adoptable_revision: "adoptable-9".to_owned(),
                            candidate_revision: "candidate-9".to_owned(),
                        },
                    )
                    .await
                    .expect("adoption provenance should seed");

                OperatorTargetAdoptionHistoryRepo
                    .append(
                        &self.pool,
                        &OperatorTargetAdoptionHistoryRow {
                            adoption_id: "adoption-9".to_owned(),
                            action_kind: "adopt".to_owned(),
                            operator_target_revision: "targets-rev-9".to_owned(),
                            previous_operator_target_revision: Some("targets-rev-7".to_owned()),
                            adoptable_revision: Some("adoptable-9".to_owned()),
                            candidate_revision: Some("candidate-9".to_owned()),
                            adopted_at: chrono::Utc::now(),
                        },
                    )
                    .await
                    .expect("adoption history should seed");

                RuntimeProgressRepo
                    .record_progress(
                        &self.pool,
                        41,
                        7,
                        Some("snapshot-7"),
                        Some("targets-rev-10"),
                        None,
                    )
                    .await
                    .expect("runtime progress should seed with unprovenanced active revision");
            });
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

fn temp_config(contents: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "app-live-targets-read-{}-{}.toml",
        std::process::id(),
        NEXT_SCHEMA_ID.fetch_add(1, Ordering::Relaxed)
    ));
    fs::write(&path, contents).expect("temporary config should write");
    path
}

fn schema_scoped_database_url(base: &str, schema: &str) -> String {
    let options = format!("options=-csearch_path%3D{schema}");
    if base.contains('?') {
        format!("{base}&{options}")
    } else {
        format!("{base}?{options}")
    }
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

fn default_test_database_url() -> &'static str {
    "postgres://axiom:axiom@localhost:5432/axiom_arb"
}

fn combined(output: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn sample_rendered_live_targets_json() -> serde_json::Value {
    json!({
        "family-a": {
            "family_id": "family-a",
            "members": [
                {
                    "condition_id": "condition-1",
                    "token_id": "token-1",
                    "price": "0.43",
                    "quantity": "5"
                }
            ]
        }
    })
}
