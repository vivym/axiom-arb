use std::io::{self, BufRead, Write};

use crate::commands::init::{InitError, PromptIo};

pub struct BootstrapPrompt {
    buffered_line: Option<String>,
}

impl BootstrapPrompt {
    pub fn from_buffered_line(buffered_line: String) -> Self {
        Self {
            buffered_line: Some(buffered_line),
        }
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

pub fn read_first_stdin_line() -> Result<Option<String>, InitError> {
    let mut line = String::new();
    let bytes_read = io::stdin().lock().read_line(&mut line)?;
    if bytes_read == 0 {
        return Ok(None);
    }
    Ok(Some(line))
}
