pub mod bootstrap;
pub mod config;
pub mod dispatch;
pub mod input_tasks;
pub mod instrumentation;
pub mod negrisk_live;
pub mod runtime;
mod snapshot_meta;
pub mod supervisor;

pub use bootstrap::{BootstrapSource, StaticSnapshotSource};
pub use config::{
    load_neg_risk_live_targets, ConfigError, NegRiskFamilyLiveTarget, NegRiskMemberLiveTarget,
};
pub use dispatch::{DispatchLoop, DispatchSummary};
pub use input_tasks::InputTaskEvent;
pub use instrumentation::AppInstrumentation;
pub use negrisk_live::{NegRiskLiveArtifact, NegRiskLiveExecutionRecord};
pub use runtime::{
    run_live, run_live_instrumented, run_live_with_neg_risk_live_targets,
    run_live_with_neg_risk_live_targets_instrumented, run_paper, run_paper_instrumented,
    AppRunResult, AppRuntime, AppRuntimeMode, ParseAppRuntimeModeError,
};
pub use supervisor::{
    AppSupervisor, NegRiskLiveStateSource, NegRiskRolloutEvidence, SupervisorError,
    SupervisorSummary,
};
