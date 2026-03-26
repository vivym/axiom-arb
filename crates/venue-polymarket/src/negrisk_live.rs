use std::future::Future;

use serde::Deserialize;

use execution::providers::{
    ReconcileProvider, ReconcileProviderError, SubmitProviderError, VenueExecutionProvider,
};
use execution::{
    LiveSubmissionRecord, LiveSubmitOutcome, PendingReconcileWork, ReconcileOutcome,
    SignedFamilySubmission,
};

use crate::orders::{build_post_order_request_from_signed_member, PostOrderTransport};
use crate::{L2AuthHeaders, PolymarketRestClient, RelayerAuth, RestError};

const PROVIDER_NAME: &str = "polymarket";

#[derive(Debug, Clone)]
pub struct PolymarketNegRiskSubmitProvider<'a> {
    rest: PolymarketRestClient,
    auth: L2AuthHeaders<'a>,
    transport: PostOrderTransport,
}

#[derive(Debug, Clone)]
pub struct PolymarketNegRiskReconcileProvider<'a> {
    rest: PolymarketRestClient,
    auth: RelayerAuth<'a>,
}

#[derive(Debug, Deserialize)]
struct SubmitOrderResponse {
    success: bool,
    #[serde(alias = "orderID", alias = "orderId")]
    order_id: String,
    status: String,
    #[serde(default, alias = "errorMsg")]
    error_msg: String,
}

impl<'a> PolymarketNegRiskSubmitProvider<'a> {
    pub fn new(
        rest: PolymarketRestClient,
        auth: L2AuthHeaders<'a>,
        transport: PostOrderTransport,
    ) -> Self {
        Self {
            rest,
            auth,
            transport,
        }
    }

    async fn submit_family_async(
        &self,
        signed: &SignedFamilySubmission,
        attempt: &domain::ExecutionAttemptContext,
    ) -> Result<LiveSubmitOutcome, SubmitProviderError> {
        let mut accepted_record: Option<LiveSubmissionRecord> = None;
        let mut pending_ref: Option<String> = None;

        for member in &signed.members {
            let submission = build_post_order_request_from_signed_member(member, &self.transport)
                .map_err(|err| {
                SubmitProviderError::new(format!("order build error: {err:?}"))
            })?;
            let request = self
                .rest
                .build_submit_order_request(&self.auth, &submission)
                .map_err(|err| SubmitProviderError::new(format!("submit request error: {err}")))?;
            let response = self
                .rest
                .execute_json::<SubmitOrderResponse>(request)
                .await
                .map_err(|err| {
                    SubmitProviderError::new(format!("submit transport error: {err}"))
                })?;

            let record = LiveSubmissionRecord {
                submission_ref: response.order_id.clone(),
                attempt_id: attempt.attempt_id.clone(),
                route: attempt.route.clone(),
                scope: attempt.scope.clone(),
                provider: PROVIDER_NAME.to_owned(),
            };

            if accepted_record.is_none() {
                accepted_record = Some(record.clone());
            }

            if response.success && is_submit_status_confirmed(&response.status) {
                continue;
            }

            if response.success && is_submit_status_ambiguous(&response.status) {
                pending_ref = Some(response.order_id);
                accepted_record = Some(record);
                continue;
            }

            if response.success {
                pending_ref = Some(response.order_id);
                accepted_record = Some(record);
                continue;
            }

            return Ok(LiveSubmitOutcome::RejectedDefinitive {
                reason: submit_rejection_reason(&response),
            });
        }

        match pending_ref {
            Some(pending_ref) => Ok(LiveSubmitOutcome::AcceptedButUnconfirmed {
                submission_record: accepted_record,
                pending_ref,
            }),
            None => Ok(LiveSubmitOutcome::Accepted {
                submission_record: accepted_record
                    .expect("family submission should produce a record"),
            }),
        }
    }
}

impl<'a> VenueExecutionProvider for PolymarketNegRiskSubmitProvider<'a> {
    fn submit_family(
        &self,
        signed: &SignedFamilySubmission,
        attempt: &domain::ExecutionAttemptContext,
    ) -> Result<LiveSubmitOutcome, SubmitProviderError> {
        run_blocking(self.submit_family_async(signed, attempt))
    }
}

impl<'a> PolymarketNegRiskReconcileProvider<'a> {
    pub fn new(rest: PolymarketRestClient, auth: RelayerAuth<'a>) -> Self {
        Self { rest, auth }
    }

    async fn reconcile_live_async(
        &self,
        work: &PendingReconcileWork,
    ) -> Result<ReconcileOutcome, ReconcileProviderError> {
        let transactions = self
            .rest
            .fetch_recent_transactions(&self.auth)
            .await
            .map_err(map_relayer_error)?;

        if transactions
            .iter()
            .any(|transaction| transaction.state_is_pending_or_unknown())
        {
            return Ok(ReconcileOutcome::StillPending);
        }

        if transactions
            .iter()
            .any(|transaction| transaction.state_is_confirmed())
        {
            return Ok(ReconcileOutcome::ConfirmedAuthoritative {
                submission_ref: work.pending_ref.clone(),
            });
        }

        if transactions
            .iter()
            .any(|transaction| transaction.state_is_terminal())
        {
            return Ok(ReconcileOutcome::NeedsRecovery {
                pending_ref: work.pending_ref.clone(),
                reason: "relayer transaction reached a terminal state".to_owned(),
            });
        }

        Ok(ReconcileOutcome::StillPending)
    }
}

impl<'a> ReconcileProvider for PolymarketNegRiskReconcileProvider<'a> {
    fn reconcile_live(
        &self,
        work: &PendingReconcileWork,
    ) -> Result<ReconcileOutcome, ReconcileProviderError> {
        run_blocking(self.reconcile_live_async(work))
    }
}

fn run_blocking<T: Send>(future: impl Future<Output = T> + Send) -> T {
    std::thread::scope(|scope| {
        scope
            .spawn(move || {
                tokio::runtime::Runtime::new()
                    .expect("tokio runtime should be available")
                    .block_on(future)
            })
            .join()
            .expect("provider future should complete")
    })
}

fn is_submit_status_confirmed(status: &str) -> bool {
    matches!(status.to_ascii_lowercase().as_str(), "live" | "matched")
}

fn is_submit_status_ambiguous(status: &str) -> bool {
    matches!(
        status.to_ascii_lowercase().as_str(),
        "delayed" | "unmatched"
    )
}

fn submit_rejection_reason(response: &SubmitOrderResponse) -> String {
    let status = response.status.trim();
    let error_msg = response.error_msg.trim();

    if !error_msg.is_empty() {
        format!(
            "polymarket rejected order {}: {error_msg}",
            response.order_id
        )
    } else if !status.is_empty() {
        format!(
            "polymarket rejected order {} with status {status}",
            response.order_id
        )
    } else {
        format!("polymarket rejected order {}", response.order_id)
    }
}

fn map_relayer_error(error: RestError) -> ReconcileProviderError {
    ReconcileProviderError::new(format!("relayer status error: {error}"))
}
