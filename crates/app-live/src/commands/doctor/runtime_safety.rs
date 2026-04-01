use config_schema::{AppLiveConfigView, RuntimeModeToml};

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
                report.push_check(
                    "Runtime Safety",
                    DoctorCheckStatus::Pass,
                    "smoke-safe startup configuration is valid",
                    "",
                );
                report.push_check(
                    "Runtime Safety",
                    DoctorCheckStatus::Pass,
                    "startup will request the shadow-only neg-risk posture",
                    "",
                );
            } else {
                report.push_check(
                    "Runtime Safety",
                    DoctorCheckStatus::Skip,
                    "shadow-only neg-risk posture not requested in non-smoke mode",
                    "",
                );
            }
        }
    }

    Ok(())
}
