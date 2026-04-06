use std::path::Path;

use config_schema::{AppLiveConfigView, RuntimeModeToml};
use persistence::{connect_pool_from_env, RuntimeProgressRepo};

use crate::commands::targets::state::{
    load_target_control_plane_state, normalize_active_operator_strategy_revision,
};
use crate::startup::{resolve_startup_targets, ResolvedTargets};
use crate::NegRiskLiveTargetSet;

use super::report::{DoctorCheckStatus, DoctorReport};
use super::DoctorFailure;
use super::DoctorLiveContext;

pub fn evaluate(
    config_path: &Path,
    config: &AppLiveConfigView<'_>,
    live_context: Option<&DoctorLiveContext>,
    report: &mut DoctorReport,
) -> Result<Option<ResolvedTargets>, DoctorFailure> {
    match config.mode() {
        RuntimeModeToml::Paper => {
            report.push_check(
                "Target Source",
                DoctorCheckStatus::Skip,
                "startup target resolution not required in paper mode",
                "",
            );
            Ok(None)
        }
        RuntimeModeToml::Live => evaluate_live(config_path, config, live_context, report),
    }
}

fn evaluate_live(
    config_path: &Path,
    config: &AppLiveConfigView<'_>,
    live_context: Option<&DoctorLiveContext>,
    report: &mut DoctorReport,
) -> Result<Option<ResolvedTargets>, DoctorFailure> {
    if config.is_legacy_explicit_strategy_config() {
        let targets = NegRiskLiveTargetSet::try_from(config).map_err(|error| {
            report.push_check(
                "Target Source",
                DoctorCheckStatus::Fail,
                "TargetSourceError",
                error.to_string(),
            );
            DoctorFailure::new("TargetSourceError", error.to_string())
        })?;
        report.push_check(
            "Target Source",
            DoctorCheckStatus::Pass,
            "startup target resolution succeeded",
            "",
        );
        report.push_check(
            "Target Source",
            DoctorCheckStatus::Skip,
            "compatibility mode: control-plane checks stay read-only until app-live targets adopt --config <config> --adopt-compatibility",
            "",
        );
        return Ok(Some(ResolvedTargets {
            operator_target_revision: (!targets.is_empty()).then(|| targets.revision().to_owned()),
            targets,
        }));
    }

    if !config.has_adopted_strategy_source() {
        let message =
            "live target source must be adopted strategy source or compatibility-mode explicit targets";
        report.push_check(
            "Target Source",
            DoctorCheckStatus::Fail,
            "TargetSourceError",
            message,
        );
        return Err(DoctorFailure::new("TargetSourceError", message));
    }

    let live_context = live_context.expect("live context should exist for live target source");

    let pool = live_context.runtime.block_on(async {
        connect_pool_from_env().await.map_err(|error| {
            report.push_check(
                "Target Source",
                DoctorCheckStatus::Fail,
                "TargetSourceError",
                error.to_string(),
            );
            DoctorFailure::new("TargetSourceError", error.to_string())
        })
    })?;

    let resolved_targets = live_context
        .runtime
        .block_on(resolve_startup_targets(&pool, config))
        .map_err(|error| {
            report.push_check(
                "Target Source",
                DoctorCheckStatus::Fail,
                "TargetSourceError",
                error.to_string(),
            );
            DoctorFailure::new("TargetSourceError", error.to_string())
        })?;

    report.push_check(
        "Target Source",
        DoctorCheckStatus::Pass,
        "startup target resolution succeeded",
        "",
    );

    if config
        .target_source()
        .is_some_and(|target_source| target_source.is_adopted())
    {
        let target_state = live_context
            .runtime
            .block_on(load_target_control_plane_state(&pool, config_path))
            .map_err(|error| {
                report.push_check(
                    "Target Source",
                    DoctorCheckStatus::Fail,
                    "TargetSourceError",
                    error.to_string(),
                );
                DoctorFailure::new("TargetSourceError", error.to_string())
            })?;

        report.push_check(
            "Target Source",
            DoctorCheckStatus::Pass,
            format!(
                "configured operator strategy revision: {}",
                target_state
                    .configured_operator_strategy_revision
                    .as_deref()
                    .unwrap_or("unavailable")
            ),
            "",
        );
        report.push_check(
            "Target Source",
            DoctorCheckStatus::Pass,
            format!(
                "active operator strategy revision: {}",
                target_state
                    .active_operator_strategy_revision
                    .as_deref()
                    .unwrap_or("unavailable")
            ),
            "",
        );

        let restart_needed = match target_state.restart_needed {
            Some(true) => "restart needed",
            Some(false) => "restart not needed",
            None => "restart need unavailable",
        };
        report.push_check("Target Source", DoctorCheckStatus::Pass, restart_needed, "");
    } else {
        let (active_operator_target_revision, active_operator_strategy_revision) = live_context
            .runtime
            .block_on(async {
                RuntimeProgressRepo.current(&pool).await.map(|progress| {
                    progress
                        .map(|row| (row.operator_target_revision, row.operator_strategy_revision))
                        .unwrap_or((None, None))
                })
            })
            .map_err(|error| {
                report.push_check(
                    "Target Source",
                    DoctorCheckStatus::Fail,
                    "TargetSourceError",
                    error.to_string(),
                );
                DoctorFailure::new("TargetSourceError", error.to_string())
            })?;

        report.push_check(
            "Target Source",
            DoctorCheckStatus::Pass,
            format!(
                "configured operator strategy revision: {}",
                config.operator_strategy_revision().unwrap_or("unavailable")
            ),
            "",
        );
        let active_operator_strategy_revision = normalize_active_operator_strategy_revision(
            config.operator_strategy_revision(),
            active_operator_target_revision.as_deref(),
            active_operator_strategy_revision.as_deref(),
        );
        report.push_check(
            "Target Source",
            DoctorCheckStatus::Pass,
            format!(
                "active operator strategy revision: {}",
                active_operator_strategy_revision.as_deref().unwrap_or("unavailable")
            ),
            "",
        );

        let restart_needed = match (
            config.operator_strategy_revision(),
            active_operator_strategy_revision.as_deref(),
        ) {
            (Some(configured), Some(active)) => {
                if configured == active {
                    "restart not needed"
                } else {
                    "restart needed"
                }
            }
            _ => "restart need unavailable",
        };
        report.push_check("Target Source", DoctorCheckStatus::Pass, restart_needed, "");
    }

    Ok(Some(resolved_targets))
}
