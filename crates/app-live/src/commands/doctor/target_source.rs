use std::path::Path;

use config_schema::{AppLiveConfigView, RuntimeModeToml};
use persistence::connect_pool_from_env;

use crate::commands::targets::state::load_target_control_plane_state;
use crate::startup::resolve_startup_targets;
use crate::NegRiskLiveTargetSet;

use super::report::{DoctorCheckStatus, DoctorReport};
use super::DoctorFailure;

pub fn evaluate(
    config_path: &Path,
    config: &AppLiveConfigView<'_>,
    report: &mut DoctorReport,
) -> Result<(), DoctorFailure> {
    match config.mode() {
        RuntimeModeToml::Paper => {
            report.push_check(
                "Target Source",
                DoctorCheckStatus::Skip,
                "startup target resolution not required in paper mode",
                "",
            );
            Ok(())
        }
        RuntimeModeToml::Live => evaluate_live(config_path, config, report),
    }
}

fn evaluate_live(
    config_path: &Path,
    config: &AppLiveConfigView<'_>,
    report: &mut DoctorReport,
) -> Result<(), DoctorFailure> {
    if config.target_source().is_none() {
        NegRiskLiveTargetSet::try_from(config).map_err(|error| {
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
            "control-plane checks not required for explicit targets",
            "",
        );
        return Ok(());
    }

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| {
            report.push_check(
                "Target Source",
                DoctorCheckStatus::Fail,
                "TargetSourceError",
                error.to_string(),
            );
            DoctorFailure::new("TargetSourceError", error.to_string())
        })?;

    let pool = runtime.block_on(async {
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

    runtime
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

    if let Some(target_source) = config.target_source() {
        if target_source.is_adopted() {
            let target_state = runtime
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
                    "configured operator target revision: {}",
                    target_state
                        .configured_operator_target_revision
                        .as_deref()
                        .unwrap_or("unavailable")
                ),
                "",
            );
            report.push_check(
                "Target Source",
                DoctorCheckStatus::Pass,
                format!(
                    "active operator target revision: {}",
                    target_state
                        .active_operator_target_revision
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
        }
    } else {
        unreachable!("explicit target branch should return early");
    }

    Ok(())
}
