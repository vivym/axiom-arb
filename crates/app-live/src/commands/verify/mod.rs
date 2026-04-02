use std::error::Error;

use crate::cli::VerifyArgs;

pub fn execute(_args: VerifyArgs) -> Result<(), Box<dyn Error>> {
    Ok(())
}
