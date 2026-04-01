use std::{error::Error, fmt};

use crate::cli::BootstrapArgs;

#[derive(Debug)]
struct BootstrapError {
    message: String,
}

impl BootstrapError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for BootstrapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl Error for BootstrapError {}

pub fn execute(_args: BootstrapArgs) -> Result<(), Box<dyn Error>> {
    Err(Box::new(BootstrapError::new(
        "bootstrap is not implemented yet",
    )))
}
