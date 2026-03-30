use std::{error::Error, fmt};

use config_schema::{load_raw_config_from_path, RuntimeModeToml, ValidatedConfig};
use persistence::connect_pool_from_env;

use crate::cli::DoctorArgs;
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
            runtime.block_on(async {
                let pool = connect_pool_from_env().await.map_err(|error| {
                    emit_check(CheckStatus::Fail, "DatabaseError", &error.to_string());
                    DoctorFailure::new("DatabaseError", error.to_string())
                })?;
                resolve_startup_targets(&pool, &config)
                    .await
                    .map_err(|error| {
                        emit_check(CheckStatus::Fail, "TargetSourceError", &error.to_string());
                        DoctorFailure::new("TargetSourceError", error.to_string())
                    })
            })?;

            emit_check(CheckStatus::Ok, "target source resolved", "");
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
