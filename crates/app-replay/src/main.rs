use std::process;

use app_replay::{parse_args, replay_event_journal_from_env, SummaryReplayConsumer};
use observability::{bootstrap_observability, span_names};
use tracing::field;
use tracing::Instrument;

#[tokio::main]
async fn main() {
    let _observability = bootstrap_observability("app-replay");
    if run().await.is_err() {
        process::exit(1);
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let run_span = tracing::info_span!(span_names::REPLAY_RUN, after_seq = field::Empty);
    let run_span_for_record = run_span.clone();

    async move {
        let range = parse_args(std::env::args()).map_err(|error| {
            tracing::error!(error = %error, "app-replay replay failed");
            Box::<dyn std::error::Error>::from(error)
        })?;
        run_span_for_record.record("after_seq", &range.after_seq);

        let mut consumer = SummaryReplayConsumer::default();
        replay_event_journal_from_env(range, &mut consumer)
            .await
            .map_err(|error| {
                tracing::error!(error = %error, "app-replay replay failed");
                Box::<dyn std::error::Error>::from(error)
            })?;

        let summary = consumer.summary();
        let summary_span = tracing::info_span!(
            span_names::REPLAY_SUMMARY,
            processed_count = summary.processed_count,
            last_journal_seq = ?summary.last_journal_seq
        );
        let _summary_guard = summary_span.enter();
        tracing::info!(
            processed_count = summary.processed_count,
            last_journal_seq = ?summary.last_journal_seq,
            "app-replay summary"
        );

        Ok::<(), Box<dyn std::error::Error>>(())
    }
    .instrument(run_span)
    .await
}
