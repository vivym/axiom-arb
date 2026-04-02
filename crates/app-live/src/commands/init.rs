mod prompt;
mod render;
mod summary;
mod wizard;

use std::{error::Error, fmt, fs, path::Path};

use config_schema::{load_raw_config_from_str, ValidatedConfig};

use crate::cli::InitArgs;
use prompt::TerminalPrompt;

pub(crate) use prompt::PromptIo;
pub(crate) use wizard::WizardResult;

#[derive(Debug)]
pub struct InitError {
    message: String,
}

impl InitError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for InitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl Error for InitError {}

impl From<std::io::Error> for InitError {
    fn from(value: std::io::Error) -> Self {
        Self::new(value.to_string())
    }
}

pub fn execute(args: InitArgs) -> Result<(), Box<dyn Error>> {
    let result = execute_inner(&args);
    if let Err(error) = &result {
        eprintln!("{error}");
    }
    result.map_err(|error| Box::new(error) as Box<dyn Error>)
}

fn execute_inner(args: &InitArgs) -> Result<(), InitError> {
    let mut prompt = TerminalPrompt::new();
    execute_with_prompt(&mut prompt, args)
}

pub(crate) fn execute_with_prompt<P: prompt::PromptIo>(
    prompt: &mut P,
    args: &InitArgs,
) -> Result<(), InitError> {
    let wizard = run_wizard_with_prompt(prompt, &args.config)?;
    validate_and_write_rendered_config(&args.config, &wizard.rendered_config)?;

    for section in &wizard.summary.sections {
        prompt.println(section.title)?;
        for line in &section.lines {
            prompt.println(line)?;
        }
    }

    Ok(())
}

pub(crate) fn run_wizard_with_prompt<P: prompt::PromptIo>(
    prompt: &mut P,
    config_path: &Path,
) -> Result<WizardResult, InitError> {
    wizard::run(prompt, config_path)
}

pub(crate) fn paper_wizard_result(config_path: &Path) -> Result<WizardResult, InitError> {
    Ok(wizard::paper(config_path))
}

pub(crate) fn validate_and_write_rendered_config(
    config_path: &Path,
    rendered_config: &str,
) -> Result<(), InitError> {
    validate_rendered_config(rendered_config)?;

    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(config_path, rendered_config)?;
    Ok(())
}

pub(crate) fn validate_rendered_config(rendered_config: &str) -> Result<(), InitError> {
    let raw = load_raw_config_from_str(rendered_config)
        .map_err(|error| InitError::new(error.to_string()))?;
    let validated = ValidatedConfig::new(raw).map_err(|error| InitError::new(error.to_string()))?;
    validated
        .for_app_live()
        .map_err(|error| InitError::new(error.to_string()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::{prompt::PromptIo, InitError};

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
    fn reusable_prompt_execution_and_validation_write_support_paper_mode() {
        let temp = tempfile::tempdir().expect("temp dir");
        let config_path = temp.path().join("config").join("axiom-arb.local.toml");
        let mut prompt = TestPrompt::new(&["paper"]);

        let wizard = super::run_wizard_with_prompt(&mut prompt, &config_path)
            .expect("wizard should render paper config");
        super::validate_rendered_config(&wizard.rendered_config)
            .expect("paper config should validate");
        super::validate_and_write_rendered_config(&config_path, &wizard.rendered_config)
            .expect("paper config should write");

        assert_eq!(
            std::fs::read_to_string(&config_path).expect("config should exist"),
            "[runtime]\nmode = \"paper\"\n"
        );
    }
}
