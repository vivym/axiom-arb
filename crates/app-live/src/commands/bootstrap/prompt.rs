use std::io::{self, BufRead, IsTerminal, Write};

use crate::commands::init::{InitError, PromptIo};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BootstrapModeSelection {
    Paper,
    Smoke,
}

pub enum BootstrapModeInput {
    Terminal,
    Piped(Option<String>),
}

pub struct BootstrapPrompt {
    buffered_line: Option<String>,
}

impl BootstrapPrompt {
    pub fn new(buffered_line: Option<String>) -> Self {
        Self { buffered_line }
    }
}

impl PromptIo for BootstrapPrompt {
    fn read_line(&mut self) -> Result<String, InitError> {
        if let Some(line) = self.buffered_line.take() {
            return Ok(line);
        }

        let mut line = String::new();
        let bytes_read = io::stdin().lock().read_line(&mut line)?;
        if bytes_read == 0 {
            return Err(InitError::new(
                "unexpected end of input while reading init wizard answer",
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

pub fn stdin_is_terminal() -> bool {
    io::stdin().is_terminal()
}

pub fn read_piped_first_line() -> Result<Option<String>, InitError> {
    let mut line = String::new();
    let bytes_read = io::stdin().lock().read_line(&mut line)?;
    if bytes_read == 0 {
        return Ok(None);
    }
    Ok(Some(line))
}

pub fn choose_bootstrap_mode<P: PromptIo>(
    prompt: &mut P,
    input: BootstrapModeInput,
) -> Result<BootstrapModeSelection, InitError> {
    match input {
        BootstrapModeInput::Terminal => select_bootstrap_mode(prompt),
        BootstrapModeInput::Piped(None) => Ok(BootstrapModeSelection::Paper),
        BootstrapModeInput::Piped(Some(line)) => parse_piped_bootstrap_mode(&line),
    }
}

fn select_bootstrap_mode<P: PromptIo>(prompt: &mut P) -> Result<BootstrapModeSelection, InitError> {
    loop {
        prompt.println("Choose a bootstrap mode:")?;
        prompt.println("paper")?;
        prompt.println("smoke")?;

        match prompt.read_line()?.trim().to_lowercase().as_str() {
            "paper" => return Ok(BootstrapModeSelection::Paper),
            "smoke" => return Ok(BootstrapModeSelection::Smoke),
            _ => prompt.println("Please choose one of the listed options.")?,
        }
    }
}

fn parse_piped_bootstrap_mode(line: &str) -> Result<BootstrapModeSelection, InitError> {
    match line.trim().to_lowercase().as_str() {
        "paper" => Ok(BootstrapModeSelection::Paper),
        "smoke" => Ok(BootstrapModeSelection::Smoke),
        _ => Err(InitError::new("bootstrap only supports paper or smoke")),
    }
}

pub fn choose_adoptable_revision<P: PromptIo>(
    prompt: &mut P,
    revisions: &[String],
) -> Result<String, InitError> {
    if revisions.is_empty() {
        return Err(InitError::new(
            "bootstrap could not find any adoptable revisions",
        ));
    }

    loop {
        prompt.println("Choose an adoptable revision for smoke bootstrap:")?;
        for revision in revisions {
            prompt.println(revision)?;
        }

        let selected = prompt.read_line()?.trim().to_owned();
        if revisions.iter().any(|revision| revision == &selected) {
            return Ok(selected);
        }

        prompt.println("Please choose one of the listed adoptable revisions.")?;
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::{choose_bootstrap_mode, BootstrapModeInput, BootstrapModeSelection};
    use crate::commands::init::{InitError, PromptIo};

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
                InitError::new("unexpected end of input while reading init wizard answer")
            })
        }

        fn println(&mut self, line: &str) -> Result<(), InitError> {
            self.output.push(line.to_owned());
            Ok(())
        }
    }

    #[test]
    fn choose_bootstrap_mode_accepts_paper_immediately_for_terminal_input() {
        let mut prompt = TestPrompt::new(&["paper"]);

        let selection = choose_bootstrap_mode(&mut prompt, BootstrapModeInput::Terminal)
            .expect("paper should be accepted");

        assert!(matches!(selection, BootstrapModeSelection::Paper));
        assert_eq!(
            prompt.output,
            vec!["Choose a bootstrap mode:", "paper", "smoke"]
        );
    }

    #[test]
    fn choose_bootstrap_mode_reprompts_until_paper_or_smoke_for_terminal_input() {
        let mut prompt = TestPrompt::new(&["live", "smoke"]);

        let selection = choose_bootstrap_mode(&mut prompt, BootstrapModeInput::Terminal)
            .expect("smoke should be accepted");

        assert!(matches!(selection, BootstrapModeSelection::Smoke));
        assert_eq!(
            prompt.output,
            vec![
                "Choose a bootstrap mode:",
                "paper",
                "smoke",
                "Please choose one of the listed options.",
                "Choose a bootstrap mode:",
                "paper",
                "smoke",
            ]
        );
    }

    #[test]
    fn choose_bootstrap_mode_defaults_to_paper_when_piped_input_is_empty() {
        let mut prompt = TestPrompt::new(&[]);

        let selection = choose_bootstrap_mode(&mut prompt, BootstrapModeInput::Piped(None))
            .expect("empty piped input should default to paper");

        assert!(matches!(selection, BootstrapModeSelection::Paper));
        assert!(prompt.output.is_empty());
    }

    #[test]
    fn choose_bootstrap_mode_rejects_unsupported_piped_mode_explicitly() {
        let mut prompt = TestPrompt::new(&[]);

        let error = choose_bootstrap_mode(
            &mut prompt,
            BootstrapModeInput::Piped(Some("live\n".to_string())),
        )
        .expect_err("unsupported piped mode should fail");

        assert_eq!(error.to_string(), "bootstrap only supports paper or smoke");
        assert!(prompt.output.is_empty());
    }

    #[test]
    fn choose_adoptable_revision_accepts_listed_revision() {
        let mut prompt = TestPrompt::new(&["adoptable-2"]);

        let selection = super::choose_adoptable_revision(
            &mut prompt,
            &["adoptable-1".into(), "adoptable-2".into()],
        )
        .expect("listed adoptable revision should be accepted");

        assert_eq!(selection, "adoptable-2");
        assert_eq!(
            prompt.output,
            vec![
                "Choose an adoptable revision for smoke bootstrap:",
                "adoptable-1",
                "adoptable-2",
            ]
        );
    }

    #[test]
    fn choose_adoptable_revision_reprompts_until_listed_revision_is_selected() {
        let mut prompt = TestPrompt::new(&["candidate-1", "adoptable-1"]);

        let selection = super::choose_adoptable_revision(&mut prompt, &["adoptable-1".into()])
            .expect("listed adoptable revision should be accepted");

        assert_eq!(selection, "adoptable-1");
        assert_eq!(
            prompt.output,
            vec![
                "Choose an adoptable revision for smoke bootstrap:",
                "adoptable-1",
                "Please choose one of the listed adoptable revisions.",
                "Choose an adoptable revision for smoke bootstrap:",
                "adoptable-1",
            ]
        );
    }
}
