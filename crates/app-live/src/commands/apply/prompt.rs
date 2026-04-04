use std::{
    io::{self, BufRead, IsTerminal, Write},
};

use crate::commands::init::{InitError, PromptIo};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InlineTargetAdoptionSelection {
    AdoptableRevision(String),
    Cancel,
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

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use crate::commands::init::{InitError, PromptIo};

    use super::{choose_adoptable_revision, InlineTargetAdoptionSelection};

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

        let selection = choose_adoptable_revision(
            &mut prompt,
            &["adoptable-1".into(), "adoptable-2".into()],
        )
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

        let selection = choose_adoptable_revision(
            &mut prompt,
            &["adoptable-1".into(), "adoptable-2".into()],
        )
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
}
