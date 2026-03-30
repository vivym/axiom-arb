use std::path::PathBuf;

#[derive(clap::ValueEnum, Clone, Debug)]
pub enum InitMode {
    Paper,
    Live,
}

#[derive(clap::Args, Debug)]
pub struct RunArgs {
    #[arg(long)]
    pub config: PathBuf,
}

#[derive(clap::Args, Debug)]
pub struct InitArgs {
    #[arg(long)]
    pub config: PathBuf,

    #[arg(long)]
    pub defaults: bool,

    #[arg(long, value_enum)]
    pub mode: InitMode,

    #[arg(long)]
    pub real_user_shadow_smoke: bool,
}

#[derive(clap::Args, Debug)]
pub struct DoctorArgs {
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
    Doctor(DoctorArgs),
    Init(InitArgs),
    Run(RunArgs),
}
