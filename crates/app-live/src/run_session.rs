use std::{error::Error, path::Path};

use chrono::Utc;
use config_schema::{AppLiveConfigView, RuntimeModeToml};
use persistence::{
    connect_pool_from_env, models::RunSessionState, RunSessionRepo, RunSessionRow,
    RuntimeProgressRepo,
};
use sha2::{Digest, Sha256};

use crate::NegRiskLiveTargetSet;

#[derive(Debug, Clone)]
pub struct RunSessionHandle {
    run_session_id: String,
    pub invoked_by: &'static str,
}

impl RunSessionHandle {
    pub fn create_starting(
        config_path: &Path,
        config: &AppLiveConfigView<'_>,
        invoked_by: &'static str,
    ) -> Result<Self, Box<dyn Error>> {
        let started_at = Utc::now();
        let row = build_starting_row(config_path, config, invoked_by, started_at)?;
        let run_session_id = row.run_session_id.clone();

        with_pool(|pool| async move {
            RunSessionRepo.create_starting(&pool, &row).await?;
            Ok::<_, Box<dyn Error>>(())
        })?;

        Ok(Self {
            run_session_id,
            invoked_by,
        })
    }

    pub fn run_session_id(&self) -> &str {
        &self.run_session_id
    }

    pub fn mark_running(&self) -> Result<(), Box<dyn Error>> {
        let seen_at = Utc::now();
        with_pool(|pool| async move {
            RunSessionRepo
                .mark_running(&pool, &self.run_session_id, seen_at)
                .await?;
            Ok::<_, Box<dyn Error>>(())
        })
    }

    pub fn refresh_last_seen(&self) -> Result<(), Box<dyn Error>> {
        let seen_at = Utc::now();
        with_pool(|pool| async move {
            RunSessionRepo
                .refresh_last_seen(&pool, &self.run_session_id, seen_at)
                .await?;
            Ok::<_, Box<dyn Error>>(())
        })
    }

    pub fn mark_exited(&self) -> Result<(), Box<dyn Error>> {
        let ended_at = Utc::now();
        with_pool(|pool| async move {
            RunSessionRepo
                .mark_exited(&pool, &self.run_session_id, ended_at, "success", None)
                .await?;
            Ok::<_, Box<dyn Error>>(())
        })
    }

    pub fn mark_failed(&self, reason: &str) -> Result<(), Box<dyn Error>> {
        let ended_at = Utc::now();
        with_pool(|pool| async move {
            RunSessionRepo
                .mark_failed(&pool, &self.run_session_id, ended_at, reason)
                .await?;
            Ok::<_, Box<dyn Error>>(())
        })
    }
}

fn build_starting_row(
    config_path: &Path,
    config: &AppLiveConfigView<'_>,
    invoked_by: &'static str,
    started_at: chrono::DateTime<Utc>,
) -> Result<RunSessionRow, Box<dyn Error>> {
    let config_path_string = config_path.display().to_string();
    let config_fingerprint = config_fingerprint(config_path)?;
    let current_active_revision = load_active_operator_target_revision_at_start()?;
    let (target_source_kind, startup_target_revision_at_start, configured_operator_target_revision) =
        startup_snapshot_target_source(config)?;
    let rollout_state_at_start = rollout_state_at_start(config);

    Ok(RunSessionRow {
        run_session_id: new_run_session_id(config_path, invoked_by, started_at),
        invoked_by: invoked_by.to_owned(),
        mode: match config.mode() {
            RuntimeModeToml::Paper => "paper".to_owned(),
            RuntimeModeToml::Live => "live".to_owned(),
        },
        state: RunSessionState::Starting,
        started_at,
        last_seen_at: started_at,
        ended_at: None,
        exit_status: None,
        exit_reason: None,
        config_path: config_path_string,
        config_fingerprint,
        target_source_kind,
        startup_target_revision_at_start,
        configured_operator_target_revision,
        active_operator_target_revision_at_start: current_active_revision,
        rollout_state_at_start,
        real_user_shadow_smoke: config.real_user_shadow_smoke(),
    })
}

fn startup_snapshot_target_source(
    config: &AppLiveConfigView<'_>,
) -> Result<(String, String, Option<String>), Box<dyn Error>> {
    if config.is_paper() {
        return Ok(("explicit".to_owned(), "paper-runtime".to_owned(), None));
    }

    if let Some(target_source) = config.target_source() {
        if target_source.is_adopted() {
            let configured = target_source
                .operator_target_revision()
                .map(str::to_owned)
                .ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "adopted run session snapshot requires operator_target_revision",
                    )
                })?;
            return Ok(("adopted".to_owned(), configured.clone(), Some(configured)));
        }
    }

    let targets = NegRiskLiveTargetSet::try_from(config)?;
    Ok(("explicit".to_owned(), targets.revision().to_owned(), None))
}

fn rollout_state_at_start(config: &AppLiveConfigView<'_>) -> Option<String> {
    if config.is_paper() {
        return None;
    }

    let Some(rollout) = config.negrisk_rollout() else {
        return Some("required".to_owned());
    };

    let ready = rollout.approved_families().iter().any(|family| {
        rollout
            .ready_families()
            .iter()
            .any(|ready_family| ready_family == family)
    });

    Some(if ready { "ready" } else { "required" }.to_owned())
}

fn load_active_operator_target_revision_at_start() -> Result<Option<String>, Box<dyn Error>> {
    with_pool(|pool| async move {
        Ok::<_, Box<dyn Error>>(
            RuntimeProgressRepo
                .current(&pool)
                .await?
                .and_then(|row| row.operator_target_revision),
        )
    })
}

fn config_fingerprint(config_path: &Path) -> Result<String, Box<dyn Error>> {
    let raw = std::fs::read(config_path)?;
    Ok(format!("{:x}", Sha256::digest(raw)))
}

fn new_run_session_id(
    config_path: &Path,
    invoked_by: &'static str,
    started_at: chrono::DateTime<Utc>,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(config_path.display().to_string().as_bytes());
    hasher.update(invoked_by.as_bytes());
    hasher.update(std::process::id().to_string().as_bytes());
    hasher.update(
        started_at
            .timestamp_nanos_opt()
            .unwrap_or_default()
            .to_string()
            .as_bytes(),
    );
    format!("run-session-{:x}", hasher.finalize())
}

fn with_pool<F, Fut, T>(f: F) -> Result<T, Box<dyn Error>>
where
    F: FnOnce(sqlx::PgPool) -> Fut,
    Fut: std::future::Future<Output = Result<T, Box<dyn Error>>>,
{
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async move {
        let pool = connect_pool_from_env().await?;
        f(pool).await
    })
}
