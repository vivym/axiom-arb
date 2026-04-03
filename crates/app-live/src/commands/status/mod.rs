use std::{error::Error, path::Path};

pub mod evaluate;
pub mod model;

use crate::cli::StatusArgs;

pub fn execute(args: StatusArgs) -> Result<(), Box<dyn Error>> {
    let outcome = evaluate::evaluate(&args.config);
    render(&outcome, &args.config);
    Ok(())
}

fn render(outcome: &evaluate::StatusOutcome, config_path: &Path) {
    match outcome {
        evaluate::StatusOutcome::Summary(summary) => render_summary(summary, config_path),
        evaluate::StatusOutcome::Deferred(deferred) => {
            println!("Summary");
            println!("Mode: {}", deferred.mode.label());
            println!("Key Details");
            println!("Reason: {}", deferred.reason);
        }
    }
}

fn render_summary(summary: &model::StatusSummary, config_path: &Path) {
    println!("Summary");
    if let Some(mode) = summary.mode {
        println!("Mode: {}", mode.label());
    }
    println!("Readiness: {}", summary.readiness.label());
    println!("Key Details");
    let mut detail_lines = 0usize;
    if let Some(configured_target) = &summary.details.configured_target {
        println!("Configured target: {configured_target}");
        detail_lines += 1;
    }
    if let Some(active_target) = &summary.details.active_target {
        println!("Active target: {active_target}");
        detail_lines += 1;
    }
    if let Some(target_source) = summary.details.target_source {
        println!("Target source: {}", target_source.label());
        detail_lines += 1;
    }
    if let Some(rollout_state) = summary.details.rollout_state {
        println!("Rollout state: {}", rollout_state.label());
        detail_lines += 1;
    }
    if let Some(restart_needed) = summary.details.restart_needed {
        println!("Restart needed: {restart_needed}");
        detail_lines += 1;
    }
    if let Some(reason) = &summary.details.reason {
        println!("Reason: {reason}");
        detail_lines += 1;
    }
    if detail_lines == 0 {
        println!("No additional details");
    }
    println!("Next Actions");
    for action in &summary.actions {
        println!("Next: {}", render_action(action, config_path));
    }
}

pub(crate) fn render_action_template(action: &model::StatusAction) -> String {
    match action {
        model::StatusAction::RunAppLiveRun => "app-live run --config {config}".to_owned(),
        model::StatusAction::RunDoctor => "app-live doctor --config {config}".to_owned(),
        model::StatusAction::RunTargetsAdopt => "app-live targets adopt --config {config}".to_owned(),
        model::StatusAction::PerformControlledRestart => "perform controlled restart".to_owned(),
        model::StatusAction::FixBlockingIssueAndRerunStatus => {
            "fix the blocking issue, then rerun app-live status --config {config}".to_owned()
        }
        model::StatusAction::EnableSmokeRollout => "app-live bootstrap --config {config}".to_owned(),
        model::StatusAction::EnableLiveRollout =>
            "edit {config} and set [negrisk.rollout].approved_families and ready_families for adopted families".to_owned(),
        model::StatusAction::MigrateLegacyExplicitTargets => {
            "migrate to adopted target source or use lower-level commands".to_owned()
        }
    }
}

fn render_action(action: &model::StatusAction, config_path: &Path) -> String {
    let quoted_config_path = shell_quote(config_path.display().to_string());
    render_status_action_template(action, &quoted_config_path)
}

fn render_status_action_template(action: &model::StatusAction, quoted_config_path: &str) -> String {
    match action {
        model::StatusAction::EnableSmokeRollout => {
            "app-live apply --config {config}".replace("{config}", quoted_config_path)
        }
        _ => render_action_template(action).replace("{config}", quoted_config_path),
    }
}

fn shell_quote(value: String) -> String {
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '.' | '-' | '_'))
    {
        value
    } else {
        format!("'{}'", value.replace('\'', r"'\''"))
    }
}
