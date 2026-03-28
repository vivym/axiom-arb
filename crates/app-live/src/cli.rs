#[derive(clap::Parser, Debug)]
pub struct AppLiveCli {
    #[arg(long)]
    pub config: std::path::PathBuf,
}
