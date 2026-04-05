use std::{error::Error, io, path::Path};

use chrono::Utc;
use config_schema::{load_raw_config_from_path, RuntimeModeToml, ValidatedConfig};
use persistence::{connect_pool_from_env, CandidateArtifactRepo};

use crate::{
    cli::DiscoverArgs, config::PolymarketSourceConfig, source_tasks::build_polymarket_rest_client,
    task_groups::MetadataTaskGroup, AppSupervisor, LocalSignerConfig,
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
    let _signer = LocalSignerConfig::try_from(&config)?;
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
    println!("Materializing discovery artifacts");
    let discovery_summary =
        AppSupervisor::materialize_authoritative_discovery_batch(batch, &source_session_id)?;
    tracing::debug!(
        source_session_id,
        candidate_revision = ?discovery_summary.latest_candidate_revision,
        adoptable_revision = ?discovery_summary.latest_adoptable_revision,
        "discover materialized authoritative discovery batch"
    );

    let pool = connect_pool_from_env().await?;
    let artifacts = CandidateArtifactRepo;
    let candidate_count = match discovery_summary.latest_candidate_revision.as_deref() {
        Some(candidate_revision) => {
            let candidate = artifacts
                .get_candidate_target_set(&pool, candidate_revision)
                .await?
                .ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::NotFound,
                        format!(
                            "candidate_revision {candidate_revision} disappeared after discover materialization"
                        ),
                    )
                })?;
            candidate_target_count(&candidate.payload)?
        }
        None => 0,
    };
    let adoptable_count = match discovery_summary.latest_adoptable_revision.as_deref() {
        Some(adoptable_revision) => {
            let adoptable = artifacts
                .get_adoptable_target_revision(&pool, adoptable_revision)
                .await?
                .ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::NotFound,
                        format!(
                            "adoptable_revision {adoptable_revision} disappeared after discover materialization"
                        ),
                    )
                })?;
            rendered_live_target_count(&adoptable.payload)?
        }
        None => 0,
    };

    Ok(DiscoverSummary {
        candidate_count,
        adoptable_count,
        recommended_adoptable_revision: discovery_summary.latest_adoptable_revision,
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

fn candidate_target_count(payload: &serde_json::Value) -> Result<usize, io::Error> {
    payload
        .get("targets")
        .and_then(serde_json::Value::as_array)
        .map(Vec::len)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "candidate payload is missing targets array",
            )
        })
}

fn rendered_live_target_count(payload: &serde_json::Value) -> Result<usize, io::Error> {
    payload
        .get("rendered_live_targets")
        .and_then(serde_json::Value::as_object)
        .map(|targets| targets.len())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "adoptable payload is missing rendered_live_targets object",
            )
        })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{candidate_target_count, rendered_live_target_count};

    #[test]
    fn candidate_target_count_rejects_missing_targets_array() {
        let err = candidate_target_count(&json!({})).expect_err("missing targets should fail");
        assert!(err.to_string().contains("targets array"), "{err}");
    }

    #[test]
    fn rendered_live_target_count_rejects_missing_rendered_live_targets_object() {
        let err = rendered_live_target_count(&json!({}))
            .expect_err("missing rendered_live_targets should fail");
        assert!(
            err.to_string().contains("rendered_live_targets object"),
            "{err}"
        );
    }
}
