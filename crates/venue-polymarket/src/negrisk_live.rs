use std::future::Future;
use std::sync::mpsc;

use serde::Deserialize;
use tokio::runtime::Handle;

use execution::providers::{
    ReconcileProvider, ReconcileProviderError, SubmitProviderError, VenueExecutionProvider,
};
use execution::{
    LiveSubmissionRecord, LiveSubmitOutcome, PendingReconcileWork, ReconcileOutcome,
    SignedFamilySubmission,
};

use crate::auth::{L2AuthHeaders, RelayerAuth};
use crate::orders::{build_post_order_request_from_signed_member, PostOrderTransport};
use crate::rest::{PolymarketRestClient, RestError};
use crate::{
    PolymarketGateway, PolymarketOrderQuery, PolymarketSignedOrder, PolymarketSubmitResponse,
};

const PROVIDER_NAME: &str = "polymarket";

#[derive(Debug, Clone)]
pub struct PolymarketNegRiskSubmitProvider<'a> {
    rest: Option<PolymarketRestClient>,
    gateway: Option<PolymarketGateway>,
    runtime_handle: Option<Handle>,
    auth: L2AuthHeaders<'a>,
    transport: PostOrderTransport,
}

#[derive(Debug, Clone)]
pub struct PolymarketNegRiskReconcileProvider<'a> {
    rest: Option<PolymarketRestClient>,
    gateway: Option<PolymarketGateway>,
    runtime_handle: Option<Handle>,
    l2_auth: L2AuthHeaders<'a>,
    relayer_auth: RelayerAuth<'a>,
}

#[derive(Debug, Deserialize)]
struct SubmitOrderResponse {
    success: bool,
    #[serde(alias = "orderID", alias = "orderId")]
    order_id: String,
    status: String,
    #[serde(default, rename = "transactionsHashes")]
    transaction_hashes: Vec<String>,
    #[serde(default, alias = "errorMsg")]
    error_msg: String,
}

enum PendingRefTarget<'a> {
    Tx(&'a str),
    Order(&'a str),
}

#[derive(Debug)]
enum FamilySubmitFailure {
    Outcome(String),
    Error(String),
}

#[derive(Debug)]
struct FamilySubmitAggregation {
    total_members: usize,
    success_count: usize,
    first_success: Option<LiveSubmitOutcome>,
    first_failure: Option<FamilySubmitFailure>,
}

impl FamilySubmitAggregation {
    fn new(total_members: usize) -> Self {
        Self {
            total_members,
            success_count: 0,
            first_success: None,
            first_failure: None,
        }
    }

    fn record(&mut self, result: Result<LiveSubmitOutcome, SubmitProviderError>) {
        match result {
            Ok(outcome) if outcome.is_accepted() => {
                self.success_count += 1;
                if self.first_success.is_none() {
                    self.first_success = Some(outcome);
                }
            }
            Ok(outcome) => {
                if self.first_failure.is_none() {
                    self.first_failure =
                        Some(FamilySubmitFailure::Outcome(outcome_reason(&outcome)));
                }
            }
            Err(error) => {
                if self.first_failure.is_none() {
                    self.first_failure = Some(FamilySubmitFailure::Error(error.reason));
                }
            }
        }
    }

    fn finish(self) -> Result<LiveSubmitOutcome, SubmitProviderError> {
        let FamilySubmitAggregation {
            total_members,
            success_count,
            first_success,
            first_failure,
        } = self;

        if total_members == 0 {
            return Err(SubmitProviderError::new(
                "polymarket live submit requires at least one signed family member",
            ));
        }

        if success_count == total_members {
            return Ok(first_success.expect("at least one successful outcome is present"));
        }

        if success_count > 0 {
            let first_success = first_success.expect("partial success requires an anchor");
            let pending_ref = outcome_anchor_ref(&first_success)
                .expect("accepted family submission should have an anchor ref")
                .to_owned();
            let reason = match first_failure {
                Some(FamilySubmitFailure::Outcome(reason)) => reason,
                Some(FamilySubmitFailure::Error(reason)) => reason,
                None => "family submit failed after partial success".to_owned(),
            };

            return Ok(LiveSubmitOutcome::Ambiguous {
                pending_ref,
                reason: format!(
                    "submitted {success_count}/{total_members} polymarket family members before failure: {reason}"
                ),
            });
        }

        match first_failure {
            Some(FamilySubmitFailure::Outcome(reason)) => {
                Ok(LiveSubmitOutcome::RejectedDefinitive { reason })
            }
            Some(FamilySubmitFailure::Error(reason)) => Err(SubmitProviderError::new(reason)),
            None => Err(SubmitProviderError::new(
                "polymarket live submit failed without capturing a member result",
            )),
        }
    }
}

impl<'a> PolymarketNegRiskSubmitProvider<'a> {
    pub fn new(
        rest: PolymarketRestClient,
        auth: L2AuthHeaders<'a>,
        transport: PostOrderTransport,
    ) -> Self {
        Self {
            rest: Some(rest),
            gateway: None,
            runtime_handle: None,
            auth,
            transport,
        }
    }

    pub fn with_gateway(
        auth: L2AuthHeaders<'a>,
        transport: PostOrderTransport,
        gateway: PolymarketGateway,
    ) -> Self {
        Self {
            rest: None,
            gateway: Some(gateway),
            runtime_handle: None,
            auth,
            transport,
        }
    }

    pub fn with_gateway_runtime(
        auth: L2AuthHeaders<'a>,
        transport: PostOrderTransport,
        gateway: PolymarketGateway,
        runtime_handle: Handle,
    ) -> Self {
        Self {
            rest: None,
            gateway: Some(gateway),
            runtime_handle: Some(runtime_handle),
            auth,
            transport,
        }
    }

    async fn submit_family_async(
        &self,
        signed: &SignedFamilySubmission,
        attempt: &domain::ExecutionAttemptContext,
    ) -> Result<LiveSubmitOutcome, SubmitProviderError> {
        let mut aggregate = FamilySubmitAggregation::new(signed.members.len());

        for member in &signed.members {
            aggregate.record(self.submit_member_async(member, attempt).await);
        }

        aggregate.finish()
    }

    fn submit_family_via_gateway_runtime(
        &self,
        gateway: &PolymarketGateway,
        runtime_handle: &Handle,
        signed: &SignedFamilySubmission,
        attempt: &domain::ExecutionAttemptContext,
    ) -> Result<LiveSubmitOutcome, SubmitProviderError> {
        let mut aggregate = FamilySubmitAggregation::new(signed.members.len());

        for member in &signed.members {
            aggregate.record(self.submit_member_via_gateway_runtime(
                gateway,
                runtime_handle,
                member,
                attempt,
            ));
        }

        aggregate.finish()
    }

    async fn submit_member_async(
        &self,
        member: &execution::signing::SignedFamilyMember,
        attempt: &domain::ExecutionAttemptContext,
    ) -> Result<LiveSubmitOutcome, SubmitProviderError> {
        let submission = build_post_order_request_from_signed_member(member, &self.transport)
            .map_err(|err| SubmitProviderError::new(format!("order build error: {err:?}")))?;
        let response = if let Some(gateway) = &self.gateway {
            let gateway_order = polymarket_signed_order_from_submission(&submission)
                .map_err(SubmitProviderError::new)?;
            let response = gateway
                .submit_order(gateway_order)
                .await
                .map_err(|err| SubmitProviderError::new(format!("submit gateway error: {err}")))?;
            submit_order_response_from_gateway(response)
        } else {
            let rest = self.rest.as_ref().ok_or_else(|| {
                SubmitProviderError::new(
                    "submit provider requires a REST client when no gateway is configured",
                )
            })?;
            let request = self
                .rest
                .as_ref()
                .expect("rest client should be present when gateway is absent")
                .build_submit_order_request(&self.auth, &submission)
                .map_err(|err| SubmitProviderError::new(format!("submit request error: {err}")))?;
            rest.execute_json::<SubmitOrderResponse>(request)
                .await
                .map_err(|err| SubmitProviderError::new(format!("submit transport error: {err}")))?
        };

        submit_order_response_to_outcome(response, attempt)
    }

    fn submit_member_via_gateway_runtime(
        &self,
        gateway: &PolymarketGateway,
        runtime_handle: &Handle,
        member: &execution::signing::SignedFamilyMember,
        attempt: &domain::ExecutionAttemptContext,
    ) -> Result<LiveSubmitOutcome, SubmitProviderError> {
        let submission = build_post_order_request_from_signed_member(member, &self.transport)
            .map_err(|err| SubmitProviderError::new(format!("order build error: {err:?}")))?;
        let gateway = gateway.clone();
        let gateway_order = polymarket_signed_order_from_submission(&submission)
            .map_err(SubmitProviderError::new)?;
        let response = run_on_shared_runtime(runtime_handle, async move {
            gateway.submit_order(gateway_order).await
        })
        .map_err(|err| SubmitProviderError::new(format!("submit shared runtime error: {err}")))?
        .map_err(|err| SubmitProviderError::new(format!("submit gateway error: {err}")))?;

        submit_order_response_to_outcome(submit_order_response_from_gateway(response), attempt)
    }
}

impl<'a> VenueExecutionProvider for PolymarketNegRiskSubmitProvider<'a> {
    fn submit_family(
        &self,
        signed: &SignedFamilySubmission,
        attempt: &domain::ExecutionAttemptContext,
    ) -> Result<LiveSubmitOutcome, SubmitProviderError> {
        if signed.members.len() > 1 {
            return Err(SubmitProviderError::new(
                "polymarket live submit requires a single signed family member",
            ));
        }

        if let (Some(gateway), Some(runtime_handle)) = (&self.gateway, &self.runtime_handle) {
            return self.submit_family_via_gateway_runtime(
                gateway,
                runtime_handle,
                signed,
                attempt,
            );
        }

        run_blocking(self.submit_family_async(signed, attempt))
    }
}

impl<'a> PolymarketNegRiskReconcileProvider<'a> {
    pub fn new(
        rest: PolymarketRestClient,
        l2_auth: L2AuthHeaders<'a>,
        relayer_auth: RelayerAuth<'a>,
    ) -> Self {
        Self {
            rest: Some(rest),
            gateway: None,
            runtime_handle: None,
            l2_auth,
            relayer_auth,
        }
    }

    pub fn with_gateway(
        l2_auth: L2AuthHeaders<'a>,
        relayer_auth: RelayerAuth<'a>,
        gateway: PolymarketGateway,
    ) -> Self {
        Self {
            rest: None,
            gateway: Some(gateway),
            runtime_handle: None,
            l2_auth,
            relayer_auth,
        }
    }

    pub fn with_gateway_runtime(
        l2_auth: L2AuthHeaders<'a>,
        relayer_auth: RelayerAuth<'a>,
        gateway: PolymarketGateway,
        runtime_handle: Handle,
    ) -> Self {
        Self {
            rest: None,
            gateway: Some(gateway),
            runtime_handle: Some(runtime_handle),
            l2_auth,
            relayer_auth,
        }
    }

    async fn reconcile_live_async(
        &self,
        work: &PendingReconcileWork,
    ) -> Result<ReconcileOutcome, ReconcileProviderError> {
        match parse_pending_ref(&work.pending_ref)? {
            PendingRefTarget::Tx(tx_ref) => self.reconcile_tx_ref(work, tx_ref).await,
            PendingRefTarget::Order(order_id) => self.reconcile_order_ref(order_id).await,
        }
    }

    async fn reconcile_tx_ref(
        &self,
        work: &PendingReconcileWork,
        tx_ref: &str,
    ) -> Result<ReconcileOutcome, ReconcileProviderError> {
        let transactions = self
            .fetch_recent_transactions()
            .await
            .map_err(map_relayer_error)?;
        let matching_transactions: Vec<_> = transactions
            .iter()
            .filter(|transaction| transaction.matches_pending_ref(tx_ref))
            .collect();

        if matching_transactions.is_empty()
            || matching_transactions
                .iter()
                .any(|transaction| transaction.state_is_pending_or_unknown())
        {
            return Ok(ReconcileOutcome::StillPending);
        }

        if matching_transactions
            .iter()
            .any(|transaction| transaction.state_is_confirmed())
        {
            return Ok(ReconcileOutcome::ConfirmedAuthoritative {
                submission_ref: tx_ref.to_owned(),
            });
        }

        if matching_transactions
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

    async fn reconcile_order_ref(
        &self,
        order_id: &str,
    ) -> Result<ReconcileOutcome, ReconcileProviderError> {
        let open_orders = self
            .fetch_open_orders()
            .await
            .map_err(map_open_orders_error)?;

        if open_orders.iter().any(|order| order.order_id == order_id) {
            return Ok(ReconcileOutcome::ConfirmedAuthoritative {
                submission_ref: order_id.to_owned(),
            });
        }

        Ok(ReconcileOutcome::StillPending)
    }

    fn reconcile_live_via_gateway_runtime(
        &self,
        gateway: &PolymarketGateway,
        runtime_handle: &Handle,
        work: &PendingReconcileWork,
    ) -> Result<ReconcileOutcome, ReconcileProviderError> {
        match parse_pending_ref(&work.pending_ref)? {
            PendingRefTarget::Tx(tx_ref) => {
                self.reconcile_tx_ref_via_gateway_runtime(gateway, runtime_handle, work, tx_ref)
            }
            PendingRefTarget::Order(order_id) => {
                self.reconcile_order_ref_via_gateway_runtime(gateway, runtime_handle, order_id)
            }
        }
    }

    fn reconcile_tx_ref_via_gateway_runtime(
        &self,
        gateway: &PolymarketGateway,
        runtime_handle: &Handle,
        work: &PendingReconcileWork,
        tx_ref: &str,
    ) -> Result<ReconcileOutcome, ReconcileProviderError> {
        let gateway = gateway.clone();
        let transactions = match &self.relayer_auth {
            RelayerAuth::BuilderApiKey {
                api_key,
                timestamp,
                passphrase,
                signature,
            } => {
                let api_key = (*api_key).to_owned();
                let timestamp = (*timestamp).to_owned();
                let passphrase = (*passphrase).to_owned();
                let signature = (*signature).to_owned();
                run_on_shared_runtime(runtime_handle, async move {
                    let auth = RelayerAuth::BuilderApiKey {
                        api_key: api_key.as_str(),
                        timestamp: timestamp.as_str(),
                        passphrase: passphrase.as_str(),
                        signature: signature.as_str(),
                    };
                    gateway.recent_transactions(&auth).await
                })
                .map_err(|err| ReconcileProviderError::new(format!("relayer status error: {err}")))?
                .map_err(|err| {
                    ReconcileProviderError::new(format!("relayer status error: {err}"))
                })?
            }
            RelayerAuth::RelayerApiKey { api_key, address } => {
                let api_key = (*api_key).to_owned();
                let address = (*address).to_owned();
                run_on_shared_runtime(runtime_handle, async move {
                    let auth = RelayerAuth::RelayerApiKey {
                        api_key: api_key.as_str(),
                        address: address.as_str(),
                    };
                    gateway.recent_transactions(&auth).await
                })
                .map_err(|err| ReconcileProviderError::new(format!("relayer status error: {err}")))?
                .map_err(|err| {
                    ReconcileProviderError::new(format!("relayer status error: {err}"))
                })?
            }
        };

        let matching_transactions: Vec<_> = transactions
            .iter()
            .filter(|transaction| transaction.matches_pending_ref(tx_ref))
            .collect();

        if matching_transactions.is_empty()
            || matching_transactions
                .iter()
                .any(|transaction| transaction.state_is_pending_or_unknown())
        {
            return Ok(ReconcileOutcome::StillPending);
        }

        if matching_transactions
            .iter()
            .any(|transaction| transaction.state_is_confirmed())
        {
            return Ok(ReconcileOutcome::ConfirmedAuthoritative {
                submission_ref: tx_ref.to_owned(),
            });
        }

        if matching_transactions
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

    fn reconcile_order_ref_via_gateway_runtime(
        &self,
        gateway: &PolymarketGateway,
        runtime_handle: &Handle,
        order_id: &str,
    ) -> Result<ReconcileOutcome, ReconcileProviderError> {
        let gateway = gateway.clone();
        let open_orders = run_on_shared_runtime(runtime_handle, async move {
            gateway
                .open_orders(PolymarketOrderQuery::open_orders())
                .await
        })
        .map_err(|err| ReconcileProviderError::new(format!("open orders status error: {err}")))?
        .map_err(|err| ReconcileProviderError::new(format!("open orders status error: {err}")))?;

        if open_orders.iter().any(|order| order.order_id == order_id) {
            return Ok(ReconcileOutcome::ConfirmedAuthoritative {
                submission_ref: order_id.to_owned(),
            });
        }

        Ok(ReconcileOutcome::StillPending)
    }
}

impl PolymarketNegRiskReconcileProvider<'_> {
    async fn fetch_recent_transactions(&self) -> Result<Vec<crate::RelayerTransaction>, RestError> {
        if let Some(gateway) = &self.gateway {
            return gateway
                .recent_transactions(&self.relayer_auth)
                .await
                .map_err(RestError::from);
        }

        self.rest
            .as_ref()
            .expect("rest client should be present when gateway is absent")
            .fetch_recent_transactions(&self.relayer_auth)
            .await
    }

    async fn fetch_open_orders(&self) -> Result<Vec<crate::OpenOrderSummary>, RestError> {
        if let Some(gateway) = &self.gateway {
            return gateway
                .open_orders(PolymarketOrderQuery::open_orders())
                .await
                .map(|orders| {
                    orders
                        .into_iter()
                        .map(|order| crate::OpenOrderSummary {
                            order_id: order.order_id,
                            status: None,
                            market: None,
                        })
                        .collect()
                })
                .map_err(RestError::from);
        }

        self.rest
            .as_ref()
            .expect("rest client should be present when gateway is absent")
            .fetch_open_orders(&self.l2_auth)
            .await
    }
}

impl<'a> ReconcileProvider for PolymarketNegRiskReconcileProvider<'a> {
    fn reconcile_live(
        &self,
        work: &PendingReconcileWork,
    ) -> Result<ReconcileOutcome, ReconcileProviderError> {
        if let (Some(gateway), Some(runtime_handle)) = (&self.gateway, &self.runtime_handle) {
            return self.reconcile_live_via_gateway_runtime(gateway, runtime_handle, work);
        }

        run_blocking(self.reconcile_live_async(work))
    }
}

fn run_on_shared_runtime<T: Send + 'static>(
    handle: &Handle,
    future: impl Future<Output = T> + Send + 'static,
) -> Result<T, String> {
    ensure_runtime_handle_is_off_thread(handle)?;

    let (tx, rx) = mpsc::sync_channel(1);
    std::mem::drop(handle.spawn(async move {
        let _ = tx.send(future.await);
    }));

    rx.recv().map_err(|_| {
        "shared runtime is unavailable or shut down before the gateway task completed".to_owned()
    })
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

fn ensure_runtime_handle_is_off_thread(handle: &Handle) -> Result<(), String> {
    if let Ok(current) = Handle::try_current() {
        if current.id() == handle.id() {
            return Err(
                "shared runtime handle must not be the current runtime; use a dedicated off-thread runtime"
                    .to_owned(),
            );
        }
    }

    Ok(())
}

fn parse_pending_ref(pending_ref: &str) -> Result<PendingRefTarget<'_>, ReconcileProviderError> {
    let (namespace, value) = pending_ref.split_once(':').ok_or_else(|| {
        ReconcileProviderError::new(format!(
            "unsupported pending_ref without namespace: {pending_ref}"
        ))
    })?;
    let value = value.trim();

    if value.is_empty() {
        return Err(ReconcileProviderError::new(format!(
            "pending_ref missing value: {pending_ref}"
        )));
    }

    match namespace {
        "tx" => Ok(PendingRefTarget::Tx(value)),
        "order" => Ok(PendingRefTarget::Order(value)),
        other => Err(ReconcileProviderError::new(format!(
            "unsupported pending_ref namespace {other}"
        ))),
    }
}

fn tx_pending_ref(tx_ref: &str) -> String {
    format!("tx:{tx_ref}")
}

fn order_pending_ref(order_id: &str) -> String {
    format!("order:{order_id}")
}

fn submission_record(
    submission_ref: &str,
    attempt: &domain::ExecutionAttemptContext,
) -> LiveSubmissionRecord {
    LiveSubmissionRecord {
        submission_ref: submission_ref.to_owned(),
        attempt_id: attempt.attempt_id.clone(),
        route: attempt.route.clone(),
        scope: attempt.scope.clone(),
        provider: PROVIDER_NAME.to_owned(),
    }
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

fn map_open_orders_error(error: RestError) -> ReconcileProviderError {
    ReconcileProviderError::new(format!("open orders status error: {error}"))
}

fn polymarket_signed_order_from_submission(
    submission: &crate::orders::PostOrderRequest,
) -> Result<PolymarketSignedOrder, String> {
    let order = serde_json::to_value(&submission.order)
        .map_err(|err| format!("submit order serialization error: {err}"))?;

    Ok(PolymarketSignedOrder {
        order,
        owner: submission.owner.clone(),
        order_type: match submission.order_type {
            crate::OrderType::Gtc => "GTC".to_owned(),
        },
        defer_exec: submission.defer_exec,
    })
}

fn submit_order_response_from_gateway(response: PolymarketSubmitResponse) -> SubmitOrderResponse {
    SubmitOrderResponse {
        success: response.success,
        order_id: response.order_id,
        status: response.status,
        transaction_hashes: response.transaction_hashes,
        error_msg: response.error_message.unwrap_or_default(),
    }
}

fn submit_order_response_to_outcome(
    response: SubmitOrderResponse,
    attempt: &domain::ExecutionAttemptContext,
) -> Result<LiveSubmitOutcome, SubmitProviderError> {
    if !response.success {
        return Ok(LiveSubmitOutcome::RejectedDefinitive {
            reason: submit_rejection_reason(&response),
        });
    }

    match response.status.trim().to_ascii_lowercase().as_str() {
        "live" | "unmatched" | "delayed" => Ok(LiveSubmitOutcome::Accepted {
            submission_record: submission_record(&response.order_id, attempt),
        }),
        "matched" => {
            let pending_tx = response
                .transaction_hashes
                .iter()
                .map(|hash| hash.trim())
                .find(|hash| !hash.is_empty())
                .ok_or_else(|| {
                    SubmitProviderError::new(
                        "matched polymarket response missing transactionsHashes",
                    )
                })?;

            Ok(LiveSubmitOutcome::AcceptedButUnconfirmed {
                submission_record: Some(submission_record(pending_tx, attempt)),
                pending_ref: tx_pending_ref(pending_tx),
            })
        }
        _ => Ok(LiveSubmitOutcome::AcceptedButUnconfirmed {
            submission_record: Some(submission_record(&response.order_id, attempt)),
            pending_ref: order_pending_ref(&response.order_id),
        }),
    }
}

fn outcome_reason(outcome: &LiveSubmitOutcome) -> String {
    match outcome {
        LiveSubmitOutcome::Accepted { submission_record } => {
            format!(
                "member accepted with submission ref {}",
                submission_record.submission_ref
            )
        }
        LiveSubmitOutcome::AcceptedButUnconfirmed {
            submission_record,
            pending_ref,
        } => match submission_record {
            Some(record) => format!(
                "member accepted but unconfirmed with submission ref {} and pending ref {}",
                record.submission_ref, pending_ref
            ),
            None => format!("member accepted but unconfirmed with pending ref {pending_ref}"),
        },
        LiveSubmitOutcome::RejectedDefinitive { reason } => reason.clone(),
        LiveSubmitOutcome::Ambiguous {
            pending_ref,
            reason,
        } => {
            format!("{reason} (pending ref {pending_ref})")
        }
    }
}

fn outcome_anchor_ref(outcome: &LiveSubmitOutcome) -> Option<&str> {
    match outcome {
        LiveSubmitOutcome::Accepted { submission_record } => {
            Some(submission_record.submission_ref.as_str())
        }
        LiveSubmitOutcome::AcceptedButUnconfirmed {
            submission_record,
            pending_ref,
        } => submission_record
            .as_ref()
            .map(|record| record.submission_ref.as_str())
            .or(Some(pending_ref.as_str())),
        LiveSubmitOutcome::RejectedDefinitive { .. } | LiveSubmitOutcome::Ambiguous { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use super::ensure_runtime_handle_is_off_thread;

    #[tokio::test(flavor = "current_thread")]
    async fn shared_runtime_handle_rejects_current_runtime() {
        let error = ensure_runtime_handle_is_off_thread(&tokio::runtime::Handle::current())
            .expect_err("current runtime handle should be rejected");

        assert!(error.contains("current runtime"));
    }
}
