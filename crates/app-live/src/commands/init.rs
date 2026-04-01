mod prompt;
mod render;
mod summary;
mod wizard;

use std::{error::Error, fmt, fs};

use config_schema::{load_raw_config_from_str, ValidatedConfig};

use crate::cli::InitArgs;
use prompt::TerminalPrompt;

#[derive(Debug)]
pub struct InitError {
    message: String,
}

impl InitError {
    fn new(message: impl Into<String>) -> Self {
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

fn execute_with_prompt<P: prompt::PromptIo>(
    prompt: &mut P,
    args: &InitArgs,
) -> Result<(), InitError> {
    let wizard = wizard::run(prompt, &args.config)?;
    validate_rendered_config(&wizard.rendered_config)?;

    if let Some(parent) = args.config.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&args.config, wizard.rendered_config)?;

    for section in &wizard.summary.sections {
        prompt.println(section.title)?;
        for line in &section.lines {
            prompt.println(line)?;
        }
    }

    Ok(())
}

fn validate_rendered_config(rendered_config: &str) -> Result<(), InitError> {
    let raw = load_raw_config_from_str(rendered_config)
        .map_err(|error| InitError::new(error.to_string()))?;
    let validated = ValidatedConfig::new(raw).map_err(|error| InitError::new(error.to_string()))?;
    validated
        .for_app_live()
        .map_err(|error| InitError::new(error.to_string()))?;
    Ok(())
}
