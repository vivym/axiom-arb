use std::{error::Error, io, path::Path};

use chrono::Utc;
use config_schema::{load_raw_config_from_path, RuntimeModeToml, ValidatedConfig};
use persistence::connect_pool_from_env;

use crate::{
    cli::DiscoverArgs,
    commands::targets::state::load_target_candidates_catalog,
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
    AppSupervisor::materialize_authoritative_discovery_batch(batch, &source_session_id)?;

    let pool = connect_pool_from_env().await?;
    let catalog = load_target_candidates_catalog(&pool).await?;
    let recommended_adoptable_revision = catalog
        .adoptable_revisions
        .first()
        .map(|row| row.adoptable_revision.clone());
    let candidate_count =
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM candidate_target_sets")
            .fetch_one(&pool)
            .await? as usize;
    let adoptable_count =
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM adoptable_target_revisions")
            .fetch_one(&pool)
            .await? as usize;

    Ok(DiscoverSummary {
        candidate_count,
        adoptable_count,
        recommended_adoptable_revision,
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
