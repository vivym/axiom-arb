use std::path::PathBuf;

#[derive(clap::Args, Debug)]
pub struct RunArgs {
    #[arg(long)]
    pub config: PathBuf,
}

#[derive(clap::Parser, Debug)]
pub struct AppLiveCli {
    #[command(subcommand)]
    pub command: AppLiveCommand,
}

#[derive(clap::Subcommand, Debug)]
pub enum AppLiveCommand {
    Run(RunArgs),
}
