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
use crate::{
    L2AuthHeaders, PolymarketGateway, PolymarketOrderQuery, PolymarketRestClient,
    PolymarketSignedOrder, PolymarketSubmitResponse, RelayerAuth, RestError,
};

const PROVIDER_NAME: &str = "polymarket";

#[derive(Debug, Clone)]
pub struct PolymarketNegRiskSubmitProvider<'a> {
    rest: PolymarketRestClient,
    gateway: Option<PolymarketGateway>,
    auth: L2AuthHeaders<'a>,
    transport: PostOrderTransport,
}

#[derive(Debug, Clone)]
pub struct PolymarketNegRiskReconcileProvider<'a> {
    rest: PolymarketRestClient,
    gateway: Option<PolymarketGateway>,
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

impl<'a> PolymarketNegRiskSubmitProvider<'a> {
    pub fn new(
        rest: PolymarketRestClient,
        auth: L2AuthHeaders<'a>,
        transport: PostOrderTransport,
    ) -> Self {
        Self {
            rest,
            gateway: None,
            auth,
            transport,
        }
    }

    pub fn with_gateway(
        rest: PolymarketRestClient,
        auth: L2AuthHeaders<'a>,
        transport: PostOrderTransport,
        gateway: PolymarketGateway,
    ) -> Self {
        Self {
            rest,
            gateway: Some(gateway),
            auth,
            transport,
        }
    }

    async fn submit_family_async(
        &self,
        signed: &SignedFamilySubmission,
        attempt: &domain::ExecutionAttemptContext,
    ) -> Result<LiveSubmitOutcome, SubmitProviderError> {
        if signed.members.len() != 1 {
            return Err(SubmitProviderError::new(
                "polymarket live submit currently supports exactly one signed family member",
            ));
        }

        let member = signed
            .members
            .first()
            .expect("validated single-member family submission");
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
            let request = self
                .rest
                .build_submit_order_request(&self.auth, &submission)
                .map_err(|err| SubmitProviderError::new(format!("submit request error: {err}")))?;
            self.rest
                .execute_json::<SubmitOrderResponse>(request)
                .await
                .map_err(|err| SubmitProviderError::new(format!("submit transport error: {err}")))?
        };

        if !response.success {
            return Ok(LiveSubmitOutcome::RejectedDefinitive {
                reason: submit_rejection_reason(&response),
            });
        }

        match response.status.trim().to_ascii_lowercase().as_str() {
            "live" | "unmatched" => Ok(LiveSubmitOutcome::Accepted {
                submission_record: submission_record(&response.order_id, attempt),
            }),
            "delayed" => Ok(LiveSubmitOutcome::Accepted {
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
    pub fn new(
        rest: PolymarketRestClient,
        l2_auth: L2AuthHeaders<'a>,
        relayer_auth: RelayerAuth<'a>,
    ) -> Self {
        Self {
            rest,
            gateway: None,
            l2_auth,
            relayer_auth,
        }
    }

    pub fn with_gateway(
        rest: PolymarketRestClient,
        l2_auth: L2AuthHeaders<'a>,
        relayer_auth: RelayerAuth<'a>,
        gateway: PolymarketGateway,
    ) -> Self {
        Self {
            rest,
            gateway: Some(gateway),
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

        self.rest.fetch_open_orders(&self.l2_auth).await
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
