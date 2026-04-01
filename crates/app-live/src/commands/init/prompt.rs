use std::io::{self, BufRead, Write};

use super::InitError;

pub trait PromptIo {
    fn read_line(&mut self) -> Result<String, InitError>;
    fn println(&mut self, line: &str) -> Result<(), InitError>;
}

pub struct TerminalPrompt;

impl TerminalPrompt {
    pub fn new() -> Self {
        Self
    }
}

impl PromptIo for TerminalPrompt {
    fn read_line(&mut self) -> Result<String, InitError> {
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

pub fn select_one<P: PromptIo>(
    prompt: &mut P,
    label: &str,
    options: &[&str],
) -> Result<usize, InitError> {
    loop {
        prompt.println(label)?;
        for option in options {
            prompt.println(option)?;
        }

        let answer = prompt.read_line()?.trim().to_lowercase();
        if let Some(index) = options.iter().position(|option| answer == *option) {
            return Ok(index);
        }

        prompt.println("Please choose one of the listed options.")?;
    }
}

#[allow(dead_code)]
pub fn confirm<P: PromptIo>(prompt: &mut P, label: &str) -> Result<bool, InitError> {
    loop {
        prompt.println(label)?;
        match prompt.read_line()?.trim().to_lowercase().as_str() {
            "y" | "yes" => return Ok(true),
            "n" | "no" => return Ok(false),
            _ => prompt.println("Please answer yes or no.")?,
        }
    }
}

#[allow(dead_code)]
pub fn ask_nonempty<P: PromptIo>(prompt: &mut P, label: &str) -> Result<String, InitError> {
    loop {
        prompt.println(label)?;
        let answer = prompt.read_line()?.trim().to_string();
        if !answer.is_empty() {
            return Ok(answer);
        }
        prompt.println("Please enter a value.")?;
    }
}
