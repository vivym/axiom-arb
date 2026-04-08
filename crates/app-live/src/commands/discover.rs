use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    io,
    path::Path,
};

use async_trait::async_trait;
use chrono::Utc;
use config_schema::{load_raw_config_from_path, RuntimeModeToml, ValidatedConfig};
use persistence::{connect_pool_from_env, StrategyControlArtifactRepo};
use serde_json::json;
use sha2::{Digest, Sha256};
use state::{
    CandidateProjectionReadiness, CandidatePublication, DirtyDomain, StateApplier, StateStore,
};

use crate::{
    cli::DiscoverArgs,
    config::{LocalSignerConfig, PolymarketSourceConfig},
    polymarket_runtime_adapter::build_polymarket_metadata_gateway,
    task_groups::{MetadataDiscoveryBatch, MetadataTaskGroup},
    CandidateNotice, CandidateRestrictionTruth, DiscoverySupervisor, InputTaskEvent,
};
use venue_polymarket::{NegRiskMarketMetadata, PolymarketGateway};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DiscoverSummary {
    candidate_count: usize,
    adoptable_count: usize,
    recommended_adoptable_revision: Option<String>,
    route_diffs: Vec<String>,
    warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct RouteArtifactKey {
    route: String,
    scope: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RouteArtifactSummary {
    key: RouteArtifactKey,
    semantic_digest: String,
}

#[async_trait]
pub(crate) trait DiscoverMetadataFacade: Send + Sync {
    async fn refresh_neg_risk_metadata(&self)
        -> Result<Vec<NegRiskMarketMetadata>, Box<dyn Error>>;
}

pub(crate) async fn fetch_discover_metadata(
    facade: &dyn DiscoverMetadataFacade,
) -> Result<Vec<NegRiskMarketMetadata>, Box<dyn Error>> {
    facade.refresh_neg_risk_metadata().await
}

#[async_trait]
impl DiscoverMetadataFacade for PolymarketGateway {
    async fn refresh_neg_risk_metadata(
        &self,
    ) -> Result<Vec<NegRiskMarketMetadata>, Box<dyn Error>> {
        venue_polymarket::PolymarketGateway::refresh_neg_risk_metadata(self)
            .await
            .map_err(|error| Box::new(error) as Box<dyn Error>)
    }
}

pub fn execute(args: DiscoverArgs) -> Result<(), Box<dyn Error>> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let summary = runtime.block_on(run_discover_from_config(&args.config))?;
    render_discover_summary(&summary);
    Ok(())
}

pub(crate) async fn run_discover_from_config(
    config_path: &Path,
) -> Result<DiscoverSummary, Box<dyn Error>> {
    println!("Starting discovery");
    let raw = load_raw_config_from_path(config_path)?;
    let validated = ValidatedConfig::new(raw)?;
    let config = validated.for_app_live()?;
    if config.mode() != RuntimeModeToml::Live {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "discover requires runtime.mode = \"live\"",
        )
        .into());
    }
    let source = PolymarketSourceConfig::try_from(&config)?;
    let signer = LocalSignerConfig::try_from(&config)?;
    tracing::debug!(
        config_path = %config_path.display(),
        "discover loaded live config"
    );

    let metadata_facade = build_polymarket_metadata_gateway(&source)?;
    println!("Fetching Polymarket metadata");
    let metadata_rows = fetch_discover_metadata(&metadata_facade).await?;
    tracing::debug!(
        metadata_row_count = metadata_rows.len(),
        "discover fetched metadata rows"
    );
    let source_session_id = format!(
        "discover-session-{}",
        Utc::now()
            .timestamp_nanos_opt()
            .unwrap_or_else(|| Utc::now().timestamp_micros() * 1_000)
    );
    let batch = MetadataTaskGroup::authoritative_discovery_batch(
        &metadata_rows,
        &source_session_id,
        Utc::now(),
    );
    println!("Materializing strategy artifacts");
    let publication = authoritative_candidate_publication(&batch, &source_session_id)?;
    let notice = CandidateNotice::authoritative_from_publication(
        &publication,
        [DirtyDomain::Candidates],
        None,
        batch.rendered_live_targets.clone(),
        CandidateRestrictionTruth::eligible(),
    )
    .with_full_set_basis_digest(full_set_basis_digest(&source, &signer)?);
    let discovery_summary = DiscoverySupervisor::persist_notice_for_runtime(notice).await?;
    tracing::debug!(
        source_session_id,
        candidate_revision = ?discovery_summary.candidate_revision,
        adoptable_revision = ?discovery_summary.adoptable_revision,
        "discover materialized strategy bundle"
    );

    let pool = connect_pool_from_env().await?;
    let artifacts = StrategyControlArtifactRepo;
    let (candidate_count, route_diffs) = match discovery_summary.candidate_revision.as_deref() {
        Some(candidate_revision) => {
            let candidate = artifacts
                .get_strategy_candidate_set(&pool, candidate_revision)
                .await?
                .ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::NotFound,
                        format!(
                            "candidate_revision {candidate_revision} disappeared after discover materialization"
                        ),
                    )
                })?;
            let refresh_seq = artifacts
                .append_discover_refresh(&pool, candidate_revision)
                .await?;
            let route_diffs =
                route_diffs_from_previous_bundle(&pool, refresh_seq, &candidate.payload).await?;
            (route_artifact_count(&candidate.payload)?, route_diffs)
        }
        None => (0, vec![]),
    };
    let adoptable_count = match discovery_summary.adoptable_revision.as_deref() {
        Some(adoptable_revision) => {
            let adoptable = artifacts
                .get_adoptable_strategy_revision(&pool, adoptable_revision)
                .await?
                .ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::NotFound,
                        format!(
                            "adoptable_revision {adoptable_revision} disappeared after discover materialization"
                        ),
                    )
                })?;
            route_artifact_count(&adoptable.payload)?
        }
        None => 0,
    };

    Ok(DiscoverSummary {
        candidate_count,
        adoptable_count,
        recommended_adoptable_revision: discovery_summary.adoptable_revision,
        route_diffs,
        warnings: discovery_summary.warnings,
    })
}

fn render_discover_summary(summary: &DiscoverSummary) {
    println!("candidate_count = {}", summary.candidate_count);
    println!("adoptable_count = {}", summary.adoptable_count);
    println!(
        "recommended_adoptable_revision = {}",
        summary
            .recommended_adoptable_revision
            .as_deref()
            .unwrap_or("none")
    );
    println!("route_diff_count = {}", summary.route_diffs.len());
    if summary.route_diffs.is_empty() {
        println!("route_diff = none");
    } else {
        for route_diff in &summary.route_diffs {
            println!("route_diff = {route_diff}");
        }
    }
    println!("warning_count = {}", summary.warnings.len());
    if summary.warnings.is_empty() {
        println!("warning = none");
    } else {
        for warning in &summary.warnings {
            println!("warning = {warning}");
        }
    }
}

fn authoritative_candidate_publication(
    batch: &MetadataDiscoveryBatch,
    publication_id: &str,
) -> Result<CandidatePublication, io::Error> {
    let mut store = StateStore::default();
    for input in batch.inputs.iter().cloned() {
        apply_input(&mut store, input)?;
    }

    Ok(CandidatePublication::from_store(
        &store,
        CandidateProjectionReadiness::ready(publication_id),
    ))
}

fn apply_input(store: &mut StateStore, input: InputTaskEvent) -> Result<(), io::Error> {
    StateApplier::new(store)
        .apply(input.journal_seq, input.into_state_fact_input())
        .map(|_| ())
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))
}

fn full_set_basis_digest(
    source: &PolymarketSourceConfig,
    signer: &LocalSignerConfig,
) -> Result<String, io::Error> {
    let canonical = json!({
        "route": "full-set",
        "scope": "default",
        "policy_version": "full-set-route-policy-v1",
        "operator": {
            "address": signer.signer.address,
            "funder_address": signer.signer.funder_address,
            "signature_type": signer.signer.signature_type,
            "wallet_route": signer.signer.wallet_route,
        },
        "source": {
            "clob_host": source.clob_host.as_str(),
            "data_api_host": source.data_api_host.as_str(),
            "relayer_host": source.relayer_host.as_str(),
        }
    });
    let bytes = serde_json::to_vec(&canonical)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;

    Ok(format!("sha256:{:x}", Sha256::digest(bytes)))
}

fn route_artifact_count(payload: &serde_json::Value) -> Result<usize, io::Error> {
    Ok(route_artifacts(payload)?.len())
}

fn route_diffs(
    previous_payload: &serde_json::Value,
    current_payload: &serde_json::Value,
) -> Result<Vec<String>, io::Error> {
    let previous = route_artifact_map(previous_payload)?;
    let current = route_artifact_map(current_payload)?;
    let mut keys = BTreeSet::new();
    keys.extend(previous.keys().cloned());
    keys.extend(current.keys().cloned());

    let mut diffs = Vec::new();
    for key in keys {
        match (previous.get(&key), current.get(&key)) {
            (Some(previous_digest), Some(current_digest)) if previous_digest != current_digest => {
                diffs.push(format!(
                    "changed route={} scope={} previous={} current={}",
                    key.route, key.scope, previous_digest, current_digest
                ));
            }
            (None, Some(current_digest)) => diffs.push(format!(
                "added route={} scope={} current={}",
                key.route, key.scope, current_digest
            )),
            (Some(previous_digest), None) => diffs.push(format!(
                "removed route={} scope={} previous={}",
                key.route, key.scope, previous_digest
            )),
            _ => {}
        }
    }

    Ok(diffs)
}

async fn route_diffs_from_previous_bundle(
    pool: &sqlx::PgPool,
    refresh_seq: i64,
    current_payload: &serde_json::Value,
) -> Result<Vec<String>, io::Error> {
    StrategyControlArtifactRepo
        .previous_discover_refresh_candidate_set(pool, refresh_seq)
        .await
        .map_err(|error| io::Error::other(error.to_string()))?
        .map(|previous| route_diffs(&previous.payload, current_payload))
        .transpose()
        .map(Option::unwrap_or_default)
}

fn route_artifact_map(
    payload: &serde_json::Value,
) -> Result<BTreeMap<RouteArtifactKey, String>, io::Error> {
    route_artifacts(payload).map(|artifacts| {
        artifacts
            .into_iter()
            .map(|artifact| (artifact.key, artifact.semantic_digest))
            .collect()
    })
}

fn route_artifacts(payload: &serde_json::Value) -> Result<Vec<RouteArtifactSummary>, io::Error> {
    payload
        .get("route_artifacts")
        .and_then(serde_json::Value::as_array)
        .map(|artifacts| {
            artifacts
                .iter()
                .map(route_artifact_from_value)
                .collect::<Result<Vec<_>, io::Error>>()
        })
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "strategy payload is missing route_artifacts array",
            )
        })
        .and_then(|result| result)
}

fn route_artifact_from_value(value: &serde_json::Value) -> Result<RouteArtifactSummary, io::Error> {
    let route = value["key"]["route"].as_str().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "route artifact is missing key.route string",
        )
    })?;
    let scope = value["key"]["scope"].as_str().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "route artifact is missing key.scope string",
        )
    })?;
    let semantic_digest = value["semantic_digest"].as_str().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "route artifact is missing semantic_digest string",
        )
    })?;

    Ok(RouteArtifactSummary {
        key: RouteArtifactKey {
            route: route.to_owned(),
            scope: scope.to_owned(),
        },
        semantic_digest: semantic_digest.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    use async_trait::async_trait;
    use serde_json::json;
    use venue_polymarket::NegRiskMarketMetadata;

    use super::{fetch_discover_metadata, route_artifact_count, DiscoverMetadataFacade};

    #[test]
    fn route_artifact_count_rejects_missing_route_artifacts_array() {
        let err =
            route_artifact_count(&json!({})).expect_err("missing route_artifacts should fail");
        assert!(err.to_string().contains("route_artifacts array"), "{err}");
    }

    #[tokio::test]
    async fn discover_metadata_refresh_uses_injected_facade() {
        let call_count = Arc::new(AtomicUsize::new(0));
        let facade = ScriptedDiscoverMetadataFacade {
            call_count: call_count.clone(),
        };

        let rows = fetch_discover_metadata(&facade)
            .await
            .expect("scripted discover metadata should succeed");

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].event_family_id, "family-a");
        assert_eq!(call_count.load(Ordering::SeqCst), 1);
    }

    struct ScriptedDiscoverMetadataFacade {
        call_count: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl DiscoverMetadataFacade for ScriptedDiscoverMetadataFacade {
        async fn refresh_neg_risk_metadata(
            &self,
        ) -> Result<Vec<NegRiskMarketMetadata>, Box<dyn std::error::Error>> {
            self.call_count.fetch_add(1, Ordering::SeqCst);
            Ok(vec![NegRiskMarketMetadata {
                event_family_id: "family-a".to_owned(),
                event_id: "event-a".to_owned(),
                condition_id: "condition-a".to_owned(),
                token_id: "token-a".to_owned(),
                outcome_label: "Alpha".to_owned(),
                route: domain::MarketRoute::NegRisk,
                enable_neg_risk: Some(true),
                neg_risk_augmented: Some(false),
                neg_risk_variant: domain::NegRiskVariant::Standard,
                is_placeholder: false,
                is_other: false,
                discovery_revision: 1,
                metadata_snapshot_hash: "sha256:test".to_owned(),
            }])
        }
    }
}
