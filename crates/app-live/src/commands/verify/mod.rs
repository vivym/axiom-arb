use std::{error::Error, io};

use crate::cli::VerifyArgs;

pub fn execute(_args: VerifyArgs) -> Result<(), Box<dyn Error>> {
    let error = io::Error::new(
        io::ErrorKind::Other,
        "verify is not implemented yet; this command currently exposes only the CLI surface",
    );
    eprintln!("{error}");
    Err(error.into())
}
