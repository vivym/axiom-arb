use std::{
    collections::BTreeSet,
    env,
    sync::atomic::{AtomicU64, Ordering},
    sync::{Mutex, OnceLock},
};

use app_live::{
    load_neg_risk_live_targets,
    run_live_daemon_from_durable_store_with_neg_risk_live_targets_instrumented, AppInstrumentation,
    AppSupervisor, InputTaskEvent, StaticSnapshotSource,
};
use chrono::{TimeZone, Utc};
use persistence::{
    models::{AdoptableTargetRevisionRow, CandidateAdoptionProvenanceRow, CandidateTargetSetRow},
    run_migrations, CandidateAdoptionRepo, CandidateArtifactRepo, RuntimeProgressRepo,
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
    )
    .expect("first explicit-operator startup should succeed");
    assert_eq!(first.summary.latest_candidate_revision, None);
    assert!(!first.summary.adoption_provenance_resolved);

    let second_targets = load_neg_risk_live_targets(Some(valid_neg_risk_live_targets_json()))
        .expect("targets should parse");
    let second = run_live_daemon_from_durable_store_with_neg_risk_live_targets_instrumented(
        &StaticSnapshotSource::empty(),
        AppInstrumentation::disabled(),
        second_targets,
        BTreeSet::new(),
        BTreeSet::new(),
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
fn candidate_derived_operator_target_revision_requires_provenance_on_restore() {
    let _guard = lock_env();
    let database = TestDatabase::new();
    let targets = load_neg_risk_live_targets(Some(valid_neg_risk_live_targets_json()))
        .expect("targets should parse");
    database.seed_candidate_restore_state(targets.revision(), false);
    env::set_var("DATABASE_URL", database.database_url());

    let err = run_live_daemon_from_durable_store_with_neg_risk_live_targets_instrumented(
        &StaticSnapshotSource::empty(),
        AppInstrumentation::disabled(),
        targets,
        BTreeSet::new(),
        BTreeSet::new(),
    )
    .expect_err("candidate-derived startup should fail closed when provenance is missing");

    assert!(
        err.to_string().contains("candidate adoption provenance"),
        "{err}"
    );

    database.cleanup();
}

#[test]
fn supervisor_resume_persists_candidate_artifacts_from_candidate_dirty_inputs() {
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

    let summary = supervisor
        .resume_once()
        .expect("resume should process candidate dirty inputs");

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
