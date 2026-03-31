use std::{error::Error, fmt};

use config_schema::{load_raw_config_from_path, RuntimeModeToml, ValidatedConfig};
use persistence::{connect_pool_from_env, RuntimeProgressRepo};

use crate::cli::DoctorArgs;
use crate::commands::targets::state::load_target_control_plane_state;
use crate::{load_real_user_shadow_smoke_config, startup::resolve_startup_targets};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CheckStatus {
    Ok,
    Fail,
    Skip,
}

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
    let raw = load_raw_config_from_path(&args.config).map_err(|error| {
        emit_check(CheckStatus::Fail, "ConfigError", &error.to_string());
        DoctorFailure::new("ConfigError", error.to_string())
    })?;
    let validated = ValidatedConfig::new(raw).map_err(|error| {
        emit_check(CheckStatus::Fail, "ConfigError", &error.to_string());
        DoctorFailure::new("ConfigError", error.to_string())
    })?;
    let config = validated.for_app_live().map_err(|error| {
        emit_check(CheckStatus::Fail, "ConfigError", &error.to_string());
        DoctorFailure::new("ConfigError", error.to_string())
    })?;

    emit_check(CheckStatus::Ok, "config parsed", "");

    match config.mode() {
        RuntimeModeToml::Paper => {
            emit_check(
                CheckStatus::Skip,
                "REST authentication not required in paper mode",
                "",
            );
            emit_check(
                CheckStatus::Skip,
                "target source resolution not required in paper mode",
                "",
            );
            Ok(())
        }
        RuntimeModeToml::Live => {
            if load_real_user_shadow_smoke_config(&config).is_ok()
                && config.real_user_shadow_smoke()
            {
                emit_check(
                    CheckStatus::Ok,
                    "real-user shadow smoke guard configured",
                    "",
                );
            }

            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            let pool = runtime.block_on(async {
                connect_pool_from_env().await.map_err(|error| {
                    emit_check(CheckStatus::Fail, "DatabaseError", &error.to_string());
                    DoctorFailure::new("DatabaseError", error.to_string())
                })
            })?;
            let resolved_targets = runtime
                .block_on(resolve_startup_targets(&pool, &config))
                .map_err(|error| {
                    emit_check(CheckStatus::Fail, "TargetSourceError", &error.to_string());
                    DoctorFailure::new("TargetSourceError", error.to_string())
                })?;

            emit_check(CheckStatus::Ok, "target source resolved", "");

            if config.target_source().is_some() {
                let target_state = runtime
                    .block_on(load_target_control_plane_state(&pool, &args.config))
                    .map_err(|error| {
                        emit_check(CheckStatus::Fail, "TargetStateError", &error.to_string());
                        DoctorFailure::new("TargetStateError", error.to_string())
                    })?;

                match target_state.restart_needed {
                    Some(true) => emit_check(
                        CheckStatus::Ok,
                        "restart required for configured target revision to become active",
                        "",
                    ),
                    Some(false) => emit_check(
                        CheckStatus::Ok,
                        "configured target revision matches active runtime state",
                        "",
                    ),
                    None => emit_check(
                        CheckStatus::Ok,
                        "active runtime target state unavailable",
                        "",
                    ),
                }
            } else {
                let active_operator_target_revision = runtime
                    .block_on(RuntimeProgressRepo.current(&pool))
                    .map_err(|error| {
                        emit_check(CheckStatus::Fail, "TargetStateError", &error.to_string());
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
                    (Some(configured), Some(active)) if configured != active => emit_check(
                        CheckStatus::Ok,
                        "restart required for configured target revision to become active",
                        "",
                    ),
                    (Some(_), Some(_)) => emit_check(
                        CheckStatus::Ok,
                        "configured target revision matches active runtime state",
                        "",
                    ),
                    _ => emit_check(
                        CheckStatus::Ok,
                        "active runtime target state unavailable",
                        "",
                    ),
                }
                emit_check(
                    CheckStatus::Skip,
                    "control-plane checks not required for explicit targets",
                    "",
                );
            }
            Ok(())
        }
    }
}

fn emit_check(status: CheckStatus, label: &str, detail: &str) {
    let marker = match status {
        CheckStatus::Ok => "OK",
        CheckStatus::Fail => "FAIL",
        CheckStatus::Skip => "SKIP",
    };

    if detail.is_empty() {
        println!("[{marker}] {label}");
    } else {
        println!("[{marker}] {label}: {detail}");
    }
}
