use std::{collections::BTreeSet, error::Error, fmt, path::Path};

use config_schema::{load_raw_config_from_path, ValidatedConfig};
use persistence::connect_pool_from_env;

use crate::cli::{ApplyArgs, DoctorArgs};
use crate::commands::{
    doctor, run,
    status::{
        self,
        evaluate::StatusOutcome,
        model::{StatusReadiness, StatusRolloutState, StatusSummary},
    },
    targets::{
        adopt, config_file::rewrite_smoke_rollout_families, state::load_target_candidates_catalog,
    },
};
use crate::startup;
use crate::strategy_control::migrate_legacy_strategy_control;

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
    maybe_migrate_apply_config(&args.config)?;

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
        ApplyScenario::Live => execute_live_apply(&args.config, args.start),
        ApplyScenario::Smoke => execute_smoke_apply(&args.config, args.start),
    }
}

fn execute_live_apply(config_path: &Path, start_requested: bool) -> Result<(), Box<dyn Error>> {
    let mut prompt = ApplyPrompt::new();
    let summary = match status::evaluate::evaluate(config_path) {
        StatusOutcome::Summary(summary) => summary,
        StatusOutcome::Deferred(deferred) => {
            return Err(apply_failure(ApplyFailureKind::ReadinessError(
                deferred.reason,
            )))
        }
    };

    match summary.readiness {
        StatusReadiness::DiscoveryRequired => {
            return stop_live_apply(
                &summary,
                "Stop because discovery artifacts are still required before live apply can continue."
                    .to_owned(),
                "Blocked".to_owned(),
                status_next_actions(config_path, &summary),
                ApplyFailureKind::ReadinessError(summary.readiness.label().to_owned()),
            );
        }
        StatusReadiness::DiscoveryReadyNotAdoptable => {
            return stop_live_apply(
                &summary,
                "Stop because discovery has not produced an adoptable revision yet.".to_owned(),
                "Blocked".to_owned(),
                status_next_actions(config_path, &summary),
                ApplyFailureKind::ReadinessError(summary.readiness.label().to_owned()),
            );
        }
        StatusReadiness::AdoptableReady => {
            return stop_live_apply(
                &summary,
                "Stop because live target adoption is still required.".to_owned(),
                "Blocked".to_owned(),
                status_next_actions(config_path, &summary),
                ApplyFailureKind::ReadinessError(summary.readiness.label().to_owned()),
            );
        }
        StatusReadiness::LiveRolloutRequired => {
            return stop_live_apply(
                &summary,
                "Stop because live rollout preparation is still required.".to_owned(),
                "Blocked".to_owned(),
                status_next_actions(config_path, &summary),
                ApplyFailureKind::ReadinessError(summary.readiness.label().to_owned()),
            );
        }
        StatusReadiness::Blocked => {
            return stop_live_apply(
                &summary,
                "Stop because status reported a blocking issue.".to_owned(),
                "Blocked".to_owned(),
                status_next_actions(config_path, &summary),
                ApplyFailureKind::ReadinessError(summary.readiness.label().to_owned()),
            );
        }
        StatusReadiness::RestartRequired
            if summary.details.rollout_state == Some(StatusRolloutState::Required) =>
        {
            return stop_live_apply(
                &summary,
                "Stop because live rollout preparation is still required before restart.".to_owned(),
                "Blocked".to_owned(),
                vec![
                    format!(
                        "Edit {} and set [strategies.neg_risk.rollout].approved_scopes and ready_scopes for adopted scopes.",
                        output::quoted_config_path(config_path)
                    ),
                    format!(
                        "Re-run app-live apply --config {} after rollout preparation is complete.",
                        output::quoted_config_path(config_path)
                    ),
                ],
                ApplyFailureKind::ReadinessError("live-rollout-required".to_owned()),
            );
        }
        StatusReadiness::RestartRequired | StatusReadiness::LiveConfigReady => {}
        _ => {
            return Err(apply_failure(ApplyFailureKind::ReadinessError(
                summary.readiness.label().to_owned(),
            )))
        }
    }

    render_live_current_state(&summary);
    output::render_planned_actions(&planned_actions(&summary, start_requested));
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
        output::render_outcome(&live_ready_outcome(summary.readiness, false));
        output::render_next_actions(&ready_next_actions(config_path, &summary, false));
        return Ok(());
    }

    let blocking_run_session_id = live_start_blocking_run_session_id(&summary);
    if let Some(blocking_run_session_id) = blocking_run_session_id {
        let has_conflicting_active_session = active_conflicting_run_session_id(&summary).is_some();
        if summary.readiness == StatusReadiness::RestartRequired {
            if has_conflicting_active_session {
                output::render_execution_line(
                    "Stopping at the manual restart boundary because another run session is still active.",
                );
            } else {
                output::render_execution_line(
                    "Stopping at the manual restart boundary because the relevant run session is still active.",
                );
            }
        } else if has_conflicting_active_session {
            output::render_execution_line(
                "Stopping before starting the runtime because another run session is still active.",
            );
        } else {
            output::render_execution_line(
                "Stopping before starting the runtime because the relevant run session is still active.",
            );
        }
        output::render_outcome("Blocked");
        let mut next_actions = vec![format!(
            "Resolve the existing runtime outside apply, then rerun app-live apply --config {} --start.",
            output::quoted_config_path(config_path)
        )];
        if has_conflicting_active_session {
            next_actions.push(format!(
                "Conflicting active run session: {blocking_run_session_id}"
            ));
        }
        output::render_next_actions(&next_actions);
        return Err(apply_failure(ApplyFailureKind::ReadinessError(
            "resolve the existing runtime outside apply".to_owned(),
        )));
    }

    if summary.readiness == StatusReadiness::RestartRequired {
        if !prompt::stdin_is_interactive() {
            output::render_execution_line(
                "Stopping at the manual restart boundary because manual restart boundary requires interactive confirmation before foreground start.",
            );
            output::render_outcome("Blocked");
            output::render_next_actions(&ready_next_actions(config_path, &summary, true));
            return Err(apply_failure(ApplyFailureKind::Transition(
                model::ApplyStage::ConfirmManualRestartBoundary,
            )));
        }

        let configured_target = summary
            .details
            .configured_target
            .as_deref()
            .unwrap_or("unknown");
        match prompt::choose_restart_boundary_confirmation(
            &mut prompt,
            configured_target,
            summary.details.active_target.as_deref(),
        )? {
            RestartBoundarySelection::Confirm => {
                output::render_execution_line(
                    "Manual restart boundary confirmed. Starting runtime in the foreground.",
                );
            }
            RestartBoundarySelection::Decline => {
                output::render_outcome(&live_ready_outcome(summary.readiness, true));
                output::render_next_actions(&ready_next_actions(config_path, &summary, true));
                return Ok(());
            }
        }
    } else {
        output::render_execution_line("Starting runtime in the foreground.");
    }

    match run::run_from_config_path_with_invoked_by(config_path, "apply") {
        Ok(()) => {
            output::render_execution_line("Foreground runtime startup completed.");
            output::render_outcome("Started");
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

fn maybe_migrate_apply_config(config_path: &Path) -> Result<(), Box<dyn Error>> {
    let Ok(raw) = load_raw_config_from_path(config_path) else {
        return Ok(());
    };
    if raw.runtime.mode != config_schema::RuntimeModeToml::Live {
        return Ok(());
    }
    if !has_auto_migratable_legacy_control_plane(&raw) {
        return Ok(());
    }

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    runtime.block_on(async {
        let pool = connect_pool_from_env().await?;
        migrate_legacy_strategy_control(&pool, config_path)
            .await
            .map_err(|error| -> Box<dyn Error> { Box::new(error) })?;
        Ok::<_, Box<dyn Error>>(())
    })?;

    Ok(())
}

fn execute_smoke_apply(config_path: &Path, start_requested: bool) -> Result<(), Box<dyn Error>> {
    let mut prompt = ApplyPrompt::new();

    loop {
        match status::evaluate::evaluate(config_path) {
            StatusOutcome::Summary(summary) => {
                let failure = smoke_readiness_failure(config_path, &summary);

                if failure
                    == ApplyFailureKind::Transition(model::ApplyStage::ChooseAdoptableRevision)
                {
                    if !prompt::stdin_is_interactive() {
                        return Err(apply_failure(ApplyFailureKind::Transition(
                            model::ApplyStage::ChooseAdoptableRevision,
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
                                model::ApplyStage::ChooseAdoptableRevision,
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
        if !prompt::stdin_is_interactive() {
            output::render_outcome(
                "Runtime not started; manual restart boundary requires interactive confirmation before foreground start.",
            );
            output::render_next_actions(&ready_next_actions(config_path, summary, true));
            return Err(apply_failure(ApplyFailureKind::Transition(
                model::ApplyStage::ConfirmManualRestartBoundary,
            )));
        }
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

    match run::run_from_config_path_with_invoked_by(config_path, "apply") {
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
        .map(|adoptable| adoptable.adoptable_strategy_revision.clone())
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
        false,
    ))?;

    println!(
        "adopted adoptable revision {} as operator_strategy_revision {}",
        summary
            .selection
            .adoptable_revision
            .as_deref()
            .unwrap_or(adoptable_revision.as_str()),
        summary.selection.operator_strategy_revision
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

fn stop_live_apply(
    summary: &StatusSummary,
    planned_action: String,
    outcome: String,
    next_actions: Vec<String>,
    failure: ApplyFailureKind,
) -> Result<(), Box<dyn Error>> {
    render_live_current_state(summary);
    output::render_planned_actions(&[planned_action]);
    output::render_execution_header();
    output::render_execution_line("Stopping before doctor preflight.");
    output::render_outcome(&outcome);
    output::render_next_actions(&next_actions);
    Err(apply_failure(failure))
}

fn render_live_current_state(summary: &StatusSummary) {
    output::render_current_state(summary);
    if let Some(relevant_run_session_id) = &summary.details.relevant_run_session_id {
        println!("Relevant run session: {relevant_run_session_id}");
    }
    if let Some(relevant_run_state) = &summary.details.relevant_run_state {
        println!("Relevant run state: {relevant_run_state}");
    }
    if let Some(conflicting_active_run_session_id) =
        &summary.details.conflicting_active_run_session_id
    {
        println!("Conflicting active run session: {conflicting_active_run_session_id}");
    }
    if let Some(conflicting_active_run_state) = &summary.details.conflicting_active_run_state {
        println!("Conflicting active state: {conflicting_active_run_state}");
    }
}

fn status_next_actions(config_path: &Path, summary: &StatusSummary) -> Vec<String> {
    let quoted_config_path = output::quoted_config_path(config_path);
    summary
        .actions
        .iter()
        .map(|action| {
            status::render_action_template_with_mode(action, summary.mode)
                .replace("{config}", &quoted_config_path)
        })
        .collect()
}

fn smoke_readiness_failure(
    config_path: &Path,
    summary: &status::model::StatusSummary,
) -> ApplyFailureKind {
    let reason = summary
        .details
        .reason
        .clone()
        .unwrap_or_else(|| "blocked".to_owned());
    let quoted_config_path = output::quoted_config_path(config_path);

    if summary.readiness == status::model::StatusReadiness::RestartRequired
        && summary.details.rollout_state == Some(StatusRolloutState::Required)
    {
        return ApplyFailureKind::Transition(model::ApplyStage::EnsureSmokeRollout);
    }

    match summary.readiness {
        status::model::StatusReadiness::DiscoveryRequired => ApplyFailureKind::ReadinessError(
            format!(
                "{reason}; run app-live bootstrap --config {quoted_config_path} or app-live discover --config {quoted_config_path}"
            ),
        ),
        status::model::StatusReadiness::DiscoveryReadyNotAdoptable => {
            ApplyFailureKind::ReadinessError(format!(
                "{reason}; inspect discovery reasons with app-live targets candidates --config {quoted_config_path}"
            ))
        }
        _ => ApplyFailureKind::from_status_readiness(summary.readiness, reason),
    }
}

fn has_auto_migratable_legacy_control_plane(raw: &config_schema::RawAxiomConfig) -> bool {
    raw.negrisk
        .as_ref()
        .is_some_and(|negrisk| negrisk.targets.is_present() && !negrisk.targets.is_empty())
}

fn planned_actions(summary: &status::model::StatusSummary, start_requested: bool) -> Vec<String> {
    let mut actions = vec!["Run doctor preflight checks.".to_owned()];

    if summary.mode == Some(status::model::StatusMode::Live) {
        match (summary.readiness, start_requested) {
            (status::model::StatusReadiness::LiveConfigReady, false) => {
                actions.push("Stop at Ready to start without starting the runtime.".to_owned());
            }
            (status::model::StatusReadiness::LiveConfigReady, true)
                if active_conflicting_run_session_id(summary).is_some() =>
            {
                actions.push(
                    "Stop before starting the runtime because another run session is still active."
                        .to_owned(),
                );
                actions.push(
                    "Do not start the runtime while a conflicting active run session is still running."
                        .to_owned(),
                );
            }
            (status::model::StatusReadiness::LiveConfigReady, true)
                if matches!(
                    summary.details.relevant_run_state.as_deref(),
                    Some("starting" | "running")
                ) =>
            {
                actions.push(
                    "Stop before starting the runtime because the relevant run session is still active."
                        .to_owned(),
                );
                actions.push(
                    "Do not start the runtime while the current run session is still active."
                        .to_owned(),
                );
            }
            (status::model::StatusReadiness::LiveConfigReady, true) => {
                actions.push("Start the runtime in the foreground.".to_owned());
            }
            (status::model::StatusReadiness::RestartRequired, false) => {
                actions.push("Stop at Ready to start at the manual restart boundary.".to_owned());
            }
            (status::model::StatusReadiness::RestartRequired, true)
                if active_conflicting_run_session_id(summary).is_some() =>
            {
                actions.push(
                    "Stop at the manual restart boundary because another run session is still active."
                        .to_owned(),
                );
                actions.push(
                    "Do not start the runtime while a conflicting active run session is still running."
                        .to_owned(),
                );
            }
            (status::model::StatusReadiness::RestartRequired, true) => {
                actions.push(
                    "Require explicit confirmation at the manual restart boundary.".to_owned(),
                );
                actions.push("Start the runtime in the foreground only if confirmed.".to_owned());
            }
            _ => {}
        }

        return actions;
    }

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

fn live_ready_outcome(readiness: StatusReadiness, boundary_declined: bool) -> String {
    match (readiness, boundary_declined) {
        (StatusReadiness::RestartRequired, true) => {
            "Ready to start; restart confirmation declined at the manual restart boundary."
                .to_owned()
        }
        (StatusReadiness::RestartRequired, false) => {
            "Ready to start; apply is waiting at the manual restart boundary.".to_owned()
        }
        _ => "Ready to start".to_owned(),
    }
}

fn active_conflicting_run_session_id(summary: &StatusSummary) -> Option<&str> {
    match summary.details.conflicting_active_run_state.as_deref() {
        Some("starting" | "running") => {
            summary.details.conflicting_active_run_session_id.as_deref()
        }
        _ => None,
    }
}

fn live_start_blocking_run_session_id(summary: &StatusSummary) -> Option<&str> {
    active_conflicting_run_session_id(summary).or({
        match summary.details.relevant_run_state.as_deref() {
            Some("starting" | "running") => summary.details.relevant_run_session_id.as_deref(),
            _ => None,
        }
    })
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
    if summary.mode != Some(status::model::StatusMode::Live) {
        actions.push(format!(
            "Or run app-live run --config {quoted_config_path} through your normal operator workflow."
        ));
    }

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
