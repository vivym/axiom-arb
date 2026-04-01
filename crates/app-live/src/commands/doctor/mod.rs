mod report;

use std::{error::Error, fmt};

use config_schema::{load_raw_config_from_path, RuntimeModeToml, ValidatedConfig};
use persistence::{connect_pool_from_env, RuntimeProgressRepo};

use crate::cli::DoctorArgs;
use crate::commands::targets::state::load_target_control_plane_state;
use crate::{load_real_user_shadow_smoke_config, startup::resolve_startup_targets};

use self::report::{DoctorCheckStatus, DoctorReport};

#[derive(Debug)]
struct DoctorFailure {
    category: &'static str,
    message: String,
}

impl DoctorFailure {
    fn new(category: &'static str, message: impl Into<String>) -> Self {
        Self {
            category,
            message: message.into(),
        }
    }
}

impl fmt::Display for DoctorFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.category, self.message)
    }
}

impl Error for DoctorFailure {}

pub fn execute(args: DoctorArgs) -> Result<(), Box<dyn Error>> {
    let mut report = DoctorReport::new();
    let result = execute_inner(&args, &mut report);
    report.render();
    result.map_err(|error| Box::new(error) as Box<dyn Error>)
}

fn execute_inner(args: &DoctorArgs, report: &mut DoctorReport) -> Result<(), DoctorFailure> {
    let raw = load_raw_config_from_path(&args.config).map_err(|error| {
        report.push_check(
            "Config",
            DoctorCheckStatus::Fail,
            "ConfigError",
            error.to_string(),
        );
        DoctorFailure::new("ConfigError", error.to_string())
    })?;
    let validated = ValidatedConfig::new(raw).map_err(|error| {
        report.push_check(
            "Config",
            DoctorCheckStatus::Fail,
            "ConfigError",
            error.to_string(),
        );
        DoctorFailure::new("ConfigError", error.to_string())
    })?;
    let config = validated.for_app_live().map_err(|error| {
        report.push_check(
            "Config",
            DoctorCheckStatus::Fail,
            "ConfigError",
            error.to_string(),
        );
        DoctorFailure::new("ConfigError", error.to_string())
    })?;

    report.push_check("Config", DoctorCheckStatus::Pass, "config parsed", "");

    match config.mode() {
        RuntimeModeToml::Paper => {
            report.push_check(
                "Connectivity",
                DoctorCheckStatus::Skip,
                "REST authentication not required in paper mode",
                "",
            );
            report.push_check(
                "Connectivity",
                DoctorCheckStatus::Skip,
                "target source resolution not required in paper mode",
                "",
            );
            Ok(())
        }
        RuntimeModeToml::Live => {
            if load_real_user_shadow_smoke_config(&config).is_ok()
                && config.real_user_shadow_smoke()
            {
                report.push_check(
                    "Connectivity",
                    DoctorCheckStatus::Pass,
                    "real-user shadow smoke guard configured",
                    "",
                );
            }

            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|error| DoctorFailure::new("RuntimeError", error.to_string()))?;
            let pool = runtime.block_on(async {
                connect_pool_from_env().await.map_err(|error| {
                    report.push_check(
                        "Connectivity",
                        DoctorCheckStatus::Fail,
                        "DatabaseError",
                        error.to_string(),
                    );
                    DoctorFailure::new("DatabaseError", error.to_string())
                })
            })?;
            let resolved_targets = runtime
                .block_on(resolve_startup_targets(&pool, &config))
                .map_err(|error| {
                    report.push_check(
                        "Connectivity",
                        DoctorCheckStatus::Fail,
                        "TargetSourceError",
                        error.to_string(),
                    );
                    DoctorFailure::new("TargetSourceError", error.to_string())
                })?;

            report.push_check(
                "Connectivity",
                DoctorCheckStatus::Pass,
                "target source resolved",
                "",
            );

            if config.target_source().is_some() {
                let target_state = runtime
                    .block_on(load_target_control_plane_state(&pool, &args.config))
                    .map_err(|error| {
                        report.push_check(
                            "Connectivity",
                            DoctorCheckStatus::Fail,
                            "TargetStateError",
                            error.to_string(),
                        );
                        DoctorFailure::new("TargetStateError", error.to_string())
                    })?;

                match target_state.restart_needed {
                    Some(true) => report.push_check(
                        "Connectivity",
                        DoctorCheckStatus::Pass,
                        "restart required for configured target revision to become active",
                        "",
                    ),
                    Some(false) => report.push_check(
                        "Connectivity",
                        DoctorCheckStatus::Pass,
                        "configured target revision matches active runtime state",
                        "",
                    ),
                    None => report.push_check(
                        "Connectivity",
                        DoctorCheckStatus::Pass,
                        "active runtime target state unavailable",
                        "",
                    ),
                }
            } else {
                let active_operator_target_revision = runtime
                    .block_on(RuntimeProgressRepo.current(&pool))
                    .map_err(|error| {
                        report.push_check(
                            "Connectivity",
                            DoctorCheckStatus::Fail,
                            "TargetStateError",
                            error.to_string(),
                        );
                        DoctorFailure::new("TargetStateError", error.to_string())
                    })?
                    .map(|row| {
                        row.operator_target_revision.ok_or_else(|| {
                            DoctorFailure::new(
                                "TargetStateError",
                                "runtime progress row exists without operator_target_revision anchor",
                            )
                        })
                    })
                    .transpose()?;

                match (
                    resolved_targets.operator_target_revision.as_deref(),
                    active_operator_target_revision.as_deref(),
                ) {
                    (Some(configured), Some(active)) if configured != active => report.push_check(
                        "Connectivity",
                        DoctorCheckStatus::Pass,
                        "restart required for configured target revision to become active",
                        "",
                    ),
                    (Some(_), Some(_)) => report.push_check(
                        "Connectivity",
                        DoctorCheckStatus::Pass,
                        "configured target revision matches active runtime state",
                        "",
                    ),
                    _ => report.push_check(
                        "Connectivity",
                        DoctorCheckStatus::Pass,
                        "active runtime target state unavailable",
                        "",
                    ),
                }
                report.push_check(
                    "Connectivity",
                    DoctorCheckStatus::Skip,
                    "control-plane checks not required for explicit targets",
                    "",
                );
            }
            Ok(())
        }
    }
}
