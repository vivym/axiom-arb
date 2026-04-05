use std::path::Path;

use crate::commands::status::model::StatusSummary;

pub(crate) fn render_current_state(summary: &StatusSummary) {
    println!("Current State");
    if let Some(mode) = summary.mode {
        println!("Mode: {}", mode.label());
    }
    println!("Readiness: {}", summary.readiness.label());

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
    if let Some(relevant_run_session_id) = &summary.details.relevant_run_session_id {
        println!("Relevant run session: {relevant_run_session_id}");
        detail_lines += 1;
    }
    if let Some(relevant_run_state) = &summary.details.relevant_run_state {
        println!("Relevant run state: {relevant_run_state}");
        detail_lines += 1;
    }
    if let Some(relevant_run_started_at) = summary.details.relevant_run_started_at {
        println!("Relevant run started at: {}", relevant_run_started_at.to_rfc3339());
        detail_lines += 1;
    }
    if let Some(relevant_startup_target_revision) = &summary.details.relevant_startup_target_revision
    {
        println!("Relevant startup target: {relevant_startup_target_revision}");
        detail_lines += 1;
    }
    if let Some(conflicting_active_run_session_id) = &summary.details.conflicting_active_run_session_id
    {
        println!("Conflicting active run session: {conflicting_active_run_session_id}");
        detail_lines += 1;
    }
    if let Some(conflicting_active_run_state) = &summary.details.conflicting_active_run_state {
        println!("Conflicting active state: {conflicting_active_run_state}");
        detail_lines += 1;
    }
    if let Some(conflicting_active_started_at) = summary.details.conflicting_active_started_at {
        println!(
            "Conflicting active started at: {}",
            conflicting_active_started_at.to_rfc3339()
        );
        detail_lines += 1;
    }
    if let Some(conflicting_active_startup_target_revision) =
        &summary.details.conflicting_active_startup_target_revision
    {
        println!("Conflicting active startup target: {conflicting_active_startup_target_revision}");
        detail_lines += 1;
    }
    if let Some(reason) = &summary.details.reason {
        println!("Reason: {reason}");
        detail_lines += 1;
    }
    if detail_lines == 0 {
        println!("No additional details");
    }
}

pub(crate) fn render_planned_actions(lines: &[String]) {
    render_section("Planned Actions", lines);
}

pub(crate) fn render_execution_header() {
    println!("Execution");
}

pub(crate) fn render_execution_line(line: &str) {
    println!("{line}");
}

pub(crate) fn render_outcome(line: &str) {
    render_section("Outcome", &[line.to_owned()]);
}

pub(crate) fn render_next_actions(lines: &[String]) {
    render_section("Next Actions", lines);
}

pub(crate) fn quoted_config_path(config_path: &Path) -> String {
    shell_quote(config_path.display().to_string())
}

fn render_section(title: &str, lines: &[String]) {
    println!("{title}");
    if lines.is_empty() {
        println!("None");
        return;
    }

    for line in lines {
        println!("{line}");
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
