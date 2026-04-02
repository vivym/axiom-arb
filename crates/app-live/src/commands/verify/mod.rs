use std::{error::Error, io};

use crate::cli::VerifyArgs;

pub fn execute(_args: VerifyArgs) -> Result<(), Box<dyn Error>> {
    Err(io::Error::new(
        io::ErrorKind::Other,
        "verify is not implemented yet; this command currently exposes only the CLI surface",
    )
    .into())
}
