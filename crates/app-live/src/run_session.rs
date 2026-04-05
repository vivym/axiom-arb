use std::{error::Error, path::Path};

use chrono::Utc;
use config_schema::{AppLiveConfigView, RuntimeModeToml};
use persistence::{
    connect_pool_from_env, models::RunSessionState, RunSessionRepo, RunSessionRow,
    RuntimeProgressRepo, RuntimeProgressRow,
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
    let revision_snapshot =
        startup_revision_snapshot(config, load_active_runtime_progress_at_start()?)?;
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
        target_source_kind: revision_snapshot.target_source_kind,
        startup_target_revision_at_start: revision_snapshot.startup_target_revision_at_start,
        configured_operator_target_revision: revision_snapshot.configured_operator_target_revision,
        active_operator_target_revision_at_start: revision_snapshot
            .active_operator_target_revision_at_start,
        configured_operator_strategy_revision: revision_snapshot
            .configured_operator_strategy_revision,
        active_operator_strategy_revision_at_start: revision_snapshot
            .active_operator_strategy_revision_at_start,
        rollout_state_at_start,
        real_user_shadow_smoke: config.real_user_shadow_smoke(),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StartupRevisionSnapshot {
    target_source_kind: String,
    startup_target_revision_at_start: String,
    configured_operator_target_revision: Option<String>,
    active_operator_target_revision_at_start: Option<String>,
    configured_operator_strategy_revision: Option<String>,
    active_operator_strategy_revision_at_start: Option<String>,
}

fn startup_revision_snapshot(
    config: &AppLiveConfigView<'_>,
    active_progress: Option<RuntimeProgressRow>,
) -> Result<StartupRevisionSnapshot, Box<dyn Error>> {
    let (target_source_kind, startup_target_revision_at_start, configured_operator_target_revision) =
        startup_snapshot_target_source(config)?;
    let active_operator_target_revision_at_start = active_progress
        .as_ref()
        .and_then(|row| row.operator_target_revision.clone());
    let active_operator_strategy_revision_at_start = active_progress.as_ref().and_then(|row| {
        row.operator_strategy_revision
            .clone()
            .or_else(|| row.operator_target_revision.clone())
    });

    Ok(StartupRevisionSnapshot {
        target_source_kind,
        startup_target_revision_at_start,
        configured_operator_strategy_revision: configured_operator_target_revision.clone(),
        configured_operator_target_revision,
        active_operator_target_revision_at_start,
        active_operator_strategy_revision_at_start,
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

fn load_active_runtime_progress_at_start() -> Result<Option<RuntimeProgressRow>, Box<dyn Error>> {
    with_pool(
        |pool| async move { Ok::<_, Box<dyn Error>>(RuntimeProgressRepo.current(&pool).await?) },
    )
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

#[cfg(test)]
mod tests {
    use config_schema::{load_raw_config_from_str, ValidatedConfig};
    use persistence::models::RuntimeProgressRow;

    use super::startup_revision_snapshot;

    #[test]
    fn startup_revision_snapshot_populates_neutral_strategy_fields_for_adopted_source() {
        let config = live_view(
            r#"
[negrisk.target_source]
source = "adopted"
operator_target_revision = "targets-rev-9"
"#,
        );

        let snapshot = startup_revision_snapshot(
            &config,
            Some(RuntimeProgressRow {
                last_journal_seq: 41,
                last_state_version: 7,
                last_snapshot_id: Some("snapshot-7".to_owned()),
                operator_target_revision: Some("targets-rev-active".to_owned()),
                operator_strategy_revision: Some("strategy-rev-active".to_owned()),
                active_run_session_id: Some("run-session-1".to_owned()),
            }),
        )
        .unwrap();

        assert_eq!(snapshot.target_source_kind, "adopted");
        assert_eq!(
            snapshot.configured_operator_target_revision.as_deref(),
            Some("targets-rev-9")
        );
        assert_eq!(
            snapshot.configured_operator_strategy_revision.as_deref(),
            Some("targets-rev-9")
        );
        assert_eq!(
            snapshot.active_operator_target_revision_at_start.as_deref(),
            Some("targets-rev-active")
        );
        assert_eq!(
            snapshot
                .active_operator_strategy_revision_at_start
                .as_deref(),
            Some("strategy-rev-active")
        );
    }

    fn live_view(extra: &str) -> config_schema::AppLiveConfigView<'static> {
        let raw = Box::leak(Box::new(
            load_raw_config_from_str(&format!(
                r#"
[runtime]
mode = "live"
real_user_shadow_smoke = false

[polymarket.account]
address = "0x1111111111111111111111111111111111111111"
funder_address = "0x2222222222222222222222222222222222222222"
signature_type = "eoa"
wallet_route = "eoa"
api_key = "poly-api-key"
secret = "poly-secret"
passphrase = "poly-passphrase"

[polymarket.relayer_auth]
kind = "relayer_api_key"
api_key = "relay-key"
address = "0x1111111111111111111111111111111111111111"

[negrisk.rollout]
approved_families = []
ready_families = []

{extra}
"#
            ))
            .expect("config should parse"),
        ));
        let validated = Box::leak(Box::new(
            ValidatedConfig::new(raw.clone()).expect("config should validate"),
        ));

        validated.for_app_live().expect("view should validate")
    }
}
