use std::{error::Error, io, path::Path};

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
    config::PolymarketSourceConfig,
    source_tasks::build_polymarket_rest_client,
    task_groups::{MetadataDiscoveryBatch, MetadataTaskGroup},
    CandidateNotice, CandidateRestrictionTruth, DiscoverySupervisor, InputTaskEvent,
    LocalSignerConfig,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DiscoverSummary {
    candidate_count: usize,
    adoptable_count: usize,
    recommended_adoptable_revision: Option<String>,
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

    let rest = build_polymarket_rest_client(&source)?;
    println!("Fetching Polymarket metadata");
    let metadata_rows = rest.try_fetch_neg_risk_metadata_rows().await?;
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
    let candidate_count = match discovery_summary.candidate_revision.as_deref() {
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
            route_artifact_count(&candidate.payload)?
        }
        None => 0,
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
    payload
        .get("route_artifacts")
        .and_then(serde_json::Value::as_array)
        .map(Vec::len)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "strategy payload is missing route_artifacts array",
            )
        })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::route_artifact_count;

    #[test]
    fn route_artifact_count_rejects_missing_route_artifacts_array() {
        let err =
            route_artifact_count(&json!({})).expect_err("missing route_artifacts should fail");
        assert!(err.to_string().contains("route_artifacts array"), "{err}");
    }
}
