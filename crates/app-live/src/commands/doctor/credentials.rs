use config_schema::{AppLiveConfigView, RuntimeModeToml};

use crate::{
    config::runtime_wallet_kind_requires_relayer, LocalAccountRuntimeConfig,
    LocalRelayerRuntimeConfig,
};

use super::report::{DoctorCheckStatus, DoctorReport};
use super::DoctorFailure;

pub fn evaluate(
    config: &AppLiveConfigView<'_>,
    report: &mut DoctorReport,
) -> Result<(), DoctorFailure> {
    match config.mode() {
        RuntimeModeToml::Paper => {
            report.push_check(
                "Credentials",
                DoctorCheckStatus::Skip,
                "long-lived account and relayer credentials not required in paper mode",
                "",
            );
            Ok(())
        }
        RuntimeModeToml::Live => evaluate_live(config, report),
    }
}

fn evaluate_live(
    config: &AppLiveConfigView<'_>,
    report: &mut DoctorReport,
) -> Result<(), DoctorFailure> {
    let mut checked_anything = false;

    if config.has_polymarket_account()
        || config.has_polymarket_signer()
        || config.polymarket_relayer_auth().is_some()
    {
        checked_anything = true;
        LocalAccountRuntimeConfig::try_from(config).map_err(|error| {
            report.push_check(
                "Credentials",
                DoctorCheckStatus::Fail,
                "CredentialError",
                error.to_string(),
            );
            DoctorFailure::new("CredentialError", error.to_string())
        })?;
        if runtime_wallet_kind_requires_relayer(config) {
            LocalRelayerRuntimeConfig::required_from(config).map_err(|error| {
                report.push_check(
                    "Credentials",
                    DoctorCheckStatus::Fail,
                    "CredentialError",
                    error.to_string(),
                );
                DoctorFailure::new("CredentialError", error.to_string())
            })?;
            report.push_check(
                "Credentials",
                DoctorCheckStatus::Pass,
                "long-lived account and relayer auth shapes validated",
                "",
            );
        } else {
            report.push_check(
                "Credentials",
                DoctorCheckStatus::Pass,
                "long-lived account and L2 auth shapes validated",
                "",
            );
        }
    }

    if !checked_anything {
        report.push_check(
            "Credentials",
            DoctorCheckStatus::Skip,
            "no long-lived account, relayer, or smoke credentials required for current startup authority",
            "",
        );
    }

    Ok(())
}
