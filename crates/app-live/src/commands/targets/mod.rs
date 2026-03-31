use std::error::Error;

use crate::cli::{TargetCommand, TargetsArgs};

pub mod adopt;
pub mod candidates;
pub mod config_file;
pub mod rollback;
pub mod show_current;
pub mod state;
pub mod status;

pub fn execute(args: TargetsArgs) -> Result<(), Box<dyn Error>> {
    match args.command {
        TargetCommand::Status(args) => status::execute(args),
        TargetCommand::Candidates(args) => candidates::execute(args),
        TargetCommand::ShowCurrent(args) => show_current::execute(args),
        TargetCommand::Adopt(args) => adopt::execute(args),
        TargetCommand::Rollback(args) => rollback::execute(args),
    }
}
