use std::path::PathBuf;

#[derive(clap::Args, Debug)]
pub struct RunArgs {
    #[arg(long)]
    pub config: PathBuf,
}

#[derive(clap::Args, Debug)]
pub struct BootstrapArgs {
    #[arg(long)]
    pub config: Option<PathBuf>,

    #[arg(long)]
    pub start: bool,
}

#[derive(clap::Args, Debug)]
pub struct InitArgs {
    #[arg(long)]
    pub config: PathBuf,
}

#[derive(clap::Args, Debug)]
pub struct DoctorArgs {
    #[arg(long)]
    pub config: PathBuf,
}

#[derive(clap::Args, Debug)]
pub struct TargetsArgs {
    #[command(subcommand)]
    pub command: TargetCommand,
}

#[derive(clap::Args, Debug)]
pub struct TargetStatusArgs {
    #[arg(long)]
    pub config: PathBuf,
}

#[derive(clap::Args, Debug)]
pub struct TargetCandidatesArgs {
    #[arg(long)]
    pub config: PathBuf,
}

#[derive(clap::Args, Debug)]
pub struct TargetShowCurrentArgs {
    #[arg(long)]
    pub config: PathBuf,
}

#[derive(clap::Args, Debug)]
pub struct TargetAdoptArgs {
    #[arg(long)]
    pub config: PathBuf,

    #[arg(long)]
    pub operator_target_revision: Option<String>,

    #[arg(long)]
    pub adoptable_revision: Option<String>,
}

#[derive(clap::Args, Debug)]
pub struct TargetRollbackArgs {
    #[arg(long)]
    pub config: PathBuf,

    #[arg(long = "to-operator-target-revision")]
    pub to_operator_target_revision: Option<String>,
}

#[derive(clap::Subcommand, Debug)]
pub enum TargetCommand {
    Status(TargetStatusArgs),
    Candidates(TargetCandidatesArgs),
    ShowCurrent(TargetShowCurrentArgs),
    Adopt(TargetAdoptArgs),
    Rollback(TargetRollbackArgs),
}

#[derive(clap::Parser, Debug)]
pub struct AppLiveCli {
    #[command(subcommand)]
    pub command: AppLiveCommand,
}

#[derive(clap::Subcommand, Debug)]
pub enum AppLiveCommand {
    Bootstrap(BootstrapArgs),
    Doctor(DoctorArgs),
    Init(InitArgs),
    Run(RunArgs),
    Targets(TargetsArgs),
}
