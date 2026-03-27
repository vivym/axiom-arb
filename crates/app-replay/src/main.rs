use std::process;

use app_replay::{
    load_neg_risk_foundation_summary_from_env, load_negrisk_candidate_summary_from_env, parse_args,
    replay_event_journal_from_env, NegRiskSummaryError, SummaryReplayConsumer,
};
use observability::{bootstrap_observability, field_keys, span_names};
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
        run_span_for_record.record("after_seq", range.after_seq);

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

        match load_neg_risk_foundation_summary_from_env().await {
            Ok(summary) => {
                let negrisk_summary_span = tracing::info_span!(
                    span_names::REPLAY_NEGRISK_SUMMARY,
                    discovered_family_count = summary.discovered_family_count,
                    validated_family_count = summary.validated_family_count,
                    excluded_family_count = summary.excluded_family_count,
                    halted_family_count = summary.halted_family_count,
                    recent_validation_event_count = summary.recent_validation_event_count,
                    recent_halt_event_count = summary.recent_halt_event_count,
                    latest_discovery_revision = summary.latest_discovery_revision,
                    latest_metadata_snapshot_hash =
                        summary.latest_metadata_snapshot_hash.as_deref(),
                );
                let _negrisk_summary_guard = negrisk_summary_span.enter();
                tracing::info!(
                    discovered_family_count = summary.discovered_family_count,
                    validated_family_count = summary.validated_family_count,
                    excluded_family_count = summary.excluded_family_count,
                    halted_family_count = summary.halted_family_count,
                    recent_validation_event_count = summary.recent_validation_event_count,
                    recent_halt_event_count = summary.recent_halt_event_count,
                    latest_discovery_revision = summary.latest_discovery_revision,
                    latest_metadata_snapshot_hash =
                        summary.latest_metadata_snapshot_hash.as_deref(),
                    "app-replay neg-risk summary"
                );
            }
            Err(NegRiskSummaryError::MissingDiscoverySnapshot) => {}
            Err(error) => {
                tracing::warn!(error = %error, "app-replay neg-risk summary unavailable");
            }
        }

        match load_negrisk_candidate_summary_from_env().await {
            Ok(summary) if summary.candidate_target_set_count > 0 => {
                let negrisk_candidates_span = tracing::info_span!(
                    span_names::REPLAY_NEGRISK_CANDIDATES,
                    candidate_target_set_count = summary.candidate_target_set_count,
                    adoptable_target_revision_count = summary.adoptable_target_revision_count,
                    adoption_provenance_count = summary.adoption_provenance_count,
                    candidate_revision = summary.latest_candidate_revision.as_deref(),
                    adoptable_revision = summary.latest_adoptable_revision.as_deref(),
                    operator_target_revision = summary.operator_target_revision.as_deref(),
                    candidate_status = if summary.operator_target_revision.is_some() {
                        "provenance_resolved"
                    } else if summary.latest_adoptable_revision.is_some() {
                        "adoptable"
                    } else {
                        "advisory"
                    }
                );
                let _negrisk_candidates_guard = negrisk_candidates_span.enter();
                tracing::info!(
                    candidate_target_set_count = summary.candidate_target_set_count,
                    adoptable_target_revision_count = summary.adoptable_target_revision_count,
                    adoption_provenance_count = summary.adoption_provenance_count,
                    { field_keys::CANDIDATE_REVISION } =
                        summary.latest_candidate_revision.as_deref(),
                    { field_keys::ADOPTABLE_REVISION } =
                        summary.latest_adoptable_revision.as_deref(),
                    { field_keys::OPERATOR_TARGET_REVISION } =
                        summary.operator_target_revision.as_deref(),
                    { field_keys::CANDIDATE_STATUS } = if summary.operator_target_revision.is_some()
                    {
                        "provenance_resolved"
                    } else if summary.latest_adoptable_revision.is_some() {
                        "adoptable"
                    } else {
                        "advisory"
                    },
                    "app-replay neg-risk candidates"
                );
            }
            Ok(_) => {}
            Err(error) => {
                tracing::warn!(error = %error, "app-replay neg-risk candidate summary unavailable");
            }
        }

        Ok::<(), Box<dyn std::error::Error>>(())
    }
    .instrument(run_span)
    .await
}
