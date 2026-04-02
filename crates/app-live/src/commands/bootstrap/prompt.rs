use std::io::{self, BufRead, IsTerminal, Write};

use crate::commands::init::{InitError, PromptIo};

#[derive(Clone, Copy)]
pub enum BootstrapModeSelection {
    Paper,
    Smoke,
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

pub fn select_bootstrap_mode<P: PromptIo>(
    prompt: &mut P,
) -> Result<BootstrapModeSelection, InitError> {
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
