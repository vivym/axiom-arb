use std::{error::Error, io};

use crate::cli::BootstrapArgs;

pub fn execute(_args: BootstrapArgs) -> Result<(), Box<dyn Error>> {
    eprintln!("bootstrap is not implemented yet");
    Err(Box::new(io::Error::other("bootstrap is not implemented yet")))
}
