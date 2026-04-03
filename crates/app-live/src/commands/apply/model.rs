use config_schema::{AppLiveConfigView, RuntimeModeToml};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyStage {
    LoadReadiness,
    EnsureTargetAnchor,
    EnsureSmokeRollout,
    ConfirmManualRestartBoundary,
    RunPreflight,
    Ready,
    RunRuntime,
}

impl ApplyStage {
    pub fn label(self) -> &'static str {
        match self {
            Self::LoadReadiness => "load-readiness",
            Self::EnsureTargetAnchor => "ensure-target-anchor",
            Self::EnsureSmokeRollout => "ensure-smoke-rollout",
            Self::ConfirmManualRestartBoundary => "confirm-manual-restart-boundary",
            Self::RunPreflight => "run-preflight",
            Self::Ready => "ready",
            Self::RunRuntime => "run-runtime",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyScenario {
    Paper,
    Smoke,
    Live,
}

impl ApplyScenario {
    pub fn label(self) -> &'static str {
        match self {
            Self::Paper => "paper",
            Self::Smoke => "smoke",
            Self::Live => "live",
        }
    }

    pub fn from_config(config: &AppLiveConfigView<'_>) -> Self {
        match config.mode() {
            RuntimeModeToml::Paper => Self::Paper,
            RuntimeModeToml::Live if config.real_user_shadow_smoke() => Self::Smoke,
            RuntimeModeToml::Live => Self::Live,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyFailureKind {
    UnsupportedScenario(ApplyScenario),
}

impl ApplyFailureKind {
    pub fn unsupported_guidance(self) -> &'static str {
        match self {
            Self::UnsupportedScenario(ApplyScenario::Paper) => {
                "paper configs are not supported by apply yet; use bootstrap or run"
            }
            Self::UnsupportedScenario(ApplyScenario::Live) => {
                "live configs are not supported by apply yet; use status -> doctor -> run"
            }
            Self::UnsupportedScenario(ApplyScenario::Smoke) => {
                "smoke configs are supported by apply"
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ApplyStage;

    #[test]
    fn apply_stage_labels_use_operator_vocabulary() {
        assert_eq!(ApplyStage::LoadReadiness.label(), "load-readiness");
        assert_eq!(
            ApplyStage::EnsureTargetAnchor.label(),
            "ensure-target-anchor"
        );
        assert_eq!(
            ApplyStage::EnsureSmokeRollout.label(),
            "ensure-smoke-rollout"
        );
        assert_eq!(
            ApplyStage::ConfirmManualRestartBoundary.label(),
            "confirm-manual-restart-boundary"
        );
        assert_eq!(ApplyStage::RunPreflight.label(), "run-preflight");
        assert_eq!(ApplyStage::Ready.label(), "ready");
        assert_eq!(ApplyStage::RunRuntime.label(), "run-runtime");
    }
}
