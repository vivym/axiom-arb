mod error;
mod flow;
mod output;

use std::error::Error;

use crate::cli::BootstrapArgs;

pub fn execute(args: BootstrapArgs) -> Result<(), Box<dyn Error>> {
    flow::execute(args).map_err(|error| Box::new(error) as Box<dyn Error>)
}
