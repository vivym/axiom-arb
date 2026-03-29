#[derive(clap::Parser, Debug)]
pub struct AppReplayCli {
    #[arg(long)]
    pub config: std::path::PathBuf,

    #[arg(long = "from-seq")]
    pub from_seq: i64,

    #[arg(long)]
    pub limit: Option<i64>,
}
