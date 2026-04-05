use std::{error::Error, io, path::Path};

use chrono::Utc;
use config_schema::{load_raw_config_from_path, RuntimeModeToml, ValidatedConfig};
use persistence::{connect_pool_from_env, CandidateArtifactRepo};

use crate::{
    cli::DiscoverArgs,
    load_real_user_shadow_smoke_config, source_tasks::build_polymarket_rest_client,
    task_groups::MetadataTaskGroup, AppSupervisor, LocalSignerConfig,
};

#[derive(Debug, Clone, PartialEq, Eq)]
struct DiscoverSummary {
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

async fn run_discover_from_config(config_path: &Path) -> Result<DiscoverSummary, Box<dyn Error>> {
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
    let smoke = load_real_user_shadow_smoke_config(&config)?.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "discover requires a real-user shadow smoke config",
        )
    })?;
    let _signer = LocalSignerConfig::try_from(&config)?;

    let rest = build_polymarket_rest_client(&smoke.source_config);
    let metadata_rows = rest.try_fetch_neg_risk_metadata_rows().await?;
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
    let discovery_summary =
        AppSupervisor::materialize_authoritative_discovery_batch(batch, &source_session_id)?;

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
            candidate_target_count(&candidate.payload)
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
            rendered_live_target_count(&adoptable.payload)
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

fn candidate_target_count(payload: &serde_json::Value) -> usize {
    payload
        .get("targets")
        .and_then(serde_json::Value::as_array)
        .map_or(0, Vec::len)
}

fn rendered_live_target_count(payload: &serde_json::Value) -> usize {
    payload
        .get("rendered_live_targets")
        .and_then(serde_json::Value::as_object)
        .map_or(0, |targets| targets.len())
}
