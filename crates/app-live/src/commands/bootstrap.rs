mod error;
mod flow;
mod output;
mod prompt;

use std::error::Error;

use crate::cli::BootstrapArgs;

pub fn execute(args: BootstrapArgs) -> Result<(), Box<dyn Error>> {
    let result = flow::execute(args);
    if let Err(error) = &result {
        eprintln!("{error}");
    }
    result.map_err(|error| Box::new(error) as Box<dyn Error>)
}
