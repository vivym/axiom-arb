use std::{
    borrow::ToOwned,
    collections::BTreeSet,
    io::{self, BufRead, IsTerminal, Write},
};

use crate::commands::init::{InitError, PromptIo};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InlineTargetAdoptionSelection {
    AdoptableRevision(String),
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InlineSmokeRolloutSelection {
    Confirm,
    Decline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestartBoundarySelection {
    Confirm,
    Decline,
}

pub struct ApplyPrompt;

impl ApplyPrompt {
    pub fn new() -> Self {
        Self
    }
}

pub fn stdin_is_interactive() -> bool {
    io::stdin().is_terminal()
        || matches!(
            std::env::var("APP_LIVE_APPLY_FORCE_INTERACTIVE").as_deref(),
            Ok("1")
        )
}

impl PromptIo for ApplyPrompt {
    fn read_line(&mut self) -> Result<String, InitError> {
        let mut line = String::new();
        let bytes_read = io::stdin().lock().read_line(&mut line)?;
        if bytes_read == 0 {
            return Err(InitError::new(
                "unexpected end of input while reading apply wizard answer",
            ));
        }
        Ok(line)
    }

    fn println(&mut self, line: &str) -> Result<(), InitError> {
        let mut stdout = io::stdout().lock();
        writeln!(stdout, "{line}")?;
        stdout.flush()?;
        Ok(())
    }
}

pub fn choose_adoptable_revision<P: PromptIo>(
    prompt: &mut P,
    revisions: &[String],
) -> Result<InlineTargetAdoptionSelection, InitError> {
    if revisions.is_empty() {
        return Err(InitError::new(
            "apply could not find any adoptable revisions",
        ));
    }

    loop {
        prompt.println("Choose an adoptable revision to adopt for apply:")?;
        for revision in revisions {
            prompt.println(revision)?;
        }
        prompt.println("cancel")?;

        let selected = prompt.read_line()?.trim().to_owned();
        if selected.eq_ignore_ascii_case("cancel") {
            return Ok(InlineTargetAdoptionSelection::Cancel);
        }
        if revisions.iter().any(|revision| revision == &selected) {
            return Ok(InlineTargetAdoptionSelection::AdoptableRevision(selected));
        }

        prompt.println("Please choose one of the listed adoptable revisions or cancel.")?;
    }
}

pub fn choose_smoke_rollout_confirmation<P: PromptIo>(
    prompt: &mut P,
    family_ids: &[String],
) -> Result<InlineSmokeRolloutSelection, InitError> {
    let normalized = family_ids
        .iter()
        .map(|family_id| family_id.trim())
        .filter(|family_id| !family_id.is_empty())
        .map(ToOwned::to_owned)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();

    if normalized.is_empty() {
        return Err(InitError::new(
            "apply could not derive any adopted smoke families",
        ));
    }

    loop {
        prompt.println("Smoke rollout readiness is not enabled for the adopted family set.")?;
        prompt.println("Adopted families:")?;
        for family_id in &normalized {
            prompt.println(family_id)?;
        }
        prompt.println("Choose one:")?;
        prompt.println("confirm")?;
        prompt.println("decline")?;

        match prompt.read_line()?.trim().to_lowercase().as_str() {
            "confirm" => return Ok(InlineSmokeRolloutSelection::Confirm),
            "decline" => return Ok(InlineSmokeRolloutSelection::Decline),
            _ => prompt.println("Please choose confirm or decline.")?,
        }
    }
}

pub fn choose_restart_boundary_confirmation<P: PromptIo>(
    prompt: &mut P,
    configured_target: &str,
    active_target: Option<&str>,
) -> Result<RestartBoundarySelection, InitError> {
    let active_target = active_target.unwrap_or("not running");

    loop {
        prompt.println("Apply reached the manual restart boundary.")?;
        prompt.println(&format!("Configured target: {configured_target}"))?;
        prompt.println(&format!("Active target: {active_target}"))?;
        prompt.println(
            "apply --start only continues in the foreground after explicit confirmation.",
        )?;
        prompt.println("It will not stop or replace an existing daemon.")?;
        prompt.println("Choose one:")?;
        prompt.println("confirm")?;
        prompt.println("decline")?;

        match prompt.read_line()?.trim().to_lowercase().as_str() {
            "confirm" => return Ok(RestartBoundarySelection::Confirm),
            "decline" => return Ok(RestartBoundarySelection::Decline),
            _ => prompt.println("Please choose confirm or decline.")?,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use crate::commands::init::{InitError, PromptIo};

    use super::{
        choose_adoptable_revision, choose_restart_boundary_confirmation,
        choose_smoke_rollout_confirmation, InlineSmokeRolloutSelection,
        InlineTargetAdoptionSelection, RestartBoundarySelection,
    };

    struct TestPrompt {
        inputs: VecDeque<String>,
        output: Vec<String>,
    }

    impl TestPrompt {
        fn new(inputs: &[&str]) -> Self {
            Self {
                inputs: inputs.iter().map(|line| format!("{line}\n")).collect(),
                output: Vec::new(),
            }
        }
    }

    impl PromptIo for TestPrompt {
        fn read_line(&mut self) -> Result<String, InitError> {
            self.inputs.pop_front().ok_or_else(|| {
                InitError::new("unexpected end of input while reading apply wizard answer")
            })
        }

        fn println(&mut self, line: &str) -> Result<(), InitError> {
            self.output.push(line.to_owned());
            Ok(())
        }
    }

    #[test]
    fn choose_adoptable_revision_returns_selected_revision() {
        let mut prompt = TestPrompt::new(&["adoptable-2"]);

        let selection =
            choose_adoptable_revision(&mut prompt, &["adoptable-1".into(), "adoptable-2".into()])
                .expect("listed revision should be accepted");

        assert_eq!(
            selection,
            InlineTargetAdoptionSelection::AdoptableRevision("adoptable-2".to_owned())
        );
        assert_eq!(
            prompt.output,
            vec![
                "Choose an adoptable revision to adopt for apply:",
                "adoptable-1",
                "adoptable-2",
                "cancel",
            ]
        );
    }

    #[test]
    fn choose_adoptable_revision_allows_cancel() {
        let mut prompt = TestPrompt::new(&["cancel"]);

        let selection =
            choose_adoptable_revision(&mut prompt, &["adoptable-1".into()]).expect("cancel");

        assert_eq!(selection, InlineTargetAdoptionSelection::Cancel);
    }

    #[test]
    fn choose_adoptable_revision_reprompts_until_listed_revision_or_cancel() {
        let mut prompt = TestPrompt::new(&["nope", "adoptable-2"]);

        let selection =
            choose_adoptable_revision(&mut prompt, &["adoptable-1".into(), "adoptable-2".into()])
                .expect("second answer should be accepted");

        assert_eq!(
            selection,
            InlineTargetAdoptionSelection::AdoptableRevision("adoptable-2".to_owned())
        );
        assert_eq!(
            prompt.output,
            vec![
                "Choose an adoptable revision to adopt for apply:",
                "adoptable-1",
                "adoptable-2",
                "cancel",
                "Please choose one of the listed adoptable revisions or cancel.",
                "Choose an adoptable revision to adopt for apply:",
                "adoptable-1",
                "adoptable-2",
                "cancel",
            ]
        );
    }

    #[test]
    fn choose_smoke_rollout_confirmation_accepts_confirm() {
        let mut prompt = TestPrompt::new(&["confirm"]);

        let selection =
            choose_smoke_rollout_confirmation(&mut prompt, &["family-b".into(), "family-a".into()])
                .expect("confirm should be accepted");

        assert_eq!(selection, InlineSmokeRolloutSelection::Confirm);
        assert_eq!(
            prompt.output,
            vec![
                "Smoke rollout readiness is not enabled for the adopted family set.",
                "Adopted families:",
                "family-a",
                "family-b",
                "Choose one:",
                "confirm",
                "decline",
            ]
        );
    }

    #[test]
    fn choose_smoke_rollout_confirmation_reprompts_until_confirm_or_decline() {
        let mut prompt = TestPrompt::new(&["later", "decline"]);

        let selection = choose_smoke_rollout_confirmation(&mut prompt, &["family-a".into()])
            .expect("decline should be accepted");

        assert_eq!(selection, InlineSmokeRolloutSelection::Decline);
        assert_eq!(
            prompt.output,
            vec![
                "Smoke rollout readiness is not enabled for the adopted family set.",
                "Adopted families:",
                "family-a",
                "Choose one:",
                "confirm",
                "decline",
                "Please choose confirm or decline.",
                "Smoke rollout readiness is not enabled for the adopted family set.",
                "Adopted families:",
                "family-a",
                "Choose one:",
                "confirm",
                "decline",
            ]
        );
    }

    #[test]
    fn choose_restart_boundary_confirmation_accepts_confirm() {
        let mut prompt = TestPrompt::new(&["confirm"]);

        let selection = choose_restart_boundary_confirmation(
            &mut prompt,
            "targets-rev-9",
            Some("targets-rev-10"),
        )
        .expect("confirm should be accepted");

        assert_eq!(selection, RestartBoundarySelection::Confirm);
        assert_eq!(
            prompt.output,
            vec![
                "Apply reached the manual restart boundary.",
                "Configured target: targets-rev-9",
                "Active target: targets-rev-10",
                "apply --start only continues in the foreground after explicit confirmation.",
                "It will not stop or replace an existing daemon.",
                "Choose one:",
                "confirm",
                "decline",
            ]
        );
    }

    #[test]
    fn choose_restart_boundary_confirmation_reprompts_until_confirm_or_decline() {
        let mut prompt = TestPrompt::new(&["later", "decline"]);

        let selection = choose_restart_boundary_confirmation(
            &mut prompt,
            "targets-rev-9",
            Some("targets-rev-10"),
        )
        .expect("decline should be accepted");

        assert_eq!(selection, RestartBoundarySelection::Decline);
        assert_eq!(
            prompt.output,
            vec![
                "Apply reached the manual restart boundary.",
                "Configured target: targets-rev-9",
                "Active target: targets-rev-10",
                "apply --start only continues in the foreground after explicit confirmation.",
                "It will not stop or replace an existing daemon.",
                "Choose one:",
                "confirm",
                "decline",
                "Please choose confirm or decline.",
                "Apply reached the manual restart boundary.",
                "Configured target: targets-rev-9",
                "Active target: targets-rev-10",
                "apply --start only continues in the foreground after explicit confirmation.",
                "It will not stop or replace an existing daemon.",
                "Choose one:",
                "confirm",
                "decline",
            ]
        );
    }
}
