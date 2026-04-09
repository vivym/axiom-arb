use std::{
    env, fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use app_live::strategy_control::{
    migrate_legacy_strategy_control, MigrationSource, StrategyControlMigrationError,
};
use persistence::{
    models::{AdoptableTargetRevisionRow, CandidateAdoptionProvenanceRow, CandidateTargetSetRow},
    run_migrations, CandidateAdoptionRepo, CandidateArtifactRepo, StrategyAdoptionRepo,
    StrategyControlArtifactRepo,
};
use serde_json::json;
use sqlx::{postgres::PgPoolOptions, PgPool};

#[path = "support/cli.rs"]
mod cli_support;

static NEXT_SCHEMA_ID: AtomicU64 = AtomicU64::new(1);

const LEGACY_TARGET_SOURCE_CONFIG: &str = r#"
[runtime]
mode = "live"

[polymarket.account]
address = "0x1111111111111111111111111111111111111111"
signature_type = "eoa"
wallet_route = "eoa"
api_key = "poly-api-key"
secret = "poly-secret"
passphrase = "poly-passphrase"

[negrisk.target_source]
source = "adopted"
operator_target_revision = "targets-rev-9"
"#;

const LEGACY_EXPLICIT_CONFIG: &str = r#"
[runtime]
mode = "live"

[polymarket.account]
address = "0x1111111111111111111111111111111111111111"
signature_type = "eoa"
wallet_route = "eoa"
api_key = "poly-api-key"
secret = "poly-secret"
passphrase = "poly-passphrase"

[[negrisk.targets]]
family_id = "family-a"

[[negrisk.targets.members]]
condition_id = "condition-1"
token_id = "token-1"
price = "0.43"
quantity = "5"
"#;

const EMPTY_EXPLICIT_CONFIG: &str = r#"
[runtime]
mode = "live"

[polymarket.account]
address = "0x1111111111111111111111111111111111111111"
signature_type = "eoa"
wallet_route = "eoa"
api_key = "poly-api-key"
secret = "poly-secret"
passphrase = "poly-passphrase"

[negrisk]
targets = []
"#;

const MIXED_CANONICAL_AND_LEGACY_CONFIG: &str = r#"
[runtime]
mode = "live"

[polymarket.account]
address = "0x1111111111111111111111111111111111111111"
signature_type = "eoa"
wallet_route = "eoa"
api_key = "poly-api-key"
secret = "poly-secret"
passphrase = "poly-passphrase"

[strategy_control]
source = "adopted"
operator_strategy_revision = "strategy-rev-12"

[negrisk.target_source]
source = "adopted"
operator_target_revision = "targets-rev-9"
"#;

#[tokio::test]
async fn migrate_legacy_target_source_materializes_canonical_strategy_rows_and_rewrites_config() {
    let database = TestDatabase::new().await;
    database.seed_legacy_target_lineage().await;
    let config_path = write_temp_config("legacy-target-source.toml", LEGACY_TARGET_SOURCE_CONFIG);

    let outcome = migrate_legacy_strategy_control(database.pool(), &config_path)
        .await
        .unwrap();

    assert_eq!(outcome.operator_strategy_revision, "strategy-rev-9");
    assert_eq!(outcome.adoptable_strategy_revision, "adoptable-strategy-9");
    assert_eq!(outcome.strategy_candidate_revision, "strategy-candidate-9");
    assert_eq!(outcome.source, MigrationSource::LegacyTargetSource);
    assert_rewritten_to_strategy_control(&config_path, "strategy-rev-9");

    let provenance = StrategyAdoptionRepo
        .get_by_operator_strategy_revision(database.pool(), "strategy-rev-9")
        .await
        .unwrap()
        .expect("canonical strategy provenance should exist");
    assert_eq!(
        provenance.adoptable_strategy_revision,
        "adoptable-strategy-9"
    );
    assert_eq!(
        provenance.strategy_candidate_revision,
        "strategy-candidate-9"
    );

    let adoptable = StrategyControlArtifactRepo
        .get_adoptable_strategy_revision(database.pool(), "adoptable-strategy-9")
        .await
        .unwrap()
        .expect("canonical adoptable strategy row should exist");
    assert_eq!(
        adoptable.rendered_operator_strategy_revision,
        "strategy-rev-9"
    );
    assert_eq!(
        adoptable.payload["rendered_operator_strategy_revision"],
        "strategy-rev-9"
    );

    let candidate = StrategyControlArtifactRepo
        .get_strategy_candidate_set(database.pool(), "strategy-candidate-9")
        .await
        .unwrap()
        .expect("canonical strategy candidate row should exist");
    assert_eq!(candidate.snapshot_id, "snapshot-candidate-9");

    let _ = fs::remove_file(config_path);
    database.cleanup().await;
}

#[tokio::test]
async fn migrate_legacy_target_source_reuses_existing_canonical_lineage_for_same_legacy_revision() {
    let database = TestDatabase::new().await;
    database
        .seed_existing_canonical_target_source_mapping()
        .await;
    let config_path = write_temp_config(
        "legacy-target-source-existing.toml",
        LEGACY_TARGET_SOURCE_CONFIG,
    );

    let outcome = migrate_legacy_strategy_control(database.pool(), &config_path)
        .await
        .unwrap();

    assert_eq!(outcome.operator_strategy_revision, "strategy-rev-custom");
    assert_eq!(
        outcome.adoptable_strategy_revision,
        "adoptable-strategy-custom"
    );
    assert_eq!(
        outcome.strategy_candidate_revision,
        "strategy-candidate-custom"
    );
    assert_eq!(outcome.source, MigrationSource::LegacyTargetSource);
    assert_rewritten_to_strategy_control(&config_path, "strategy-rev-custom");

    let _ = fs::remove_file(config_path);
    database.cleanup().await;
}

#[tokio::test]
async fn migrate_legacy_target_source_fails_closed_when_only_derived_canonical_name_collides() {
    let database = TestDatabase::new().await;
    database.seed_unmarked_canonical_strategy_revision().await;
    let config_path = write_temp_config(
        "legacy-target-source-derived-only.toml",
        LEGACY_TARGET_SOURCE_CONFIG,
    );
    let original =
        fs::read_to_string(&config_path).expect("original derived-only config should read");

    let error = migrate_legacy_strategy_control(database.pool(), &config_path)
        .await
        .unwrap_err();

    assert!(matches!(
        error,
        StrategyControlMigrationError::MissingLineage(_)
    ));
    assert!(error
        .to_string()
        .contains("has no target-shaped provenance or canonical strategy lineage"));
    assert_eq!(
        fs::read_to_string(&config_path).expect("config should not be rewritten on collision"),
        original
    );

    let _ = fs::remove_file(config_path);
    database.cleanup().await;
}

#[tokio::test]
async fn migrate_explicit_targets_uses_deterministic_strategy_digest_and_rewrites_config() {
    let database = TestDatabase::new().await;
    let config_path = write_temp_config("legacy-explicit.toml", LEGACY_EXPLICIT_CONFIG);

    let outcome = migrate_legacy_strategy_control(database.pool(), &config_path)
        .await
        .unwrap();

    assert_eq!(outcome.source, MigrationSource::LegacyExplicitTargets);
    assert!(outcome
        .operator_strategy_revision
        .starts_with("strategy-rev-"));
    assert_eq!(
        outcome.adoptable_strategy_revision,
        format!(
            "adoptable-strategy-{}",
            outcome
                .operator_strategy_revision
                .trim_start_matches("strategy-rev-")
        )
    );
    assert_eq!(
        outcome.strategy_candidate_revision,
        format!(
            "strategy-candidate-{}",
            outcome
                .operator_strategy_revision
                .trim_start_matches("strategy-rev-")
        )
    );
    assert_rewritten_to_strategy_control(&config_path, &outcome.operator_strategy_revision);

    let provenance = StrategyAdoptionRepo
        .get_by_operator_strategy_revision(database.pool(), &outcome.operator_strategy_revision)
        .await
        .unwrap()
        .expect("canonical explicit-target strategy provenance should exist");
    let adoptable = StrategyControlArtifactRepo
        .get_adoptable_strategy_revision(database.pool(), &provenance.adoptable_strategy_revision)
        .await
        .unwrap()
        .expect("canonical adoptable strategy row should exist");
    assert_eq!(
        adoptable.rendered_operator_strategy_revision,
        outcome.operator_strategy_revision
    );
    assert!(adoptable.payload["rendered_live_targets"]["family-a"].is_object());

    let _ = fs::remove_file(config_path);
    database.cleanup().await;
}

#[tokio::test]
async fn migrate_empty_targets_array_fails_closed() {
    let database = TestDatabase::new().await;
    let config_path = write_temp_config("legacy-empty.toml", EMPTY_EXPLICIT_CONFIG);

    let error = migrate_legacy_strategy_control(database.pool(), &config_path)
        .await
        .unwrap_err();

    assert!(matches!(
        error,
        StrategyControlMigrationError::InvalidConfig(_)
    ));
    assert!(error.to_string().contains("empty negrisk.targets"));

    let _ = fs::remove_file(config_path);
    database.cleanup().await;
}

#[tokio::test]
async fn migrate_mixed_canonical_and_legacy_input_fails_closed_without_rewrite() {
    let database = TestDatabase::new().await;
    let config_path = write_temp_config(
        "mixed-canonical-legacy.toml",
        MIXED_CANONICAL_AND_LEGACY_CONFIG,
    );
    let original = fs::read_to_string(&config_path)
        .expect("original mixed config should read before migration");

    let error = migrate_legacy_strategy_control(database.pool(), &config_path)
        .await
        .unwrap_err();

    assert!(matches!(
        error,
        StrategyControlMigrationError::InvalidConfig(_)
    ));
    assert!(error
        .to_string()
        .contains("canonical [strategy_control] cannot be combined"));
    assert_eq!(
        fs::read_to_string(&config_path).expect("mixed config should remain readable"),
        original
    );

    let _ = fs::remove_file(config_path);
    database.cleanup().await;
}

fn write_temp_config(name: &str, contents: &str) -> PathBuf {
    let dir = tempfile::tempdir().expect("temp dir should create");
    let path = dir.keep().join(name);
    fs::write(&path, contents).expect("temp config should write");
    path
}

fn assert_rewritten_to_strategy_control(config_path: &Path, expected_revision: &str) {
    let text = fs::read_to_string(config_path).expect("rewritten config should read");
    assert!(text.contains("[strategy_control]"), "{text}");
    assert!(text.contains("source = \"adopted\""), "{text}");
    assert!(
        text.contains(&format!(
            "operator_strategy_revision = \"{expected_revision}\""
        )),
        "{text}"
    );
    assert!(!text.contains("[negrisk.target_source]"), "{text}");
    assert!(!text.contains("[[negrisk.targets]]"), "{text}");
    assert!(!text.contains("operator_target_revision"), "{text}");
}

struct TestDatabase {
    admin_pool: PgPool,
    pool: PgPool,
    schema: String,
}

impl TestDatabase {
    async fn new() -> Self {
        let admin_database_url = env::var("DATABASE_URL")
            .unwrap_or_else(|_| cli_support::default_test_database_url().to_owned());
        let admin_pool = PgPoolOptions::new()
            .max_connections(8)
            .connect(&admin_database_url)
            .await
            .expect("test database should connect");
        let schema = format!(
            "app_live_strategy_control_migration_{}_{}",
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
        }
    }

    fn pool(&self) -> &PgPool {
        &self.pool
    }

    async fn seed_legacy_target_lineage(&self) {
        CandidateArtifactRepo
            .upsert_candidate_target_set(
                &self.pool,
                &CandidateTargetSetRow {
                    candidate_revision: "candidate-9".to_owned(),
                    snapshot_id: "snapshot-candidate-9".to_owned(),
                    source_revision: "discovery-candidate-9".to_owned(),
                    payload: json!({
                        "candidate_revision": "candidate-9",
                        "snapshot_id": "snapshot-candidate-9",
                    }),
                },
            )
            .await
            .expect("candidate row should persist");

        CandidateArtifactRepo
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
                        "rendered_live_targets": {
                            "family-a": {
                                "family_id": "family-a",
                                "members": [
                                    {
                                        "condition_id": "0x0000000000000000000000000000000000000000000000000000000000000001",
                                        "token_id": "29",
                                        "price": "0.43",
                                        "quantity": "5"
                                    }
                                ]
                            }
                        }
                    }),
                },
            )
            .await
            .expect("adoptable row should persist");

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
            .expect("candidate provenance should persist");
    }

    async fn seed_existing_canonical_target_source_mapping(&self) {
        StrategyControlArtifactRepo
            .upsert_strategy_candidate_set(
                &self.pool,
                &persistence::models::StrategyCandidateSetRow {
                    strategy_candidate_revision: "strategy-candidate-custom".to_owned(),
                    snapshot_id: "snapshot-candidate-custom".to_owned(),
                    source_revision: "migration-seeded".to_owned(),
                    payload: json!({
                        "strategy_candidate_revision": "strategy-candidate-custom",
                        "legacy_operator_target_revision": "targets-rev-9",
                    }),
                },
            )
            .await
            .expect("canonical strategy candidate should persist");

        StrategyControlArtifactRepo
            .upsert_adoptable_strategy_revision(
                &self.pool,
                &persistence::models::AdoptableStrategyRevisionRow {
                    adoptable_strategy_revision: "adoptable-strategy-custom".to_owned(),
                    strategy_candidate_revision: "strategy-candidate-custom".to_owned(),
                    rendered_operator_strategy_revision: "strategy-rev-custom".to_owned(),
                    payload: json!({
                        "adoptable_strategy_revision": "adoptable-strategy-custom",
                        "strategy_candidate_revision": "strategy-candidate-custom",
                        "rendered_operator_strategy_revision": "strategy-rev-custom",
                        "legacy_operator_target_revision": "targets-rev-9",
                    }),
                },
            )
            .await
            .expect("canonical adoptable strategy should persist");

        StrategyAdoptionRepo
            .upsert_provenance(
                &self.pool,
                &persistence::models::StrategyAdoptionProvenanceRow {
                    operator_strategy_revision: "strategy-rev-custom".to_owned(),
                    adoptable_strategy_revision: "adoptable-strategy-custom".to_owned(),
                    strategy_candidate_revision: "strategy-candidate-custom".to_owned(),
                },
            )
            .await
            .expect("canonical strategy provenance should persist");
    }

    async fn seed_unmarked_canonical_strategy_revision(&self) {
        StrategyControlArtifactRepo
            .upsert_strategy_candidate_set(
                &self.pool,
                &persistence::models::StrategyCandidateSetRow {
                    strategy_candidate_revision: "strategy-candidate-9".to_owned(),
                    snapshot_id: "snapshot-candidate-9".to_owned(),
                    source_revision: "derived-name-only".to_owned(),
                    payload: json!({
                        "strategy_candidate_revision": "strategy-candidate-9",
                    }),
                },
            )
            .await
            .expect("unmarked canonical strategy candidate should persist");

        StrategyControlArtifactRepo
            .upsert_adoptable_strategy_revision(
                &self.pool,
                &persistence::models::AdoptableStrategyRevisionRow {
                    adoptable_strategy_revision: "adoptable-strategy-9".to_owned(),
                    strategy_candidate_revision: "strategy-candidate-9".to_owned(),
                    rendered_operator_strategy_revision: "strategy-rev-9".to_owned(),
                    payload: json!({
                        "adoptable_strategy_revision": "adoptable-strategy-9",
                        "strategy_candidate_revision": "strategy-candidate-9",
                        "rendered_operator_strategy_revision": "strategy-rev-9",
                    }),
                },
            )
            .await
            .expect("unmarked canonical adoptable strategy should persist");

        StrategyAdoptionRepo
            .upsert_provenance(
                &self.pool,
                &persistence::models::StrategyAdoptionProvenanceRow {
                    operator_strategy_revision: "strategy-rev-9".to_owned(),
                    adoptable_strategy_revision: "adoptable-strategy-9".to_owned(),
                    strategy_candidate_revision: "strategy-candidate-9".to_owned(),
                },
            )
            .await
            .expect("unmarked canonical strategy provenance should persist");
    }

    async fn cleanup(self) {
        self.pool.close().await;
        sqlx::query(&format!(
            r#"DROP SCHEMA IF EXISTS "{}" CASCADE"#,
            self.schema
        ))
        .execute(&self.admin_pool)
        .await
        .expect("test schema should drop");
        self.admin_pool.close().await;
    }
}

fn schema_scoped_database_url(base: &str, schema: &str) -> String {
    let separator = if base.contains('?') { '&' } else { '?' };
    format!("{base}{separator}options=-csearch_path%3D{schema}")
}
