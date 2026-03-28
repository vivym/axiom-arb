use std::{
    collections::BTreeSet,
    env,
    sync::atomic::{AtomicU64, Ordering},
    sync::{Mutex, OnceLock},
};

use app_live::{
    load_neg_risk_live_targets, load_real_user_shadow_smoke_config,
    run_live_daemon_from_durable_store_with_neg_risk_live_targets_instrumented, AppDaemon,
    AppInstrumentation, AppSupervisor, InputTaskEvent, StaticSnapshotSource,
};
use chrono::{TimeZone, Utc};
use persistence::{
    models::{
        AdoptableTargetRevisionRow, CandidateAdoptionProvenanceRow, CandidateTargetSetRow,
        ExecutionAttemptRow, LiveSubmissionRecordRow,
    },
    run_migrations, CandidateAdoptionRepo, CandidateArtifactRepo, ExecutionAttemptRepo,
    LiveSubmissionRepo, RuntimeProgressRepo,
};
use serde_json::json;
use sqlx::{postgres::PgPoolOptions, PgPool};

static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
static SCHEMA_COUNTER: AtomicU64 = AtomicU64::new(0);

#[test]
fn daemon_restores_candidate_status_without_blocking_non_adoption_startup() {
    let _guard = lock_env();
    let database = TestDatabase::new();
    database.seed_candidate_restore_state("targets-rev-advisory", true);
    env::set_var("DATABASE_URL", database.database_url());

    let result = run_live_daemon_from_durable_store_with_neg_risk_live_targets_instrumented(
        &StaticSnapshotSource::empty(),
        AppInstrumentation::disabled(),
        app_live::NegRiskLiveTargetSet::empty(),
        BTreeSet::new(),
        BTreeSet::new(),
        None,
    )
    .expect("ordinary startup should not fail closed when candidate artifacts are absent or advisory only");

    assert_eq!(
        result.summary.latest_candidate_revision.as_deref(),
        Some("candidate-9")
    );
    assert_eq!(
        result.summary.latest_adoptable_revision.as_deref(),
        Some("adoptable-9")
    );
    assert_eq!(
        result
            .summary
            .latest_candidate_operator_target_revision
            .as_deref(),
        Some("targets-rev-advisory")
    );
    assert!(result.summary.adoption_provenance_resolved);

    database.cleanup();
}

#[test]
fn explicit_operator_target_restart_succeeds_after_first_startup_without_candidate_provenance() {
    let _guard = lock_env();
    let database = TestDatabase::new();
    env::set_var("DATABASE_URL", database.database_url());

    let first_targets = load_neg_risk_live_targets(Some(valid_neg_risk_live_targets_json()))
        .expect("targets should parse");
    let first = run_live_daemon_from_durable_store_with_neg_risk_live_targets_instrumented(
        &StaticSnapshotSource::empty(),
        AppInstrumentation::disabled(),
        first_targets,
        BTreeSet::new(),
        BTreeSet::new(),
        None,
    )
    .expect("first explicit-operator startup should succeed");
    assert_eq!(first.summary.latest_candidate_revision, None);
    assert!(!first.summary.adoption_provenance_resolved);

    let second_targets = load_neg_risk_live_targets(Some(valid_neg_risk_live_targets_json()))
        .expect("targets should parse");
    database.seed_candidate_artifacts(second_targets.revision(), false);
    let second = run_live_daemon_from_durable_store_with_neg_risk_live_targets_instrumented(
        &StaticSnapshotSource::empty(),
        AppInstrumentation::disabled(),
        second_targets,
        BTreeSet::new(),
        BTreeSet::new(),
        None,
    )
    .expect("matching explicit operator restart should not require candidate provenance");

    assert_eq!(second.summary.latest_candidate_revision, None);
    assert_eq!(second.summary.latest_adoptable_revision, None);
    assert_eq!(
        second.summary.latest_candidate_operator_target_revision,
        None
    );
    assert!(!second.summary.adoption_provenance_resolved);

    database.cleanup();
}

#[test]
fn malformed_candidate_adoption_provenance_fails_closed_on_restore() {
    let _guard = lock_env();
    let database = TestDatabase::new();
    let targets = load_neg_risk_live_targets(Some(valid_neg_risk_live_targets_json()))
        .expect("targets should parse");
    database.seed_candidate_restore_state(targets.revision(), true);
    database.corrupt_candidate_provenance(targets.revision());
    env::set_var("DATABASE_URL", database.database_url());

    let err = run_live_daemon_from_durable_store_with_neg_risk_live_targets_instrumented(
        &StaticSnapshotSource::empty(),
        AppInstrumentation::disabled(),
        targets,
        BTreeSet::new(),
        BTreeSet::new(),
        None,
    )
    .expect_err("startup should fail closed when candidate provenance chain is malformed");

    assert!(
        err.to_string().contains("could not be linked back")
            || err.to_string().contains("candidate adoption provenance"),
        "{err}"
    );

    database.cleanup();
}

#[test]
fn daemon_run_persists_candidate_artifacts_from_candidate_dirty_inputs() {
    let _guard = lock_env();
    let database = TestDatabase::new();
    env::set_var("DATABASE_URL", database.database_url());

    let targets = load_neg_risk_live_targets(Some(valid_neg_risk_live_targets_json()))
        .expect("targets should parse");
    let operator_target_revision = targets.revision().to_owned();
    let mut supervisor = AppSupervisor::for_tests();
    supervisor.seed_neg_risk_live_targets(targets.into_targets());
    supervisor.seed_unapplied_journal_entry(
        18,
        InputTaskEvent::family_discovery_observed(
            18,
            "session-discovery",
            "evt-18",
            "family-a",
            Utc.with_ymd_and_hms(2026, 3, 28, 9, 0, 0).unwrap(),
        ),
    );
    supervisor.seed_unapplied_journal_entry(
        19,
        InputTaskEvent::family_backfill_observed(
            19,
            "session-discovery",
            "evt-19",
            "family-a",
            "cursor-19",
            true,
            Utc.with_ymd_and_hms(2026, 3, 28, 9, 5, 0).unwrap(),
        ),
    );

    let report = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("test runtime")
        .block_on(async {
            AppDaemon::for_tests(supervisor)
                .run_until_idle_for_tests(3)
                .await
        })
        .expect("daemon run should process candidate dirty inputs");
    let summary = report.summary;

    assert_eq!(
        summary.latest_candidate_revision.as_deref(),
        Some("candidate-pub-2")
    );
    assert_eq!(
        summary.latest_adoptable_revision.as_deref(),
        Some("adoptable-candidate-pub-2")
    );
    assert_eq!(
        summary.latest_candidate_operator_target_revision.as_deref(),
        Some(operator_target_revision.as_str())
    );
    assert!(summary.adoption_provenance_resolved);

    let candidate = database
        .load_candidate_target_set("candidate-pub-2")
        .expect("candidate row lookup should succeed")
        .expect("candidate row should persist");
    assert_eq!(candidate.snapshot_id, "candidate-pub-2");

    let adoptable = database
        .load_adoptable_target_revision("adoptable-candidate-pub-2")
        .expect("adoptable row lookup should succeed")
        .expect("adoptable row should persist");
    assert_eq!(adoptable.candidate_revision, "candidate-pub-2");
    assert_eq!(
        adoptable.rendered_operator_target_revision,
        operator_target_revision
    );

    let provenance = database
        .load_candidate_provenance(&operator_target_revision)
        .expect("provenance lookup should succeed")
        .expect("provenance row should persist");
    assert_eq!(provenance.candidate_revision, "candidate-pub-2");
    assert_eq!(provenance.adoptable_revision, "adoptable-candidate-pub-2");

    database.cleanup();
}

#[test]
fn smoke_enabled_daemon_persists_shadow_rows_to_durable_store() {
    let _guard = lock_env();
    let database = TestDatabase::new();
    env::set_var("DATABASE_URL", database.database_url());

    let targets = load_neg_risk_live_targets(Some(valid_neg_risk_live_targets_json()))
        .expect("targets should parse");
    let smoke =
        load_real_user_shadow_smoke_config(Some("1"), Some(valid_polymarket_source_config_json()))
            .expect("smoke config should parse");

    let result = run_live_daemon_from_durable_store_with_neg_risk_live_targets_instrumented(
        &StaticSnapshotSource::empty(),
        AppInstrumentation::disabled(),
        targets,
        BTreeSet::from(["family-a".to_owned()]),
        BTreeSet::from(["family-a".to_owned()]),
        smoke,
    )
    .expect("smoke daemon startup should succeed");

    assert_eq!(result.summary.negrisk_mode, domain::ExecutionMode::Shadow);
    assert_eq!(result.summary.neg_risk_live_attempt_count, 0);

    let shadow_attempt = database
        .load_single_shadow_attempt()
        .expect("shadow attempt query should succeed")
        .expect("shadow attempt should persist");
    assert_eq!(shadow_attempt.0, "neg-risk");
    assert_eq!(shadow_attempt.1, "family-a");
    assert_eq!(shadow_attempt.2, "shadow");

    let artifact = database
        .load_single_shadow_artifact()
        .expect("shadow artifact query should succeed")
        .expect("shadow artifact should persist");
    assert_eq!(artifact.0, "neg-risk-shadow-plan");
    assert_eq!(artifact.1, shadow_attempt.3);

    assert_eq!(
        database
            .live_attempt_count()
            .expect("live count query should succeed"),
        0
    );

    database.cleanup();
}

#[test]
fn smoke_enabled_daemon_fails_closed_when_durable_live_rows_exist() {
    let _guard = lock_env();
    let database = TestDatabase::new();
    env::set_var("DATABASE_URL", database.database_url());

    let targets = load_neg_risk_live_targets(Some(valid_neg_risk_live_targets_json()))
        .expect("targets should parse");
    database.seed_durable_live_execution_state(targets.revision());
    let smoke =
        load_real_user_shadow_smoke_config(Some("1"), Some(valid_polymarket_source_config_json()))
            .expect("smoke config should parse");

    let err = run_live_daemon_from_durable_store_with_neg_risk_live_targets_instrumented(
        &StaticSnapshotSource::empty(),
        AppInstrumentation::disabled(),
        targets,
        BTreeSet::from(["family-a".to_owned()]),
        BTreeSet::from(["family-a".to_owned()]),
        smoke,
    )
    .expect_err("smoke startup should fail closed when durable live rows already exist");

    assert!(
        err.to_string()
            .contains("real-user shadow smoke cannot resume with durable live execution records"),
        "{err}"
    );

    database.cleanup();
}

struct TestDatabase {
    pool: PgPool,
    database_url: String,
}

impl TestDatabase {
    fn new() -> Self {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime should build")
            .block_on(async {
                let database_url = env::var("TEST_DATABASE_URL")
                    .or_else(|_| env::var("DATABASE_URL"))
                    .unwrap_or_else(|_| default_database_url_for_tests().to_owned());
                let admin_pool = PgPoolOptions::new()
                    .max_connections(1)
                    .connect(&database_url)
                    .await
                    .expect("admin pool should connect");
                let schema = format!(
                    "app_live_candidate_daemon_{}_{}",
                    std::process::id(),
                    SCHEMA_COUNTER.fetch_add(1, Ordering::Relaxed)
                );
                sqlx::query(&format!(r#"DROP SCHEMA IF EXISTS "{schema}" CASCADE"#))
                    .execute(&admin_pool)
                    .await
                    .expect("drop schema");
                sqlx::query(&format!(r#"CREATE SCHEMA "{schema}""#))
                    .execute(&admin_pool)
                    .await
                    .expect("create schema");
                let scoped_url = schema_scoped_database_url(&database_url, &schema);
                let pool = PgPoolOptions::new()
                    .max_connections(1)
                    .connect(&scoped_url)
                    .await
                    .expect("pool should connect");
                run_migrations(&pool).await.expect("migrations should run");
                Self {
                    pool,
                    database_url: scoped_url,
                }
            })
    }

    fn database_url(&self) -> &str {
        &self.database_url
    }

    fn seed_candidate_restore_state(
        &self,
        rendered_operator_target_revision: &str,
        persist_provenance: bool,
    ) {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime should build")
            .block_on(async {
                RuntimeProgressRepo
                    .record_progress(
                        &self.pool,
                        41,
                        7,
                        Some("snapshot-7"),
                        Some(rendered_operator_target_revision),
                    )
                    .await
                    .expect("runtime progress should persist");
            });
        self.seed_candidate_artifacts(rendered_operator_target_revision, persist_provenance);
    }

    fn seed_candidate_artifacts(
        &self,
        rendered_operator_target_revision: &str,
        persist_provenance: bool,
    ) {
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
                            candidate_revision: "candidate-9".to_owned(),
                            snapshot_id: "snapshot-9".to_owned(),
                            source_revision: "discovery-9".to_owned(),
                            payload: json!({
                                "candidate_revision": "candidate-9",
                                "snapshot_id": "snapshot-9",
                            }),
                        },
                    )
                    .await
                    .expect("candidate row should persist");

                artifacts
                    .upsert_adoptable_target_revision(
                        &self.pool,
                        &AdoptableTargetRevisionRow {
                            adoptable_revision: "adoptable-9".to_owned(),
                            candidate_revision: "candidate-9".to_owned(),
                            rendered_operator_target_revision: rendered_operator_target_revision
                                .to_owned(),
                            payload: json!({
                                "adoptable_revision": "adoptable-9",
                                "candidate_revision": "candidate-9",
                                "rendered_operator_target_revision": rendered_operator_target_revision,
                            }),
                        },
                    )
                    .await
                    .expect("adoptable row should persist");

                if persist_provenance {
                    CandidateAdoptionRepo
                        .upsert_provenance(
                            &self.pool,
                            &CandidateAdoptionProvenanceRow {
                                operator_target_revision: rendered_operator_target_revision
                                    .to_owned(),
                                adoptable_revision: "adoptable-9".to_owned(),
                                candidate_revision: "candidate-9".to_owned(),
                            },
                        )
                        .await
                        .expect("provenance should persist");
                }
            });
    }

    fn corrupt_candidate_provenance(&self, operator_target_revision: &str) {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime should build")
            .block_on(async {
                sqlx::query(
                    r#"
                    UPDATE adoptable_target_revisions
                    SET rendered_operator_target_revision = 'targets-rev-mismatch'
                    WHERE rendered_operator_target_revision = $1
                    "#,
                )
                .bind(operator_target_revision)
                .execute(&self.pool)
                .await
                .expect("candidate provenance should be corruptible for fail-closed restore test");
            });
    }

    fn load_candidate_target_set(
        &self,
        candidate_revision: &str,
    ) -> persistence::Result<Option<CandidateTargetSetRow>> {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime should build")
            .block_on(async {
                CandidateArtifactRepo
                    .get_candidate_target_set(&self.pool, candidate_revision)
                    .await
            })
    }

    fn load_adoptable_target_revision(
        &self,
        adoptable_revision: &str,
    ) -> persistence::Result<Option<AdoptableTargetRevisionRow>> {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime should build")
            .block_on(async {
                CandidateArtifactRepo
                    .get_adoptable_target_revision(&self.pool, adoptable_revision)
                    .await
            })
    }

    fn load_candidate_provenance(
        &self,
        operator_target_revision: &str,
    ) -> persistence::Result<Option<CandidateAdoptionProvenanceRow>> {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime should build")
            .block_on(async {
                CandidateAdoptionRepo
                    .get_by_operator_target_revision(&self.pool, operator_target_revision)
                    .await
            })
    }

    fn cleanup(self) {
        let _ = self;
    }

    fn seed_durable_live_execution_state(&self, operator_target_revision: &str) {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime should build")
            .block_on(async {
                RuntimeProgressRepo
                    .record_progress(
                        &self.pool,
                        7,
                        7,
                        Some("snapshot-7"),
                        Some(operator_target_revision),
                    )
                    .await
                    .expect("runtime progress should persist");

                let attempt = ExecutionAttemptRow {
                    attempt_id: "attempt-live-1".to_owned(),
                    plan_id: "negrisk-submit-family:family-a".to_owned(),
                    snapshot_id: "snapshot-7".to_owned(),
                    route: "neg-risk".to_owned(),
                    scope: "family-a".to_owned(),
                    matched_rule_id: Some("family-a-live".to_owned()),
                    execution_mode: domain::ExecutionMode::Live,
                    attempt_no: 1,
                    idempotency_key: "idem-attempt-live-1".to_owned(),
                };
                ExecutionAttemptRepo
                    .append(&self.pool, &attempt)
                    .await
                    .expect("live attempt should persist");

                LiveSubmissionRepo
                    .append(
                        &self.pool,
                        LiveSubmissionRecordRow {
                            submission_ref: "submission-live-1".to_owned(),
                            attempt_id: "attempt-live-1".to_owned(),
                            route: "neg-risk".to_owned(),
                            scope: "family-a".to_owned(),
                            provider: "venue-polymarket".to_owned(),
                            state: "submitted".to_owned(),
                            payload: json!({
                                "submission_ref": "submission-live-1",
                                "family_id": "family-a",
                                "route": "neg-risk",
                                "reason": "submitted_for_execution",
                            }),
                        },
                    )
                    .await
                    .expect("live submission should persist");
            });
    }

    fn load_single_shadow_attempt(
        &self,
    ) -> persistence::Result<Option<(String, String, String, String)>> {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime should build")
            .block_on(async {
                sqlx::query_as::<_, (String, String, String, String)>(
                    r#"
                    SELECT route, scope, execution_mode, attempt_id
                    FROM execution_attempts
                    WHERE execution_mode = 'shadow'
                    ORDER BY created_at, attempt_id
                    LIMIT 1
                    "#,
                )
                .fetch_optional(&self.pool)
                .await
                .map_err(Into::into)
            })
    }

    fn load_single_shadow_artifact(&self) -> persistence::Result<Option<(String, String)>> {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime should build")
            .block_on(async {
                sqlx::query_as::<_, (String, String)>(
                    r#"
                    SELECT stream, attempt_id
                    FROM shadow_execution_artifacts
                    ORDER BY attempt_id, stream
                    LIMIT 1
                    "#,
                )
                .fetch_optional(&self.pool)
                .await
                .map_err(Into::into)
            })
    }

    fn live_attempt_count(&self) -> persistence::Result<i64> {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime should build")
            .block_on(async {
                sqlx::query_scalar::<_, i64>(
                    r#"
                    SELECT COUNT(*)
                    FROM execution_attempts
                    WHERE execution_mode = 'live'
                    "#,
                )
                .fetch_one(&self.pool)
                .await
                .map_err(Into::into)
            })
    }
}

fn lock_env() -> std::sync::MutexGuard<'static, ()> {
    match ENV_LOCK.get_or_init(|| Mutex::new(())).lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
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

fn valid_neg_risk_live_targets_json() -> &'static str {
    r#"
    [
      {
        "family_id": "family-a",
        "members": [
          {
            "condition_id": "condition-a",
            "token_id": "token-a",
            "price": "0.42",
            "quantity": "5"
          }
        ]
      }
    ]
    "#
}

fn default_database_url_for_tests() -> &'static str {
    "postgres://axiom:axiom@localhost:5432/axiom_arb"
}

fn valid_polymarket_source_config_json() -> &'static str {
    r#"
    {
      "clob_host": "https://clob.polymarket.com",
      "data_api_host": "https://data-api.polymarket.com",
      "relayer_host": "https://relayer-v2.polymarket.com",
      "market_ws_url": "wss://ws-subscriptions-clob.polymarket.com/ws/market",
      "user_ws_url": "wss://ws-subscriptions-clob.polymarket.com/ws/user",
      "heartbeat_interval_seconds": 15,
      "relayer_poll_interval_seconds": 5,
      "metadata_refresh_interval_seconds": 60
    }
    "#
}
