mod support;

use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{mpsc, Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use domain::{ExecutionAttemptContext, ExecutionMode, SignatureType, WalletRoute};
use execution::providers::{ReconcileProvider, VenueExecutionProvider};
use execution::{
    LiveSubmitOutcome, PendingReconcileWork, ReconcileOutcome, SignedFamilySubmission,
};
use rust_decimal::Decimal;
use support::{
    scripted_gateway_with_open_orders, scripted_gateway_with_relayer, scripted_open_order,
};
use url::Url;
use venue_polymarket::{
    L2AuthHeaders, OrderType, PolymarketClobApi, PolymarketGateway, PolymarketGatewayError,
    PolymarketHeartbeatStatus, PolymarketNegRiskReconcileProvider, PolymarketNegRiskSubmitProvider,
    PolymarketRelayerApi, PolymarketRestClient, PolymarketSignedOrder, PolymarketSubmitResponse,
    PostOrderTransport, RelayerAuth, RelayerTransaction, RelayerTransactionType, SignerContext,
};

#[tokio::test]
async fn polymarket_submit_provider_maps_live_response_into_submission_record() {
    let server = MockServer::spawn(
        "200 OK",
        r#"{"success":true,"orderID":"0xorder-1","status":"live","makingAmount":"10","takingAmount":"5","errorMsg":""}"#,
    );
    let provider = sample_submit_provider(server.base_url());

    let outcome = provider
        .submit_family(&sample_signed_submission(), &sample_attempt())
        .expect("submit should succeed");

    match outcome {
        LiveSubmitOutcome::Accepted { submission_record } => {
            assert_eq!(submission_record.provider, "polymarket");
            assert_eq!(submission_record.submission_ref, "0xorder-1");
            assert_eq!(submission_record.attempt_id, "attempt-1");
            assert_eq!(submission_record.route, "neg-risk");
            assert_eq!(submission_record.scope, "family-a");
        }
        other => panic!("unexpected outcome: {other:?}"),
    }

    let request = server.finish();
    assert!(request.starts_with("POST /order HTTP/1.1"));
    assert!(request.contains("poly-address: 0xowner"));
}

#[tokio::test]
async fn polymarket_submit_provider_maps_unmatched_response_into_accepted_submission_record() {
    let server = MockServer::spawn(
        "200 OK",
        r#"{"success":true,"orderID":"0xorder-unmatched","status":"unmatched","makingAmount":"10","takingAmount":"5","errorMsg":""}"#,
    );
    let provider = sample_submit_provider(server.base_url());

    let outcome = provider
        .submit_family(&sample_signed_submission(), &sample_attempt())
        .expect("submit should succeed");

    match outcome {
        LiveSubmitOutcome::Accepted { submission_record } => {
            assert_eq!(submission_record.provider, "polymarket");
            assert_eq!(submission_record.submission_ref, "0xorder-unmatched");
        }
        other => panic!("unexpected outcome: {other:?}"),
    }

    let _ = server.finish();
}

#[tokio::test]
async fn polymarket_submit_provider_maps_matched_response_into_tx_backed_unconfirmed_acceptance() {
    let server = MockServer::spawn(
        "200 OK",
        r#"{"success":true,"orderID":"0xorder-matched","status":"matched","transactionsHashes":["0xtx-1","0xtx-2"],"makingAmount":"10","takingAmount":"5","errorMsg":""}"#,
    );
    let provider = sample_submit_provider(server.base_url());

    let outcome = provider
        .submit_family(&sample_signed_submission(), &sample_attempt())
        .expect("matched submit should require reconcile");

    match outcome {
        LiveSubmitOutcome::AcceptedButUnconfirmed {
            submission_record,
            pending_ref,
        } => {
            let submission_record = submission_record.expect("durable local anchor");
            assert_eq!(submission_record.submission_ref, "0xtx-1");
            assert_eq!(pending_ref, "tx:0xtx-1");
        }
        other => panic!("unexpected outcome: {other:?}"),
    }

    let _ = server.finish();
}

#[tokio::test]
async fn polymarket_submit_provider_maps_delayed_response_into_plain_acceptance() {
    let server = MockServer::spawn(
        "200 OK",
        r#"{"success":true,"orderID":"0xorder-2","status":"delayed","makingAmount":"10","takingAmount":"5","errorMsg":""}"#,
    );
    let provider = sample_submit_provider(server.base_url());

    let outcome = provider
        .submit_family(&sample_signed_submission(), &sample_attempt())
        .expect("submit should succeed");

    match outcome {
        LiveSubmitOutcome::Accepted { submission_record } => {
            assert_eq!(submission_record.provider, "polymarket");
            assert_eq!(submission_record.submission_ref, "0xorder-2");
        }
        other => panic!("unexpected outcome: {other:?}"),
    }

    let _ = server.finish();
}

#[tokio::test]
async fn polymarket_submit_provider_rejects_multi_member_submission_before_side_effects() {
    let server = MockServer::spawn("200 OK", r#"{"unused":true}"#);
    let provider = sample_submit_provider(server.base_url());

    let err = provider
        .submit_family(&sample_multi_member_submission(), &sample_attempt())
        .expect_err("multi-member family should fail closed");

    assert!(err.reason.contains("exactly one signed family member"));
    server.finish_without_request();
}

#[tokio::test]
async fn polymarket_submit_provider_can_use_gateway_without_http_side_effects() {
    let server = MockServer::spawn("200 OK", r#"{"unused":true}"#);
    let provider = sample_submit_provider_with_gateway(
        server.base_url(),
        scripted_gateway_with_open_orders(Vec::new()),
    );

    let outcome = provider
        .submit_family(&sample_signed_submission(), &sample_attempt())
        .expect("gateway-backed submit should succeed");

    match outcome {
        LiveSubmitOutcome::Accepted { submission_record } => {
            assert_eq!(submission_record.provider, "polymarket");
            assert_eq!(submission_record.submission_ref, "order-default");
        }
        other => panic!("unexpected outcome: {other:?}"),
    }

    server.finish_without_request();
}

#[tokio::test]
async fn polymarket_reconcile_provider_maps_pending_relayer_status_into_still_pending() {
    let server = MockServer::spawn(
        "200 OK",
        r#"[{"transactionID":"tx-1","state":"STATE_NEW","type":"SAFE","nonce":"60","owner":"0x4444444444444444444444444444444444444444"}]"#,
    );
    let provider = sample_reconcile_provider(server.base_url());
    let work = sample_pending_work("tx:tx-1");

    let outcome = provider
        .reconcile_live(&work)
        .expect("reconcile should succeed");

    assert!(matches!(outcome, ReconcileOutcome::StillPending));

    let request = server.finish();
    assert!(request.starts_with("GET /transactions HTTP/1.1"));
    assert!(request.contains("poly-builder-api-key: builder-key-1"));
}

#[tokio::test]
async fn polymarket_reconcile_provider_maps_unknown_matching_status_into_still_pending() {
    let server = MockServer::spawn(
        "200 OK",
        r#"[{"transactionID":"tx-unknown","state":"STATE_MYSTERY","type":"SAFE","nonce":"60","owner":"0x4444444444444444444444444444444444444444"}]"#,
    );
    let provider = sample_reconcile_provider(server.base_url());

    let outcome = provider
        .reconcile_live(&sample_pending_work("tx:tx-unknown"))
        .expect("unknown status should stay pending");

    assert!(matches!(outcome, ReconcileOutcome::StillPending));
    let _ = server.finish();
}

#[tokio::test]
async fn polymarket_reconcile_provider_ignores_unrelated_confirmed_transactions() {
    let server = MockServer::spawn(
        "200 OK",
        r#"[{"transactionID":"tx-other","state":"STATE_CONFIRMED","type":"SAFE","nonce":"60","owner":"0x4444444444444444444444444444444444444444"}]"#,
    );
    let provider = sample_reconcile_provider(server.base_url());

    let outcome = provider
        .reconcile_live(&sample_pending_work("tx:tx-target"))
        .expect("unrelated relayer rows should not resolve the work");

    assert!(matches!(outcome, ReconcileOutcome::StillPending));
    let _ = server.finish();
}

#[tokio::test]
async fn polymarket_reconcile_provider_confirms_matching_tx_pending_ref() {
    let server = MockServer::spawn(
        "200 OK",
        r#"[{"transactionID":"tx-other","state":"STATE_CONFIRMED","type":"SAFE","nonce":"60","owner":"0x4444444444444444444444444444444444444444"},{"transactionID":"tx-1","transactionHash":"0xtx-1","state":"STATE_CONFIRMED","type":"SAFE","nonce":"61","owner":"0x4444444444444444444444444444444444444444"}]"#,
    );
    let provider = sample_reconcile_provider(server.base_url());

    let outcome = provider
        .reconcile_live(&sample_pending_work("tx:0xtx-1"))
        .expect("confirmed tx should resolve authoritatively");

    match outcome {
        ReconcileOutcome::ConfirmedAuthoritative { submission_ref } => {
            assert_eq!(submission_ref, "0xtx-1");
        }
        other => panic!("unexpected outcome: {other:?}"),
    }

    let _ = server.finish();
}

#[tokio::test]
async fn polymarket_reconcile_provider_confirms_matching_open_order_for_order_pending_ref() {
    let server = MockServer::spawn(
        "200 OK",
        r#"[{"id":"0xorder-open","status":"LIVE","market":"market-1"}]"#,
    );
    let provider = sample_reconcile_provider(server.base_url());

    let outcome = provider
        .reconcile_live(&sample_pending_work("order:0xorder-open"))
        .expect("open order should confirm authoritatively");

    match outcome {
        ReconcileOutcome::ConfirmedAuthoritative { submission_ref } => {
            assert_eq!(submission_ref, "0xorder-open");
        }
        other => panic!("unexpected outcome: {other:?}"),
    }

    let request = server.finish();
    assert!(request.starts_with("GET /data/orders?"));
    assert!(request.contains("poly-address: 0xowner"));
}

#[tokio::test]
async fn polymarket_reconcile_provider_confirms_gateway_open_order_without_http_side_effects() {
    let server = MockServer::spawn("200 OK", r#"{"unused":true}"#);
    let provider = sample_reconcile_provider_with_gateway(
        server.base_url(),
        scripted_gateway_with_open_orders(vec![scripted_open_order("order-gateway-open")]),
    );

    let outcome = provider
        .reconcile_live(&sample_pending_work("order:order-gateway-open"))
        .expect("gateway-backed open orders should confirm");

    match outcome {
        ReconcileOutcome::ConfirmedAuthoritative { submission_ref } => {
            assert_eq!(submission_ref, "order-gateway-open");
        }
        other => panic!("unexpected outcome: {other:?}"),
    }

    server.finish_without_request();
}

#[tokio::test]
async fn polymarket_reconcile_provider_confirms_gateway_tx_without_http_side_effects() {
    let server = MockServer::spawn("200 OK", r#"{"unused":true}"#);
    let provider = sample_reconcile_provider_with_gateway(
        server.base_url(),
        scripted_gateway_with_relayer(
            Ok(vec![confirmed_transaction("0xtx-gateway")]),
            Ok("60".to_owned()),
        ),
    );

    let outcome = provider
        .reconcile_live(&sample_pending_work("tx:0xtx-gateway"))
        .expect("gateway-backed tx should confirm");

    match outcome {
        ReconcileOutcome::ConfirmedAuthoritative { submission_ref } => {
            assert_eq!(submission_ref, "0xtx-gateway");
        }
        other => panic!("unexpected outcome: {other:?}"),
    }

    server.finish_without_request();
}

#[tokio::test]
async fn polymarket_reconcile_provider_maps_gateway_relayer_errors_to_provider_errors() {
    let server = MockServer::spawn("200 OK", r#"{"unused":true}"#);
    let provider = sample_reconcile_provider_with_gateway(
        server.base_url(),
        scripted_gateway_with_relayer(
            Err(venue_polymarket::PolymarketGatewayError::relayer(
                "gateway relayer down",
            )),
            Ok("60".to_owned()),
        ),
    );

    let err = provider
        .reconcile_live(&sample_pending_work("tx:0xtx-gateway"))
        .expect_err("gateway relayer error should surface");

    assert!(err.reason.contains("gateway relayer down"));
    server.finish_without_request();
}

#[tokio::test]
async fn polymarket_reconcile_provider_surfaces_transport_failures_as_provider_errors() {
    let server = MockServer::spawn("500 Internal Server Error", r#"{"error":"boom"}"#);
    let provider = sample_reconcile_provider(server.base_url());

    let err = provider
        .reconcile_live(&sample_pending_work("tx:pending-1"))
        .expect_err("transport failure should stay an error");

    assert!(err.reason.contains("500"));
    let _ = server.finish();
}

#[test]
fn polymarket_submit_provider_with_gateway_runtime_executes_on_supplied_runtime() {
    let server = MockServer::spawn("200 OK", r#"{"unused":true}"#);
    let recorder = Arc::new(Mutex::new(Vec::new()));
    let runtime = SharedRuntimeHarness::spawn("shared-gateway-submit");
    let provider = sample_submit_provider_with_gateway_runtime(
        server.base_url(),
        PolymarketGateway::from_clob_api(Arc::new(ThreadRecordingClobApi {
            submit_threads: recorder.clone(),
        })),
        runtime.handle(),
    );

    let outcome = provider
        .submit_family(&sample_signed_submission(), &sample_attempt())
        .expect("shared-runtime gateway submit should succeed");

    match outcome {
        LiveSubmitOutcome::Accepted { submission_record } => {
            assert_eq!(submission_record.submission_ref, "order-threaded");
        }
        other => panic!("unexpected outcome: {other:?}"),
    }

    let recorded = recorder.lock().expect("submit thread recorder").clone();
    assert_eq!(recorded, vec!["shared-gateway-submit".to_owned()]);
    server.finish_without_request();
}

#[test]
fn polymarket_reconcile_provider_with_gateway_runtime_executes_on_supplied_runtime() {
    let server = MockServer::spawn("200 OK", r#"{"unused":true}"#);
    let recorder = Arc::new(Mutex::new(Vec::new()));
    let runtime = SharedRuntimeHarness::spawn("shared-gateway-reconcile");
    let provider = sample_reconcile_provider_with_gateway_runtime(
        server.base_url(),
        PolymarketGateway::from_relayer_api(Arc::new(ThreadRecordingRelayerApi {
            recent_transactions: vec![confirmed_transaction("0xtx-shared-runtime")],
            recent_threads: recorder.clone(),
        })),
        runtime.handle(),
    );

    let outcome = provider
        .reconcile_live(&sample_pending_work("tx:0xtx-shared-runtime"))
        .expect("shared-runtime gateway reconcile should succeed");

    match outcome {
        ReconcileOutcome::ConfirmedAuthoritative { submission_ref } => {
            assert_eq!(submission_ref, "0xtx-shared-runtime");
        }
        other => panic!("unexpected outcome: {other:?}"),
    }

    let recorded = recorder.lock().expect("reconcile thread recorder").clone();
    assert_eq!(recorded, vec!["shared-gateway-reconcile".to_owned()]);
    server.finish_without_request();
}

#[test]
fn polymarket_reconcile_open_orders_with_gateway_runtime_executes_on_supplied_runtime() {
    let server = MockServer::spawn("200 OK", r#"{"unused":true}"#);
    let recorder = Arc::new(Mutex::new(Vec::new()));
    let runtime = SharedRuntimeHarness::spawn("shared-gateway-open-orders");
    let provider = sample_reconcile_provider_with_gateway_runtime(
        server.base_url(),
        PolymarketGateway::from_clob_api(Arc::new(ThreadRecordingOpenOrdersClobApi {
            open_orders: vec![scripted_open_order("order-shared-open")],
            open_order_threads: recorder.clone(),
        })),
        runtime.handle(),
    );

    let outcome = provider
        .reconcile_live(&sample_pending_work("order:order-shared-open"))
        .expect("shared-runtime open orders reconcile should succeed");

    match outcome {
        ReconcileOutcome::ConfirmedAuthoritative { submission_ref } => {
            assert_eq!(submission_ref, "order-shared-open");
        }
        other => panic!("unexpected outcome: {other:?}"),
    }

    let recorded = recorder.lock().expect("open-order thread recorder").clone();
    assert_eq!(recorded, vec!["shared-gateway-open-orders".to_owned()]);
    server.finish_without_request();
}

#[test]
fn polymarket_submit_provider_with_shutdown_runtime_surfaces_provider_error() {
    let server = MockServer::spawn("200 OK", r#"{"unused":true}"#);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .expect("runtime should build");
    let runtime_handle = runtime.handle().clone();
    drop(runtime);
    let provider = sample_submit_provider_with_gateway_runtime(
        server.base_url(),
        scripted_gateway_with_open_orders(Vec::new()),
        runtime_handle,
    );

    let err = provider
        .submit_family(&sample_signed_submission(), &sample_attempt())
        .expect_err("shutdown runtime should surface as a provider error");

    assert!(err.reason.contains("runtime"));
    server.finish_without_request();
}

#[test]
fn polymarket_reconcile_provider_with_shutdown_runtime_surfaces_provider_error() {
    let server = MockServer::spawn("200 OK", r#"{"unused":true}"#);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .expect("runtime should build");
    let runtime_handle = runtime.handle().clone();
    drop(runtime);
    let provider = sample_reconcile_provider_with_gateway_runtime(
        server.base_url(),
        scripted_gateway_with_open_orders(vec![scripted_open_order("order-unused")]),
        runtime_handle,
    );

    let err = provider
        .reconcile_live(&sample_pending_work("order:order-unused"))
        .expect_err("shutdown runtime should surface as a reconcile error");

    assert!(err.reason.contains("runtime"));
    server.finish_without_request();
}

fn sample_submit_provider(base_url: Url) -> PolymarketNegRiskSubmitProvider<'static> {
    PolymarketNegRiskSubmitProvider::new(
        sample_rest_client(base_url),
        sample_l2_auth(),
        sample_post_order_transport(),
    )
}

fn sample_submit_provider_with_gateway(
    base_url: Url,
    gateway: PolymarketGateway,
) -> PolymarketNegRiskSubmitProvider<'static> {
    PolymarketNegRiskSubmitProvider::with_gateway(
        sample_rest_client(base_url),
        sample_l2_auth(),
        sample_post_order_transport(),
        gateway,
    )
}

fn sample_submit_provider_with_gateway_runtime(
    base_url: Url,
    gateway: PolymarketGateway,
    runtime_handle: tokio::runtime::Handle,
) -> PolymarketNegRiskSubmitProvider<'static> {
    PolymarketNegRiskSubmitProvider::with_gateway_runtime(
        sample_rest_client(base_url),
        sample_l2_auth(),
        sample_post_order_transport(),
        gateway,
        runtime_handle,
    )
}

fn sample_reconcile_provider(base_url: Url) -> PolymarketNegRiskReconcileProvider<'static> {
    PolymarketNegRiskReconcileProvider::new(
        sample_rest_client(base_url),
        sample_l2_auth(),
        sample_relayer_auth(),
    )
}

fn sample_reconcile_provider_with_gateway(
    base_url: Url,
    gateway: PolymarketGateway,
) -> PolymarketNegRiskReconcileProvider<'static> {
    PolymarketNegRiskReconcileProvider::with_gateway(
        sample_rest_client(base_url),
        sample_l2_auth(),
        sample_relayer_auth(),
        gateway,
    )
}

fn sample_reconcile_provider_with_gateway_runtime(
    base_url: Url,
    gateway: PolymarketGateway,
    runtime_handle: tokio::runtime::Handle,
) -> PolymarketNegRiskReconcileProvider<'static> {
    PolymarketNegRiskReconcileProvider::with_gateway_runtime(
        sample_rest_client(base_url),
        sample_l2_auth(),
        sample_relayer_auth(),
        gateway,
        runtime_handle,
    )
}

fn sample_rest_client(base_url: Url) -> PolymarketRestClient {
    let client = reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("test client");

    PolymarketRestClient::with_http_client(
        client,
        base_url.clone(),
        base_url.clone(),
        base_url,
        None,
    )
}

fn sample_l2_auth() -> L2AuthHeaders<'static> {
    L2AuthHeaders {
        signer: SignerContext {
            address: "0xowner",
            funder_address: "0xfunder",
            signature_type: SignatureType::Eoa,
            wallet_route: WalletRoute::Eoa,
        },
        api_key: "key-1",
        passphrase: "pass-1",
        timestamp: "1700000000",
        signature: "0xsig",
    }
}

fn sample_relayer_auth() -> RelayerAuth<'static> {
    RelayerAuth::BuilderApiKey {
        api_key: "builder-key-1",
        timestamp: "1700000000",
        passphrase: "builder-pass-1",
        signature: "0xbuilder",
    }
}

fn sample_attempt() -> ExecutionAttemptContext {
    ExecutionAttemptContext {
        attempt_id: "attempt-1".to_owned(),
        snapshot_id: "snapshot-1".to_owned(),
        execution_mode: ExecutionMode::Live,
        route: "neg-risk".to_owned(),
        scope: "family-a".to_owned(),
        matched_rule_id: None,
    }
}

fn sample_pending_work(pending_ref: &str) -> PendingReconcileWork {
    PendingReconcileWork {
        pending_ref: pending_ref.to_owned(),
        route: "neg-risk".to_owned(),
        scope: "family-a".to_owned(),
    }
}

fn sample_signed_submission() -> SignedFamilySubmission {
    SignedFamilySubmission {
        plan_id: "plan-1".to_owned(),
        members: vec![execution::signing::SignedFamilyMember {
            condition_id: domain::ConditionId::from("condition-1"),
            token_id: domain::TokenId::from("token-1"),
            price: Decimal::new(45, 2),
            quantity: Decimal::new(10, 0),
            maker: "0xmaker".to_owned(),
            signer: "0xsigner".to_owned(),
            taker: "0x0000000000000000000000000000000000000000".to_owned(),
            maker_amount: "10".to_owned(),
            taker_amount: "5".to_owned(),
            side: "BUY".to_owned(),
            expiration: "0".to_owned(),
            fee_rate_bps: "30".to_owned(),
            signature_type: 0,
            identity: domain::SignedOrderIdentity {
                signed_order_hash: "hash-1".to_owned(),
                salt: "123".to_owned(),
                nonce: "0".to_owned(),
                signature: "sig-1".to_owned(),
            },
        }],
    }
}

fn sample_multi_member_submission() -> SignedFamilySubmission {
    let mut signed = sample_signed_submission();
    signed.members.push(execution::signing::SignedFamilyMember {
        condition_id: domain::ConditionId::from("condition-2"),
        token_id: domain::TokenId::from("token-2"),
        price: Decimal::new(55, 2),
        quantity: Decimal::new(8, 0),
        maker: "0xmaker".to_owned(),
        signer: "0xsigner".to_owned(),
        taker: "0x0000000000000000000000000000000000000000".to_owned(),
        maker_amount: "8".to_owned(),
        taker_amount: "4.4".to_owned(),
        side: "BUY".to_owned(),
        expiration: "0".to_owned(),
        fee_rate_bps: "30".to_owned(),
        signature_type: 0,
        identity: domain::SignedOrderIdentity {
            signed_order_hash: "hash-2".to_owned(),
            salt: "124".to_owned(),
            nonce: "1".to_owned(),
            signature: "sig-2".to_owned(),
        },
    });
    signed
}

fn confirmed_transaction(tx_ref: &str) -> RelayerTransaction {
    RelayerTransaction {
        transaction_id: tx_ref.to_owned(),
        transaction_hash: Some(tx_ref.to_owned()),
        from_address: None,
        to_address: None,
        proxy_address: None,
        nonce: Some("60".to_owned()),
        state: Some("STATE_CONFIRMED".to_owned()),
        wallet_type: Some(RelayerTransactionType::Safe),
        owner: Some("0x4444444444444444444444444444444444444444".to_owned()),
        created_at: None,
        updated_at: None,
        data: None,
        value: None,
        signature: None,
        metadata: None,
    }
}

fn sample_post_order_transport() -> PostOrderTransport {
    PostOrderTransport {
        owner: "owner-uuid".to_owned(),
        order_type: OrderType::Gtc,
        defer_exec: false,
    }
}

struct MockServer {
    request: std::sync::Arc<std::sync::Mutex<Option<String>>>,
    join: thread::JoinHandle<()>,
    addr: std::net::SocketAddr,
}

#[derive(Debug)]
struct SharedRuntimeHarness {
    handle: tokio::runtime::Handle,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    join: Option<thread::JoinHandle<()>>,
}

impl SharedRuntimeHarness {
    fn spawn(thread_name: &str) -> Self {
        let (handle_tx, handle_rx) = mpsc::sync_channel(1);
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
        let worker_name = thread_name.to_owned();
        let join = thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(1)
                .thread_name(worker_name)
                .enable_all()
                .build()
                .expect("shared runtime should build");
            handle_tx
                .send(runtime.handle().clone())
                .expect("shared runtime handle should send");
            runtime.block_on(async move {
                let _ = shutdown_rx.await;
            });
        });
        let handle = handle_rx
            .recv()
            .expect("shared runtime handle should be received");

        Self {
            handle,
            shutdown: Some(shutdown_tx),
            join: Some(join),
        }
    }

    fn handle(&self) -> tokio::runtime::Handle {
        self.handle.clone()
    }
}

impl Drop for SharedRuntimeHarness {
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        if let Some(join) = self.join.take() {
            join.join().expect("shared runtime thread should join");
        }
    }
}

#[derive(Debug)]
struct ThreadRecordingClobApi {
    submit_threads: Arc<Mutex<Vec<String>>>,
}

#[async_trait]
impl PolymarketClobApi for ThreadRecordingClobApi {
    async fn open_orders(
        &self,
        _query: &venue_polymarket::PolymarketOrderQuery,
    ) -> Result<Vec<venue_polymarket::PolymarketOpenOrderSummary>, PolymarketGatewayError> {
        Ok(Vec::new())
    }

    async fn submit_order(
        &self,
        _order: &PolymarketSignedOrder,
    ) -> Result<PolymarketSubmitResponse, PolymarketGatewayError> {
        self.submit_threads
            .lock()
            .expect("submit thread recorder")
            .push(current_thread_name());
        Ok(PolymarketSubmitResponse {
            order_id: "order-threaded".to_owned(),
            status: "LIVE".to_owned(),
            success: true,
            error_message: None,
            transaction_hashes: Vec::new(),
        })
    }

    async fn post_heartbeat(
        &self,
        _previous_heartbeat_id: Option<&str>,
    ) -> Result<PolymarketHeartbeatStatus, PolymarketGatewayError> {
        Ok(PolymarketHeartbeatStatus {
            heartbeat_id: "hb-threaded".to_owned(),
            valid: true,
        })
    }
}

#[derive(Debug)]
struct ThreadRecordingOpenOrdersClobApi {
    open_orders: Vec<venue_polymarket::PolymarketOpenOrderSummary>,
    open_order_threads: Arc<Mutex<Vec<String>>>,
}

#[async_trait]
impl PolymarketClobApi for ThreadRecordingOpenOrdersClobApi {
    async fn open_orders(
        &self,
        _query: &venue_polymarket::PolymarketOrderQuery,
    ) -> Result<Vec<venue_polymarket::PolymarketOpenOrderSummary>, PolymarketGatewayError> {
        self.open_order_threads
            .lock()
            .expect("open-order thread recorder")
            .push(current_thread_name());
        Ok(self.open_orders.clone())
    }

    async fn submit_order(
        &self,
        _order: &PolymarketSignedOrder,
    ) -> Result<PolymarketSubmitResponse, PolymarketGatewayError> {
        Err(PolymarketGatewayError::protocol(
            "submit_order should not be called in open-order reconcile tests",
        ))
    }

    async fn post_heartbeat(
        &self,
        _previous_heartbeat_id: Option<&str>,
    ) -> Result<PolymarketHeartbeatStatus, PolymarketGatewayError> {
        Ok(PolymarketHeartbeatStatus {
            heartbeat_id: "hb-open-order-threaded".to_owned(),
            valid: true,
        })
    }
}

#[derive(Debug)]
struct ThreadRecordingRelayerApi {
    recent_transactions: Vec<RelayerTransaction>,
    recent_threads: Arc<Mutex<Vec<String>>>,
}

#[async_trait]
impl PolymarketRelayerApi for ThreadRecordingRelayerApi {
    async fn recent_transactions(
        &self,
        _auth: &RelayerAuth<'_>,
    ) -> Result<Vec<RelayerTransaction>, PolymarketGatewayError> {
        self.recent_threads
            .lock()
            .expect("reconcile thread recorder")
            .push(current_thread_name());
        Ok(self.recent_transactions.clone())
    }

    async fn current_nonce(
        &self,
        _auth: &RelayerAuth<'_>,
        _address: &str,
        _wallet_type: RelayerTransactionType,
    ) -> Result<String, PolymarketGatewayError> {
        Ok("60".to_owned())
    }
}

fn current_thread_name() -> String {
    thread::current()
        .name()
        .unwrap_or("unnamed-thread")
        .to_owned()
}

impl MockServer {
    fn spawn(status: &'static str, body: &'static str) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test listener");
        listener
            .set_nonblocking(true)
            .expect("set listener nonblocking");
        let addr = listener.local_addr().expect("local addr");
        let request = std::sync::Arc::new(std::sync::Mutex::new(None));
        let captured = request.clone();
        let deadline = Instant::now() + Duration::from_millis(300);

        let join = thread::spawn(move || loop {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let mut buf = [0_u8; 8192];
                    let mut request_text = Vec::new();
                    loop {
                        match stream.read(&mut buf) {
                            Ok(0) => break,
                            Ok(n) => {
                                request_text.extend_from_slice(&buf[..n]);
                                if request_text.windows(4).any(|window| window == b"\r\n\r\n") {
                                    break;
                                }
                            }
                            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                                thread::sleep(Duration::from_millis(10));
                            }
                            Err(err) => panic!("request read failed: {err}"),
                        }
                    }

                    *captured.lock().unwrap() =
                        Some(String::from_utf8_lossy(&request_text).into_owned());

                    let response = format!(
                        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    stream
                        .write_all(response.as_bytes())
                        .expect("write response");
                    break;
                }
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    if Instant::now() >= deadline {
                        break;
                    }
                    thread::sleep(Duration::from_millis(10));
                }
                Err(err) => panic!("accept failed: {err}"),
            }
        });

        Self {
            request,
            join,
            addr,
        }
    }

    fn base_url(&self) -> Url {
        Url::parse(&format!("http://{}/", self.addr)).expect("base url")
    }

    fn finish(self) -> String {
        self.join.join().expect("server thread should finish");
        self.request
            .lock()
            .unwrap()
            .clone()
            .expect("request should be captured")
    }

    fn finish_without_request(self) {
        self.join.join().expect("server thread should finish");
        assert!(self.request.lock().unwrap().is_none());
    }
}
