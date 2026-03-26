use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
    time::{Duration, Instant},
};

use domain::{ExecutionAttemptContext, ExecutionMode, SignatureType, WalletRoute};
use execution::providers::{ReconcileProvider, VenueExecutionProvider};
use execution::{
    LiveSubmitOutcome, PendingReconcileWork, ReconcileOutcome, SignedFamilySubmission,
};
use rust_decimal::Decimal;
use url::Url;
use venue_polymarket::{
    L2AuthHeaders, OrderType, PolymarketNegRiskReconcileProvider, PolymarketNegRiskSubmitProvider,
    PolymarketRestClient, PostOrderTransport, RelayerAuth, SignerContext,
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
    assert!(request.starts_with("GET /orders?"));
    assert!(request.contains("poly-address: 0xowner"));
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

fn sample_submit_provider(base_url: Url) -> PolymarketNegRiskSubmitProvider<'static> {
    PolymarketNegRiskSubmitProvider::new(
        sample_rest_client(base_url),
        sample_l2_auth(),
        sample_post_order_transport(),
    )
}

fn sample_reconcile_provider(base_url: Url) -> PolymarketNegRiskReconcileProvider<'static> {
    PolymarketNegRiskReconcileProvider::new(
        sample_rest_client(base_url),
        sample_l2_auth(),
        sample_relayer_auth(),
    )
}

fn sample_rest_client(base_url: Url) -> PolymarketRestClient {
    let client = reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("test client");

    PolymarketRestClient::with_http_client(client, base_url.clone(), base_url.clone(), base_url)
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
