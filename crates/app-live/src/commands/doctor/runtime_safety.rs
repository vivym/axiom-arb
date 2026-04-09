use config_schema::{AppLiveConfigView, RuntimeModeToml};

use crate::load_real_user_shadow_smoke_config;

use super::report::{DoctorCheckStatus, DoctorReport};
use super::DoctorFailure;

pub fn evaluate(
    config: &AppLiveConfigView<'_>,
    report: &mut DoctorReport,
) -> Result<(), DoctorFailure> {
    match config.mode() {
        RuntimeModeToml::Paper => {
            report.push_check(
                "Runtime Safety",
                DoctorCheckStatus::Skip,
                "live runtime safety checks not required in paper mode",
                "",
            );
        }
        RuntimeModeToml::Live => {
            if config.real_user_shadow_smoke() {
                load_real_user_shadow_smoke_config(config).map_err(|error| {
                    report.push_check(
                        "Runtime Safety",
                        DoctorCheckStatus::Fail,
                        "RuntimeSafetyError",
                        error.to_string(),
                    );
                    DoctorFailure::new("RuntimeSafetyError", error.to_string())
                })?;
                report.push_check(
                    "Runtime Safety",
                    DoctorCheckStatus::Pass,
                    "smoke-safe startup configuration is valid",
                    "",
                );
                report.push_check(
                    "Runtime Safety",
                    DoctorCheckStatus::Pass,
                    "startup will request the shadow posture for risk-expanding routes",
                    "",
                );
            } else {
                report.push_check(
                    "Runtime Safety",
                    DoctorCheckStatus::Skip,
                    "shadow posture for risk-expanding routes not requested in non-smoke mode",
                    "",
                );
            }
        }
    }

    Ok(())
}
