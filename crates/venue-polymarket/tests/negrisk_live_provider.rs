mod support;

use std::{
    collections::VecDeque,
    io::{Read, Write},
    net::TcpListener,
    sync::{mpsc, Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use domain::{ExecutionAttemptContext, ExecutionMode};
use execution::providers::{ReconcileProvider, VenueExecutionProvider};
use execution::{
    LiveSubmitOutcome, PendingReconcileWork, ReconcileOutcome, SignedFamilySubmission,
};
use rust_decimal::Decimal;
use support::{
    scripted_gateway_with_open_orders, scripted_gateway_with_relayer, scripted_open_order,
};
use venue_polymarket::{
    OrderType, PolymarketClobApi, PolymarketGateway, PolymarketGatewayError,
    PolymarketHeartbeatStatus, PolymarketNegRiskReconcileProvider, PolymarketNegRiskSubmitProvider,
    PolymarketSignedOrder, PolymarketSubmitResponse, PostOrderTransport, RelayerAuth,
    RelayerTransaction, RelayerTransactionType,
};

#[tokio::test]
async fn polymarket_submit_provider_can_use_gateway_without_http_side_effects() {
    let server = MockServer::spawn("200 OK", r#"{"unused":true}"#);
    let provider =
        sample_submit_provider_with_gateway(scripted_gateway_with_open_orders(Vec::new()));

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
async fn polymarket_submit_provider_rejects_multi_member_family_before_gateway_submit() {
    let server = MockServer::spawn("200 OK", r#"{"unused":true}"#);
    let submitted_tokens = Arc::new(Mutex::new(Vec::new()));
    let gateway = PolymarketGateway::from_clob_api(Arc::new(SequencedSubmitClobApi::new(
        submitted_tokens.clone(),
        vec![
            Ok(PolymarketSubmitResponse {
                order_id: "0xorder-gw-1".to_owned(),
                status: "LIVE".to_owned(),
                success: true,
                error_message: None,
                transaction_hashes: Vec::new(),
            }),
            Err(PolymarketGatewayError::protocol("gateway submit failed")),
        ],
    )));
    let provider = sample_submit_provider_with_gateway(gateway);

    let err = provider
        .submit_family(&sample_multi_member_submission(), &sample_attempt())
        .expect_err("multi-member family should be rejected before gateway submit");

    assert!(err.reason.contains("single signed family member"));

    assert_eq!(
        submitted_tokens.lock().expect("submitted tokens").clone(),
        Vec::<String>::new()
    );
    server.finish_without_request();
}

#[tokio::test]
async fn polymarket_reconcile_provider_confirms_gateway_open_order_without_http_side_effects() {
    let server = MockServer::spawn("200 OK", r#"{"unused":true}"#);
    let provider = sample_reconcile_provider_with_gateway(scripted_gateway_with_open_orders(vec![
        scripted_open_order("order-gateway-open"),
    ]));

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
    let provider = sample_reconcile_provider_with_gateway(scripted_gateway_with_relayer(
        Ok(vec![confirmed_transaction("0xtx-gateway")]),
        Ok("60".to_owned()),
    ));

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

#[test]
fn polymarket_submit_provider_with_gateway_runtime_executes_on_supplied_runtime() {
    let server = MockServer::spawn("200 OK", r#"{"unused":true}"#);
    let recorder = Arc::new(Mutex::new(Vec::new()));
    let runtime = SharedRuntimeHarness::spawn("shared-gateway-submit");
    let provider = sample_submit_provider_with_gateway_runtime(
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
        scripted_gateway_with_open_orders(vec![scripted_open_order("order-unused")]),
        runtime_handle,
    );

    let err = provider
        .reconcile_live(&sample_pending_work("order:order-unused"))
        .expect_err("shutdown runtime should surface as a reconcile error");

    assert!(err.reason.contains("runtime"));
    server.finish_without_request();
}

fn sample_submit_provider_with_gateway(
    gateway: PolymarketGateway,
) -> PolymarketNegRiskSubmitProvider {
    PolymarketNegRiskSubmitProvider::with_gateway(sample_post_order_transport(), gateway)
}

fn sample_submit_provider_with_gateway_runtime(
    gateway: PolymarketGateway,
    runtime_handle: tokio::runtime::Handle,
) -> PolymarketNegRiskSubmitProvider {
    PolymarketNegRiskSubmitProvider::with_gateway_runtime(
        sample_post_order_transport(),
        gateway,
        runtime_handle,
    )
}

fn sample_reconcile_provider_with_gateway(
    gateway: PolymarketGateway,
) -> PolymarketNegRiskReconcileProvider<'static> {
    PolymarketNegRiskReconcileProvider::with_gateway(sample_relayer_auth(), gateway)
}

fn sample_reconcile_provider_with_gateway_runtime(
    gateway: PolymarketGateway,
    runtime_handle: tokio::runtime::Handle,
) -> PolymarketNegRiskReconcileProvider<'static> {
    PolymarketNegRiskReconcileProvider::with_gateway_runtime(
        sample_relayer_auth(),
        gateway,
        runtime_handle,
    )
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

fn current_thread_name() -> String {
    thread::current()
        .name()
        .unwrap_or("unnamed-thread")
        .to_owned()
}

#[derive(Debug)]
struct SequencedSubmitClobApi {
    submitted_tokens: Arc<Mutex<Vec<String>>>,
    submit_results: Mutex<VecDeque<Result<PolymarketSubmitResponse, PolymarketGatewayError>>>,
}

impl SequencedSubmitClobApi {
    fn new(
        submitted_tokens: Arc<Mutex<Vec<String>>>,
        submit_results: Vec<Result<PolymarketSubmitResponse, PolymarketGatewayError>>,
    ) -> Self {
        Self {
            submitted_tokens,
            submit_results: Mutex::new(VecDeque::from(submit_results)),
        }
    }
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

#[async_trait]
impl PolymarketClobApi for SequencedSubmitClobApi {
    async fn open_orders(
        &self,
        _query: &venue_polymarket::PolymarketOrderQuery,
    ) -> Result<Vec<venue_polymarket::PolymarketOpenOrderSummary>, PolymarketGatewayError> {
        Ok(Vec::new())
    }

    async fn submit_order(
        &self,
        order: &PolymarketSignedOrder,
    ) -> Result<PolymarketSubmitResponse, PolymarketGatewayError> {
        let token_id = order
            .order
            .get("tokenId")
            .and_then(|value| value.as_str())
            .unwrap_or("missing-token")
            .to_owned();
        self.submitted_tokens
            .lock()
            .expect("submitted token recorder")
            .push(token_id);

        self.submit_results
            .lock()
            .expect("sequenced submit results")
            .pop_front()
            .expect("test should provide enough submit results")
    }

    async fn post_heartbeat(
        &self,
        _previous_heartbeat_id: Option<&str>,
    ) -> Result<PolymarketHeartbeatStatus, PolymarketGatewayError> {
        Ok(PolymarketHeartbeatStatus {
            heartbeat_id: "hb-sequenced".to_owned(),
            valid: true,
        })
    }
}

impl MockServer {
    fn spawn(status: &'static str, body: &'static str) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test listener");
        listener
            .set_nonblocking(true)
            .expect("set listener nonblocking");
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

        Self { request, join }
    }

    fn finish_without_request(self) {
        self.join.join().expect("server thread should finish");
        assert!(self.request.lock().unwrap().is_none());
    }
}
