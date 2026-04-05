use std::{
    env,
    sync::atomic::{AtomicU64, Ordering},
};

use app_live::resolve_startup_targets;
use config_schema::{load_raw_config_from_str, ValidatedConfig};
use persistence::{
    models::{AdoptableTargetRevisionRow, CandidateAdoptionProvenanceRow, CandidateTargetSetRow},
    run_migrations, CandidateAdoptionRepo, CandidateArtifactRepo,
};
use serde_json::{json, Value};
use sqlx::{postgres::PgPoolOptions, PgPool};

static SCHEMA_COUNTER: AtomicU64 = AtomicU64::new(0);

#[tokio::test]
async fn resolves_adopted_target_source_to_operator_target_revision() {
    let db = TestDatabase::new().await;
    db.seed_adoptable_target_with_rendered_live_targets(
        "adoptable-9",
        "candidate-9",
        "targets-rev-9",
        sample_rendered_live_targets_json(),
    )
    .await;

    let resolved = resolve_startup_targets(&db.pool, &sample_live_view("targets-rev-9"))
        .await
        .unwrap();

    assert_eq!(resolved.targets.revision(), "targets-rev-9");
    assert!(resolved.targets.targets().contains_key("family-a"));
}

#[tokio::test]
async fn adopted_target_resolution_fails_closed_when_provenance_is_missing() {
    let db = TestDatabase::new().await;
    let err = resolve_startup_targets(&db.pool, &sample_live_view("targets-rev-missing"))
        .await
        .unwrap_err();

    assert!(err.to_string().contains("operator_target_revision"));
}

#[tokio::test]
async fn adopted_target_resolution_fails_closed_when_rendered_live_targets_are_empty() {
    let db = TestDatabase::new().await;
    db.seed_adoptable_target_with_rendered_live_targets(
        "adoptable-empty",
        "candidate-empty",
        "targets-rev-empty",
        json!({}),
    )
    .await;

    let err = resolve_startup_targets(&db.pool, &sample_live_view("targets-rev-empty"))
        .await
        .unwrap_err();

    assert!(err.to_string().contains("rendered_live_targets"));
    assert!(err.to_string().contains("targets-rev-empty"));
}

struct TestDatabase {
    pool: PgPool,
}

impl TestDatabase {
    async fn new() -> Self {
        let database_url = env::var("TEST_DATABASE_URL")
            .or_else(|_| env::var("DATABASE_URL"))
            .unwrap_or_else(|_| default_database_url_for_tests().to_owned());
        let admin_pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(&database_url)
            .await
            .expect("admin pool should connect");
        let schema = format!(
            "app_live_startup_resolution_{}_{}",
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

        Self { pool }
    }

    async fn seed_adoptable_target_with_rendered_live_targets(
        &self,
        adoptable_revision: &str,
        candidate_revision: &str,
        operator_target_revision: &str,
        rendered_live_targets: Value,
    ) {
        CandidateArtifactRepo
            .upsert_candidate_target_set(
                &self.pool,
                &CandidateTargetSetRow {
                    candidate_revision: candidate_revision.to_owned(),
                    snapshot_id: "snapshot-9".to_owned(),
                    source_revision: "discovery-9".to_owned(),
                    payload: json!({
                        "candidate_revision": candidate_revision,
                        "snapshot_id": "snapshot-9",
                    }),
                },
            )
            .await
            .expect("candidate row should persist");

        CandidateArtifactRepo
            .upsert_adoptable_target_revision(
                &self.pool,
                &AdoptableTargetRevisionRow {
                    adoptable_revision: adoptable_revision.to_owned(),
                    candidate_revision: candidate_revision.to_owned(),
                    rendered_operator_target_revision: operator_target_revision.to_owned(),
                    payload: json!({
                        "adoptable_revision": adoptable_revision,
                        "candidate_revision": candidate_revision,
                        "rendered_operator_target_revision": operator_target_revision,
                        "rendered_live_targets": rendered_live_targets,
                    }),
                },
            )
            .await
            .expect("adoptable row should persist");

        CandidateAdoptionRepo
            .upsert_provenance(
                &self.pool,
                &CandidateAdoptionProvenanceRow {
                    operator_target_revision: operator_target_revision.to_owned(),
                    adoptable_revision: adoptable_revision.to_owned(),
                    candidate_revision: candidate_revision.to_owned(),
                },
            )
            .await
            .expect("candidate provenance should persist");
    }
}

fn sample_live_view(operator_target_revision: &str) -> config_schema::AppLiveConfigView<'static> {
    let raw = Box::leak(Box::new(
        load_raw_config_from_str(&format!(
            r#"
[runtime]
mode = "live"
real_user_shadow_smoke = false

[polymarket.source]
clob_host = "https://clob.polymarket.com"
data_api_host = "https://gamma-api.polymarket.com"
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
operator_target_revision = "{operator_target_revision}"
"#,
        ))
        .expect("config should parse"),
    ));
    let validated = Box::leak(Box::new(
        ValidatedConfig::new(raw.clone()).expect("config should validate"),
    ));

    validated.for_app_live().expect("live view should validate")
}

fn sample_rendered_live_targets_json() -> Value {
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

fn default_database_url_for_tests() -> &'static str {
    "postgres://axiom:axiom@localhost:5432/axiom_arb"
}

fn schema_scoped_database_url(database_url: &str, schema: &str) -> String {
    let separator = if database_url.contains('?') { '&' } else { '?' };
    format!("{database_url}{separator}options=-csearch_path%3D{schema}")
}
