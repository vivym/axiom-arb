use std::{collections::BTreeSet, error::Error, fmt, path::Path};

use config_schema::{load_raw_config_from_path, ValidatedConfig};
use persistence::connect_pool_from_env;

use crate::cli::{ApplyArgs, DoctorArgs};
use crate::commands::{
    doctor, run,
    status::{self, evaluate::StatusOutcome, model::StatusRolloutState},
    targets::{
        adopt, config_file::rewrite_smoke_rollout_families, state::load_target_candidates_catalog,
    },
};
use crate::startup;

pub mod model;
mod output;
mod prompt;

use self::model::{ApplyFailureKind, ApplyScenario, ApplyUnsupportedScenario};
use self::prompt::{
    ApplyPrompt, InlineSmokeRolloutSelection, InlineTargetAdoptionSelection,
    RestartBoundarySelection,
};

enum InlineTargetAdoptionOutcome {
    Adopted,
    Cancelled,
    Unavailable,
}

enum InlineSmokeRolloutOutcome {
    Enabled,
    Declined,
}

#[derive(Debug)]
struct ApplyError {
    kind: ApplyFailureKind,
}

impl fmt::Display for ApplyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.kind.guidance())
    }
}

impl Error for ApplyError {}

pub fn execute(args: ApplyArgs) -> Result<(), Box<dyn Error>> {
    let raw = match load_raw_config_from_path(&args.config) {
        Ok(raw) => raw,
        Err(error) => {
            return Err(apply_failure(ApplyFailureKind::ReadinessError(
                error.to_string(),
            )))
        }
    };
    let validated = match ValidatedConfig::new(raw) {
        Ok(validated) => validated,
        Err(error) => {
            return Err(apply_failure(ApplyFailureKind::ReadinessError(
                error.to_string(),
            )))
        }
    };
    let config = match validated.for_app_live() {
        Ok(config) => config,
        Err(error) => {
            return Err(apply_failure(ApplyFailureKind::ReadinessError(
                error.to_string(),
            )))
        }
    };
    let scenario = ApplyScenario::from_config(&config);

    match scenario {
        ApplyScenario::Paper => Err(apply_failure(ApplyFailureKind::UnsupportedScenario(
            ApplyUnsupportedScenario::Paper,
        ))),
        ApplyScenario::Live => Err(apply_failure(ApplyFailureKind::UnsupportedScenario(
            ApplyUnsupportedScenario::Live,
        ))),
        ApplyScenario::Smoke => execute_smoke_apply(&args.config, args.start),
    }
}

fn execute_smoke_apply(config_path: &Path, start_requested: bool) -> Result<(), Box<dyn Error>> {
    let mut prompt = ApplyPrompt::new();

    loop {
        match status::evaluate::evaluate(config_path) {
            StatusOutcome::Summary(summary) => {
                let reason = summary
                    .details
                    .reason
                    .clone()
                    .unwrap_or_else(|| "blocked".to_owned());
                let failure = if summary.readiness
                    == status::model::StatusReadiness::RestartRequired
                    && summary.details.rollout_state == Some(StatusRolloutState::Required)
                {
                    ApplyFailureKind::Transition(model::ApplyStage::EnsureSmokeRollout)
                } else {
                    ApplyFailureKind::from_status_readiness(summary.readiness, reason)
                };

                if failure == ApplyFailureKind::Transition(model::ApplyStage::EnsureTargetAnchor) {
                    if !prompt::stdin_is_interactive() {
                        return Err(apply_failure(ApplyFailureKind::Transition(
                            model::ApplyStage::EnsureTargetAnchor,
                        )));
                    }

                    match inline_target_adoption(config_path, &mut prompt) {
                        Ok(InlineTargetAdoptionOutcome::Adopted) => continue,
                        Ok(InlineTargetAdoptionOutcome::Cancelled) => {
                            return Err(apply_failure(ApplyFailureKind::ReadinessError(
                                "inline target adoption cancelled".to_owned(),
                            )))
                        }
                        Ok(InlineTargetAdoptionOutcome::Unavailable) => {
                            return Err(apply_failure(ApplyFailureKind::Transition(
                                model::ApplyStage::EnsureTargetAnchor,
                            )))
                        }
                        Err(error) => {
                            return Err(apply_failure(ApplyFailureKind::ReadinessError(
                                error.to_string(),
                            )))
                        }
                    }
                }

                if failure == ApplyFailureKind::Transition(model::ApplyStage::EnsureSmokeRollout) {
                    if !prompt::stdin_is_interactive() {
                        return Err(apply_failure(ApplyFailureKind::Transition(
                            model::ApplyStage::EnsureSmokeRollout,
                        )));
                    }

                    match inline_smoke_rollout_enablement(config_path, &mut prompt) {
                        Ok(InlineSmokeRolloutOutcome::Enabled) => continue,
                        Ok(InlineSmokeRolloutOutcome::Declined) => {
                            return Err(apply_failure(ApplyFailureKind::ReadinessError(
                                "inline smoke rollout enablement declined".to_owned(),
                            )))
                        }
                        Err(error) => {
                            return Err(apply_failure(ApplyFailureKind::ReadinessError(
                                error.to_string(),
                            )))
                        }
                    }
                }

                match summary.readiness {
                    status::model::StatusReadiness::SmokeConfigReady
                    | status::model::StatusReadiness::RestartRequired => {
                        return finalize_smoke_apply(
                            config_path,
                            start_requested,
                            &summary,
                            &mut prompt,
                        )
                    }
                    _ => return Err(apply_failure(failure)),
                }
            }
            StatusOutcome::Deferred(deferred) => {
                return Err(apply_failure(ApplyFailureKind::ReadinessError(
                    deferred.reason,
                )))
            }
        }
    }
}

fn finalize_smoke_apply(
    config_path: &Path,
    start_requested: bool,
    summary: &status::model::StatusSummary,
    prompt: &mut ApplyPrompt,
) -> Result<(), Box<dyn Error>> {
    output::render_current_state(summary);
    output::render_planned_actions(&planned_actions(summary, start_requested));
    output::render_execution_header();
    output::render_execution_line("Running doctor preflight.");

    let doctor_args = DoctorArgs {
        config: config_path.to_path_buf(),
    };
    let doctor_execution = doctor::run_report(&doctor_args);
    doctor_execution.render();
    if let Err(error) = doctor_execution.into_result() {
        output::render_outcome("Apply stopped because doctor preflight failed.");
        output::render_next_actions(&doctor_failure_next_actions(config_path));
        return Err(error);
    }

    if !start_requested {
        output::render_outcome(&ready_outcome(summary));
        output::render_next_actions(&ready_next_actions(config_path, summary, false));
        return Ok(());
    }

    if summary.readiness == status::model::StatusReadiness::RestartRequired {
        let configured_target = summary
            .details
            .configured_target
            .as_deref()
            .unwrap_or("unknown");
        match prompt::choose_restart_boundary_confirmation(
            prompt,
            configured_target,
            summary.details.active_target.as_deref(),
        )? {
            RestartBoundarySelection::Confirm => {
                output::render_execution_line(
                    "Manual restart boundary confirmed. Starting runtime in the foreground.",
                );
            }
            RestartBoundarySelection::Decline => {
                output::render_outcome(
                    "Runtime not started; restart confirmation declined at the manual restart boundary.",
                );
                output::render_next_actions(&ready_next_actions(config_path, summary, true));
                return Ok(());
            }
        }
    } else {
        output::render_execution_line("Starting runtime in the foreground.");
    }

    match run::run_from_config_path(config_path) {
        Ok(()) => {
            output::render_outcome("Foreground runtime startup completed.");
            output::render_next_actions(&started_next_actions(config_path));
            Ok(())
        }
        Err(error) => {
            output::render_outcome("Foreground runtime startup failed.");
            output::render_next_actions(&run_failure_next_actions(config_path));
            Err(error)
        }
    }
}

fn inline_target_adoption(
    config_path: &Path,
    prompt: &mut ApplyPrompt,
) -> Result<InlineTargetAdoptionOutcome, Box<dyn Error>> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    let (pool, catalog) = runtime.block_on(async {
        let pool = connect_pool_from_env().await?;
        let catalog = load_target_candidates_catalog(&pool).await?;
        Ok::<_, Box<dyn Error>>((pool, catalog))
    })?;

    let adoptable_revisions = catalog
        .adoptable_revisions
        .iter()
        .map(|adoptable| adoptable.adoptable_revision.clone())
        .collect::<Vec<_>>();
    if adoptable_revisions.is_empty() {
        return Ok(InlineTargetAdoptionOutcome::Unavailable);
    }

    let selection = prompt::choose_adoptable_revision(prompt, &adoptable_revisions)?;
    let adoptable_revision = match selection {
        InlineTargetAdoptionSelection::AdoptableRevision(revision) => revision,
        InlineTargetAdoptionSelection::Cancel => return Ok(InlineTargetAdoptionOutcome::Cancelled),
    };

    let summary = runtime.block_on(adopt::adopt_selected_revision(
        &pool,
        config_path,
        None,
        Some(adoptable_revision.as_str()),
    ))?;

    println!(
        "adopted adoptable revision {} as operator_target_revision {}",
        summary
            .selection
            .adoptable_revision
            .as_deref()
            .unwrap_or(adoptable_revision.as_str()),
        summary.selection.operator_target_revision
    );

    Ok(InlineTargetAdoptionOutcome::Adopted)
}

fn inline_smoke_rollout_enablement(
    config_path: &Path,
    prompt: &mut ApplyPrompt,
) -> Result<InlineSmokeRolloutOutcome, Box<dyn Error>> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    let family_ids = runtime.block_on(adopted_smoke_family_ids(config_path))?;
    let selection = prompt::choose_smoke_rollout_confirmation(prompt, &family_ids)?;

    match selection {
        InlineSmokeRolloutSelection::Confirm => {
            rewrite_smoke_rollout_families(config_path, &family_ids)?;
            Ok(InlineSmokeRolloutOutcome::Enabled)
        }
        InlineSmokeRolloutSelection::Decline => Ok(InlineSmokeRolloutOutcome::Declined),
    }
}

async fn adopted_smoke_family_ids(config_path: &Path) -> Result<Vec<String>, Box<dyn Error>> {
    let raw = load_raw_config_from_path(config_path)?;
    let validated = ValidatedConfig::new(raw)?;
    let config = validated.for_app_live()?;
    let pool = connect_pool_from_env().await?;
    let resolved = startup::resolve_startup_targets(&pool, &config).await?;
    let family_ids = resolved
        .targets
        .targets()
        .keys()
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    if family_ids.is_empty() {
        return Err("apply could not derive any adopted smoke families".into());
    }
    Ok(family_ids)
}

fn apply_failure(kind: ApplyFailureKind) -> Box<dyn Error> {
    let error = ApplyError { kind };
    eprintln!("{error}");
    Box::new(error)
}

fn planned_actions(summary: &status::model::StatusSummary, start_requested: bool) -> Vec<String> {
    let mut actions = vec!["Run doctor preflight checks.".to_owned()];

    match (summary.readiness, start_requested) {
        (status::model::StatusReadiness::SmokeConfigReady, false) => {
            actions.push("Stop at ready without starting the runtime.".to_owned());
        }
        (status::model::StatusReadiness::SmokeConfigReady, true) => {
            actions.push("Start the runtime in the foreground.".to_owned());
        }
        (status::model::StatusReadiness::RestartRequired, false) => {
            actions.push(
                "Stop at the manual restart boundary without starting the runtime.".to_owned(),
            );
        }
        (status::model::StatusReadiness::RestartRequired, true) => {
            actions
                .push("Require explicit confirmation at the manual restart boundary.".to_owned());
            actions.push("Start the runtime in the foreground only if confirmed.".to_owned());
        }
        _ => {}
    }

    actions
}

fn ready_outcome(summary: &status::model::StatusSummary) -> String {
    if summary.readiness == status::model::StatusReadiness::RestartRequired {
        "Runtime not started; apply reached ready state at the manual restart boundary.".to_owned()
    } else {
        "Runtime not started; apply reached ready state.".to_owned()
    }
}

fn doctor_failure_next_actions(config_path: &Path) -> Vec<String> {
    let quoted_config_path = output::quoted_config_path(config_path);
    vec![
        "Fix the reported doctor issue.".to_owned(),
        format!("app-live apply --config {quoted_config_path}"),
    ]
}

fn ready_next_actions(
    config_path: &Path,
    summary: &status::model::StatusSummary,
    start_requested: bool,
) -> Vec<String> {
    let quoted_config_path = output::quoted_config_path(config_path);
    let mut actions = Vec::new();

    if summary.readiness == status::model::StatusReadiness::RestartRequired {
        actions.push(
            "Respect the manual restart boundary; apply will not stop or replace an existing daemon."
                .to_owned(),
        );
    }

    if start_requested {
        actions.push(format!(
            "Re-run app-live apply --config {quoted_config_path} --start when you are ready to continue."
        ));
    } else {
        actions.push(format!(
            "Re-run app-live apply --config {quoted_config_path} --start to continue in the foreground."
        ));
    }
    actions.push(format!(
        "Or run app-live run --config {quoted_config_path} through your normal operator workflow."
    ));

    actions
}

fn started_next_actions(config_path: &Path) -> Vec<String> {
    let quoted_config_path = output::quoted_config_path(config_path);
    vec![format!(
        "Use app-live status --config {quoted_config_path} to verify the active target and rollout state."
    )]
}

fn run_failure_next_actions(config_path: &Path) -> Vec<String> {
    let quoted_config_path = output::quoted_config_path(config_path);
    vec![
        format!("Review the runtime error, then retry app-live apply --config {quoted_config_path} --start."),
        format!("Use app-live status --config {quoted_config_path} to re-check readiness before retrying."),
    ]
}
