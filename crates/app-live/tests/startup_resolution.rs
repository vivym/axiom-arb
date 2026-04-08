use std::{
    env,
    sync::atomic::{AtomicU64, Ordering},
};

use app_live::resolve_startup_targets;
use config_schema::{load_raw_config_from_str, ValidatedConfig};
use persistence::{
    models::{
        AdoptableStrategyRevisionRow, AdoptableTargetRevisionRow, CandidateAdoptionProvenanceRow,
        CandidateTargetSetRow, StrategyAdoptionProvenanceRow, StrategyCandidateSetRow,
    },
    run_migrations, CandidateAdoptionRepo, CandidateArtifactRepo, StrategyAdoptionRepo,
    StrategyControlArtifactRepo,
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

#[tokio::test]
async fn startup_resolution_supports_distinct_operator_strategy_revision_anchor() {
    let db = TestDatabase::new().await;
    db.seed_adoptable_strategy_with_rendered_live_targets(
        "adoptable-strategy-12",
        "strategy-candidate-12",
        "strategy-rev-12",
        "targets-rev-9",
        sample_rendered_live_targets_json(),
    )
    .await;

    let resolved = resolve_startup_targets(&db.pool, &sample_neutral_live_view("strategy-rev-12"))
        .await
        .expect("strategy-adopted startup resolution should succeed for distinct strategy anchor");

    assert_eq!(resolved.targets.revision(), "strategy-rev-12");
    assert_eq!(
        resolved.operator_target_revision.as_deref(),
        Some("strategy-rev-12")
    );
    assert!(resolved.targets.targets().contains_key("family-a"));
}

#[tokio::test]
async fn adopted_strategy_revision_can_resolve_fullset_and_negrisk_artifacts() {
    let db = TestDatabase::new().await;
    db.seed_adopted_strategy_revision("strategy-rev-12", sample_multi_route_revision())
        .await;

    let resolved = app_live::startup::resolve_startup_strategy_revision(
        &db.pool,
        &sample_neutral_live_view("strategy-rev-12"),
    )
    .await
    .unwrap();

    assert!(resolved.route_artifacts.contains_key("full-set"));
    assert!(resolved.route_artifacts.contains_key("neg-risk"));
}

#[tokio::test]
async fn startup_resolution_fails_closed_when_operator_strategy_revision_provenance_is_missing() {
    let db = TestDatabase::new().await;

    let err = resolve_startup_targets(&db.pool, &sample_neutral_live_view("strategy-rev-missing"))
        .await
        .expect_err("missing strategy provenance should fail closed");

    let text = err.to_string();
    assert!(text.contains("strategy-rev-missing"), "{text}");
    assert!(text.contains("provenance"), "{text}");
}

#[tokio::test]
async fn adopted_strategy_source_without_operator_strategy_revision_fails_closed() {
    let db = TestDatabase::new().await;

    let err = resolve_startup_targets(&db.pool, &sample_neutral_live_view_without_revision())
        .await
        .expect_err("missing adopted strategy revision should fail closed");

    let text = err.to_string();
    assert!(
        text.contains("strategy_control.operator_strategy_revision"),
        "{text}"
    );
}

#[tokio::test]
async fn startup_resolution_rejects_invalid_route_scope_for_registered_adapter() {
    let db = TestDatabase::new().await;
    db.seed_adopted_strategy_revision(
        "strategy-rev-invalid-fullset-scope",
        sample_invalid_fullset_scope_revision(),
    )
    .await;

    let err = app_live::startup::resolve_startup_strategy_revision(
        &db.pool,
        &sample_neutral_live_view("strategy-rev-invalid-fullset-scope"),
    )
    .await
    .expect_err("invalid full-set scope should fail closed");

    let text = err.to_string();
    assert!(text.contains("full-set"), "{text}");
    assert!(text.contains("default scope"), "{text}");
}

#[tokio::test]
async fn compatibility_explicit_targets_still_resolve_without_operator_strategy_revision() {
    let db = TestDatabase::new().await;

    let resolved =
        resolve_startup_targets(&db.pool, &sample_compatibility_live_view_without_revision())
            .await
            .expect("compatibility explicit targets should still resolve via config fallback");

    assert!(!resolved.targets.is_empty());
    assert_eq!(
        resolved.operator_target_revision.as_deref(),
        Some(resolved.targets.revision())
    );
    assert!(resolved.targets.targets().contains_key("family-a"));
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

    async fn seed_adoptable_strategy_with_rendered_live_targets(
        &self,
        adoptable_strategy_revision: &str,
        strategy_candidate_revision: &str,
        operator_strategy_revision: &str,
        rendered_operator_target_revision: &str,
        rendered_live_targets: Value,
    ) {
        StrategyControlArtifactRepo
            .upsert_strategy_candidate_set(
                &self.pool,
                &StrategyCandidateSetRow {
                    strategy_candidate_revision: strategy_candidate_revision.to_owned(),
                    snapshot_id: "snapshot-strategy-12".to_owned(),
                    source_revision: "discovery-strategy-12".to_owned(),
                    payload: json!({
                        "strategy_candidate_revision": strategy_candidate_revision,
                        "snapshot_id": "snapshot-strategy-12",
                    }),
                },
            )
            .await
            .expect("strategy candidate row should persist");

        StrategyControlArtifactRepo
            .upsert_adoptable_strategy_revision(
                &self.pool,
                &AdoptableStrategyRevisionRow {
                    adoptable_strategy_revision: adoptable_strategy_revision.to_owned(),
                    strategy_candidate_revision: strategy_candidate_revision.to_owned(),
                    rendered_operator_strategy_revision: operator_strategy_revision.to_owned(),
                    payload: json!({
                        "adoptable_strategy_revision": adoptable_strategy_revision,
                        "strategy_candidate_revision": strategy_candidate_revision,
                        "rendered_operator_strategy_revision": operator_strategy_revision,
                        "rendered_operator_target_revision": rendered_operator_target_revision,
                        "rendered_live_targets": rendered_live_targets,
                    }),
                },
            )
            .await
            .expect("adoptable strategy row should persist");

        StrategyAdoptionRepo
            .upsert_provenance(
                &self.pool,
                &StrategyAdoptionProvenanceRow {
                    operator_strategy_revision: operator_strategy_revision.to_owned(),
                    adoptable_strategy_revision: adoptable_strategy_revision.to_owned(),
                    strategy_candidate_revision: strategy_candidate_revision.to_owned(),
                },
            )
            .await
            .expect("strategy provenance should persist");
    }

    async fn seed_adopted_strategy_revision(
        &self,
        operator_strategy_revision: &str,
        payload: Value,
    ) {
        let adoptable_strategy_revision = "adoptable-strategy-12";
        let strategy_candidate_revision = "strategy-candidate-12";

        StrategyControlArtifactRepo
            .upsert_strategy_candidate_set(
                &self.pool,
                &StrategyCandidateSetRow {
                    strategy_candidate_revision: strategy_candidate_revision.to_owned(),
                    snapshot_id: "snapshot-strategy-12".to_owned(),
                    source_revision: "discovery-strategy-12".to_owned(),
                    payload: json!({
                        "strategy_candidate_revision": strategy_candidate_revision,
                        "snapshot_id": "snapshot-strategy-12",
                    }),
                },
            )
            .await
            .expect("strategy candidate row should persist");

        StrategyControlArtifactRepo
            .upsert_adoptable_strategy_revision(
                &self.pool,
                &AdoptableStrategyRevisionRow {
                    adoptable_strategy_revision: adoptable_strategy_revision.to_owned(),
                    strategy_candidate_revision: strategy_candidate_revision.to_owned(),
                    rendered_operator_strategy_revision: operator_strategy_revision.to_owned(),
                    payload,
                },
            )
            .await
            .expect("adoptable strategy row should persist");

        StrategyAdoptionRepo
            .upsert_provenance(
                &self.pool,
                &StrategyAdoptionProvenanceRow {
                    operator_strategy_revision: operator_strategy_revision.to_owned(),
                    adoptable_strategy_revision: adoptable_strategy_revision.to_owned(),
                    strategy_candidate_revision: strategy_candidate_revision.to_owned(),
                },
            )
            .await
            .expect("strategy provenance should persist");
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

fn sample_neutral_live_view(
    operator_strategy_revision: &str,
) -> config_schema::AppLiveConfigView<'static> {
    let raw = Box::leak(Box::new(
        load_raw_config_from_str(&format!(
            r#"
[runtime]
mode = "live"
real_user_shadow_smoke = false

[strategy_control]
source = "adopted"
operator_strategy_revision = "{operator_strategy_revision}"

[strategies.neg_risk]
enabled = true

[strategies.neg_risk.rollout]
approved_scopes = ["family-a"]
ready_scopes = ["family-a"]

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
"#,
        ))
        .expect("config should parse"),
    ));
    let validated = Box::leak(Box::new(
        ValidatedConfig::new(raw.clone()).expect("config should validate"),
    ));

    validated.for_app_live().expect("live view should validate")
}

fn sample_neutral_live_view_without_revision() -> config_schema::AppLiveConfigView<'static> {
    let raw = Box::leak(Box::new(
        load_raw_config_from_str(
            r#"
[runtime]
mode = "live"
real_user_shadow_smoke = false

[strategy_control]
source = "adopted"

[strategies.neg_risk]
enabled = true

[strategies.neg_risk.rollout]
approved_scopes = ["family-a"]
ready_scopes = ["family-a"]

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
"#,
        )
        .expect("config should parse"),
    ));
    let validated = Box::leak(Box::new(
        ValidatedConfig::new(raw.clone()).expect("config should validate"),
    ));

    validated.for_app_live().expect("live view should validate")
}

fn sample_compatibility_live_view_without_revision() -> config_schema::AppLiveConfigView<'static> {
    let raw = Box::leak(Box::new(
        load_raw_config_from_str(
            r#"
[runtime]
mode = "live"
real_user_shadow_smoke = false

[strategy_control]
source = "adopted"

[strategies.neg_risk]
enabled = true

[strategies.neg_risk.rollout]
approved_scopes = ["family-a"]
ready_scopes = ["family-a"]

[polymarket.account]
address = "0x1111111111111111111111111111111111111111"
funder_address = "0x2222222222222222222222222222222222222222"
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
"#,
        )
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

fn sample_multi_route_revision() -> Value {
    json!({
        "adoptable_strategy_revision": "adoptable-strategy-12",
        "strategy_candidate_revision": "strategy-candidate-12",
        "rendered_operator_strategy_revision": "strategy-rev-12",
        "bundle_policy_version": "strategy-bundle-v1",
        "route_artifact_count": 2,
        "route_artifacts": [
            {
                "key": {
                    "route": "full-set",
                    "scope": "default",
                },
                "route_policy_version": "full-set-route-policy-v1",
                "semantic_digest": "full-set-digest-12",
                "content": {
                    "config_basis_digest": "full-set-basis-default",
                    "mode": "static-default",
                },
            },
            {
                "key": {
                    "route": "neg-risk",
                    "scope": "family-a",
                },
                "route_policy_version": "neg-risk-route-policy-v1",
                "semantic_digest": "neg-risk-digest-12",
                "content": {
                    "family_id": "family-a",
                    "rendered_live_target": {
                        "family_id": "family-a",
                        "members": [
                            {
                                "condition_id": "condition-1",
                                "token_id": "token-1",
                                "price": "0.43",
                                "quantity": "5",
                            }
                        ],
                    },
                    "target_id": "candidate-target-family-a",
                    "validation": {
                        "status": "adoptable",
                    },
                },
            },
        ],
        "warnings": [],
        "execution_requests": [],
    })
}

fn sample_invalid_fullset_scope_revision() -> Value {
    json!({
        "route_artifacts": [
            {
                "key": {
                    "route": "full-set",
                    "scope": "family-a"
                },
                "route_policy_version": "full-set-policy-v1",
                "semantic_digest": "digest-fullset-family-a",
                "content": {
                    "route": "full-set",
                    "scope": "family-a",
                    "mode": "default"
                }
            },
            {
                "key": {
                    "route": "neg-risk",
                    "scope": "family-a"
                },
                "route_policy_version": "neg-risk-policy-v1",
                "semantic_digest": "digest-neg-risk-family-a",
                "content": {
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
            }
        ],
        "rendered_live_targets": sample_rendered_live_targets_json(),
        "rendered_operator_strategy_revision": "strategy-rev-invalid-fullset-scope",
        "rendered_operator_target_revision": "targets-rev-9"
    })
}

fn default_database_url_for_tests() -> &'static str {
    "postgres://axiom:axiom@localhost:5432/axiom_arb"
}

fn schema_scoped_database_url(database_url: &str, schema: &str) -> String {
    let separator = if database_url.contains('?') { '&' } else { '?' };
    format!("{database_url}{separator}options=-csearch_path%3D{schema}")
}
