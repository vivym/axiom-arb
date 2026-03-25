use std::process;

use app_replay::{parse_args, replay_event_journal_from_env, SummaryReplayConsumer};
use observability::{bootstrap_observability, span_names};

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        tracing::error!(error = %error, "app-replay replay failed");
        process::exit(1);
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let _observability = bootstrap_observability("app-replay");
    let bootstrap_span = tracing::info_span!(span_names::REPLAY_RUN);
    let _bootstrap_guard = bootstrap_span.enter();
    let range = parse_args(std::env::args())?;
    let mut consumer = SummaryReplayConsumer::default();
    replay_event_journal_from_env(range, &mut consumer).await?;

    let summary = consumer.summary();
    let summary_span = tracing::info_span!(
        span_names::REPLAY_SUMMARY,
        processed_count = summary.processed_count,
        last_journal_seq = %summary
            .last_journal_seq
            .map(|value| value.to_string())
            .unwrap_or_else(|| "none".to_owned())
    );
    let _summary_guard = summary_span.enter();
    tracing::info!(
        processed_count = summary.processed_count,
        last_journal_seq = %summary
            .last_journal_seq
            .map(|value| value.to_string())
            .unwrap_or_else(|| "none".to_owned()),
        "replay summary emitted"
    );

    Ok(())
}
