use std::process;

use app_replay::{parse_args, replay_event_journal_from_env, NoopReplayConsumer};

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("{error}");
        process::exit(1);
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let range = parse_args(std::env::args())?;
    let mut consumer = NoopReplayConsumer;
    replay_event_journal_from_env(range, &mut consumer).await?;
    Ok(())
}
