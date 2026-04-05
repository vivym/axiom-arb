#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusMode {
    Paper,
    RealUserShadowSmoke,
    Live,
}

impl StatusMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Paper => "paper",
            Self::RealUserShadowSmoke => "real-user shadow smoke",
            Self::Live => "live",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusReadiness {
    PaperReady,
    DiscoveryRequired,
    DiscoveryReadyNotAdoptable,
    AdoptableReady,
    RestartRequired,
    SmokeRolloutRequired,
    SmokeConfigReady,
    LiveRolloutRequired,
    LiveConfigReady,
    Blocked,
}

impl StatusReadiness {
    pub fn label(self) -> &'static str {
        match self {
            Self::PaperReady => "paper-ready",
            Self::DiscoveryRequired => "discovery-required",
            Self::DiscoveryReadyNotAdoptable => "discovery-ready-not-adoptable",
            Self::AdoptableReady => "adoptable-ready",
            Self::RestartRequired => "restart-required",
            Self::SmokeRolloutRequired => "smoke-rollout-required",
            Self::SmokeConfigReady => "smoke-config-ready",
            Self::LiveRolloutRequired => "live-rollout-required",
            Self::LiveConfigReady => "live-config-ready",
            Self::Blocked => "blocked",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusRolloutState {
    Required,
    Ready,
}

impl StatusRolloutState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Required => "required",
            Self::Ready => "ready",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusAction {
    RunDiscover,
    InspectDiscoveryReasons,
    ChooseAndAdoptRevision,
    RunDoctor,
    RunAppLiveApply,
    PerformControlledRestart,
    RunAppLiveRun,
    FixBlockingIssueAndRerunStatus,
    EnableSmokeRollout,
    EnableLiveRollout,
    MigrateLegacyExplicitTargets,
}

impl StatusAction {
    pub fn label(self) -> &'static str {
        match self {
            Self::RunDiscover => "run discover",
            Self::InspectDiscoveryReasons => "inspect discovery reasons",
            Self::ChooseAndAdoptRevision => "choose and adopt an adoptable revision",
            Self::RunDoctor => "run doctor",
            Self::RunAppLiveApply => "run app-live apply",
            Self::PerformControlledRestart => "perform controlled restart",
            Self::RunAppLiveRun => "run app-live run",
            Self::FixBlockingIssueAndRerunStatus => "fix blocking issue and rerun status",
            Self::EnableSmokeRollout => "enable smoke rollout",
            Self::EnableLiveRollout => "enable live rollout",
            Self::MigrateLegacyExplicitTargets => "migrate legacy explicit targets",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusTargetSource {
    LegacyExplicitTargets,
    AdoptedTargets,
}

impl StatusTargetSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::LegacyExplicitTargets => "legacy explicit targets",
            Self::AdoptedTargets => "adopted targets",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusDetails {
    pub configured_target: Option<String>,
    pub active_target: Option<String>,
    pub target_source: Option<StatusTargetSource>,
    pub rollout_state: Option<StatusRolloutState>,
    pub restart_needed: Option<bool>,
    pub relevant_run_session_id: Option<String>,
    pub relevant_run_state: Option<String>,
    pub relevant_run_started_at: Option<DateTime<Utc>>,
    pub relevant_startup_target_revision: Option<String>,
    pub conflicting_active_run_session_id: Option<String>,
    pub conflicting_active_run_state: Option<String>,
    pub conflicting_active_started_at: Option<DateTime<Utc>>,
    pub conflicting_active_startup_target_revision: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusSummary {
    pub mode: Option<StatusMode>,
    pub readiness: StatusReadiness,
    pub details: StatusDetails,
    pub actions: Vec<StatusAction>,
}
use chrono::{DateTime, Utc};
