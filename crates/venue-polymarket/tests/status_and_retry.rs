use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::mpsc,
    thread,
    time::Duration,
};

use domain::{
    OrderId, RuntimeMode, RuntimeOverlay, SignatureType, SignedOrderIdentity, WalletRoute,
};
use reqwest::StatusCode;
use url::Url;
use venue_polymarket::{
    map_venue_status, AuthError, BusinessErrorKind, HttpRetryContext, L2AuthHeaders,
    PolymarketRestClient, RelayerAuth, RelayerTransaction, RelayerTransactionType, RestError,
    RetryClass, RetryDecision, SignerContext,
};

#[test]
fn http_503_cancel_only_maps_to_no_new_risk_cancel_only() {
    let mapped = map_venue_status(503, Some("cancel-only"));

    assert_eq!(mapped.mode, RuntimeMode::NoNewRisk);
    assert_eq!(mapped.overlay, Some(RuntimeOverlay::CancelOnly));
}

#[test]
fn http_503_trading_disabled_halts_globally() {
    let mapped = map_venue_status(503, Some("trading-disabled"));

    assert_eq!(mapped.mode, RuntimeMode::GlobalHalt);
    assert_eq!(mapped.overlay, None);
}

#[test]
fn http_425_is_transport_retry_that_forces_reconciling() {
    let retry = RetryDecision::for_http_status(425, None, Some(&sample_signed_order()));

    assert_eq!(retry.class, RetryClass::Transport);
    assert!(retry.reuse_payload);
    assert!(retry.backoff);
    assert_eq!(retry.next_mode, Some(RuntimeMode::Reconciling));
    assert_eq!(retry.preserved_identity, Some(sample_signed_order()));
}

#[test]
fn transport_retry_preserves_signed_order_identity() {
    let retry = RetryDecision::for_transport_timeout(&sample_signed_order());

    assert!(retry.reuse_payload);
    assert!(retry.reconcile_first);
    assert_eq!(retry.preserved_identity, Some(sample_signed_order()));
}

#[test]
fn duplicate_signed_order_requires_reconcile_before_business_retry() {
    let retry =
        RetryDecision::for_duplicate_signed_order(sample_order_id(), &sample_signed_order());

    assert_eq!(retry.class, RetryClass::Business);
    assert!(!retry.reuse_payload);
    assert!(retry.reconcile_first);
    assert_eq!(retry.retry_of_order_id, Some(sample_order_id()));
    assert_eq!(retry.next_mode, Some(RuntimeMode::Reconciling));
}

#[test]
fn malformed_or_tick_size_rejections_are_terminal_business_failures() {
    let retry = RetryDecision::for_business_error(
        BusinessErrorKind::TickSize,
        Some(sample_order_id()),
        None,
    );

    assert_eq!(retry.class, RetryClass::None);
    assert!(!retry.reuse_payload);
    assert!(!retry.reconcile_first);
    assert_eq!(retry.retry_of_order_id, None);
    assert_eq!(retry.next_mode, None);
}

#[test]
fn insufficient_allowance_rejection_forces_no_new_risk_without_retry() {
    let retry = RetryDecision::for_business_error(
        BusinessErrorKind::InsufficientAllowance,
        Some(sample_order_id()),
        None,
    );

    assert_eq!(retry.class, RetryClass::None);
    assert!(!retry.reuse_payload);
    assert!(retry.reconcile_first);
    assert_eq!(retry.next_mode, Some(RuntimeMode::NoNewRisk));
}

#[test]
fn persistent_http_429_degrades_runtime_mode() {
    let retry = RetryDecision::for_http_status_with_context(
        429,
        None,
        Some(&sample_signed_order()),
        HttpRetryContext {
            persistent_rate_limit: true,
        },
    );

    assert_eq!(retry.class, RetryClass::Transport);
    assert!(retry.reuse_payload);
    assert!(retry.backoff);
    assert_eq!(retry.next_mode, Some(RuntimeMode::Degraded));
}

#[test]
fn balance_allowance_request_is_authenticated_and_signer_aware() {
    let client = sample_client();
    let request = client
        .build_balance_allowance_request(&sample_auth(), "0xtoken")
        .expect("request should build");

    assert_eq!(request.method().as_str(), "GET");
    assert_eq!(request.url().path(), "/balance-allowance");
    assert_eq!(header_value(request.headers(), "poly-address"), "0xowner");
    assert_eq!(
        header_value(request.headers(), "poly-signature-type"),
        "EOA"
    );
    assert_eq!(header_value(request.headers(), "poly-api-key"), "key-1");

    let query = request.url().query().expect("query");
    assert!(query.contains("owner=0xowner"));
    assert!(query.contains("funder=0xfunder"));
    assert!(query.contains("asset=0xtoken"));
    assert!(query.contains("signature_type=EOA"));
    assert!(query.contains("wallet_route=eoa"));
}

#[test]
fn open_orders_request_uses_authenticated_signer_context() {
    let client = sample_client();
    let request = client
        .build_open_orders_request(&sample_auth())
        .expect("request should build");

    assert_eq!(request.method().as_str(), "GET");
    assert_eq!(request.url().path(), "/orders");
    assert_eq!(header_value(request.headers(), "poly-address"), "0xowner");
    assert!(request
        .url()
        .query()
        .expect("query")
        .contains("funder=0xfunder"));
}

#[test]
fn proxy_balance_allowance_request_uses_documented_signature_type_value() {
    let client = sample_client();
    let request = client
        .build_balance_allowance_request(&sample_proxy_auth(), "0xtoken")
        .expect("proxy request should build");

    assert_eq!(
        header_value(request.headers(), "poly-signature-type"),
        "POLY_PROXY"
    );
    let query = request.url().query().expect("query");
    assert!(query.contains("signature_type=POLY_PROXY"));
    assert!(query.contains("wallet_route=proxy"));
}

#[test]
fn safe_open_orders_request_uses_documented_signature_type_value() {
    let client = sample_client();
    let request = client
        .build_open_orders_request(&sample_safe_auth())
        .expect("safe request should build");

    assert_eq!(
        header_value(request.headers(), "poly-signature-type"),
        "GNOSIS_SAFE"
    );
    let query = request.url().query().expect("query");
    assert!(query.contains("signature_type=GNOSIS_SAFE"));
    assert!(query.contains("wallet_route=safe"));
}

#[test]
fn authenticated_requests_reject_empty_funder_address() {
    let client = sample_client();
    let err = client
        .build_open_orders_request(&sample_auth_with_funder(""))
        .expect_err("empty funder_address should fail");

    match err {
        RestError::Auth(AuthError::EmptyField("funder_address")) => {}
        other => panic!("unexpected error: {other:?}"),
    }
}

#[tokio::test]
async fn fetch_clob_status_preserves_non_success_status_and_body() {
    let server = MockServer::spawn(
        "422 Unprocessable Entity",
        r#"{"error":"duplicate signed order"}"#,
    );
    let client = sample_client_for(server.base_url());

    let err = client
        .fetch_clob_status()
        .await
        .expect_err("non-2xx should preserve status and body");
    let request = server.finish();

    assert!(request.starts_with("GET /status HTTP/1.1"));
    match err {
        RestError::HttpResponse { status, body } => {
            assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
            assert!(body.contains("duplicate signed order"));
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[tokio::test]
async fn fetch_open_orders_preserves_authenticated_error_body() {
    let server = MockServer::spawn("400 Bad Request", r#"{"error":"tick size violation"}"#);
    let client = sample_client_for(server.base_url());

    let err = client
        .fetch_open_orders(&sample_auth())
        .await
        .expect_err("non-2xx should preserve authenticated error details");
    let request = server.finish();

    assert!(request.starts_with("GET /orders?"));
    assert!(request.contains("poly-address: 0xowner"));
    match err {
        RestError::HttpResponse { status, body } => {
            assert_eq!(status, StatusCode::BAD_REQUEST);
            assert!(body.contains("tick size violation"));
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[tokio::test]
async fn fetch_recent_transactions_uses_builder_auth_and_documented_shape() {
    let server = MockServer::spawn(
        "200 OK",
        r#"[{"transactionID":"tx-1","transactionHash":"0xhash","from":"0x1111111111111111111111111111111111111111","to":"0x2222222222222222222222222222222222222222","proxyAddress":"0x3333333333333333333333333333333333333333","nonce":"60","state":"STATE_CONFIRMED","type":"SAFE","owner":"0x4444444444444444444444444444444444444444","createdAt":"2024-07-14T21:13:08.819782Z","updatedAt":"2024-07-14T21:13:46.576639Z"}]"#,
    );
    let client = sample_client_for(server.base_url());

    let transactions = client
        .fetch_recent_transactions(&sample_builder_relayer_auth())
        .await
        .expect("transactions should parse");
    let request = server.finish();

    assert!(request.starts_with("GET /transactions HTTP/1.1"));
    assert!(request.contains("poly-builder-api-key: builder-key-1"));
    assert!(request.contains("poly-builder-timestamp: 1700000000"));
    assert!(request.contains("poly-builder-passphrase: builder-pass-1"));
    assert!(request.contains("poly-builder-signature: 0xbuilder"));
    assert_eq!(transactions.len(), 1);
    assert_eq!(transactions[0].transaction_id, "tx-1");
    assert_eq!(transactions[0].transaction_hash.as_deref(), Some("0xhash"));
    assert_eq!(
        transactions[0].from_address.as_deref(),
        Some("0x1111111111111111111111111111111111111111")
    );
    assert_eq!(
        transactions[0].proxy_address.as_deref(),
        Some("0x3333333333333333333333333333333333333333")
    );
    assert_eq!(
        transactions[0].wallet_type,
        Some(RelayerTransactionType::Safe)
    );
    assert_eq!(transactions[0].state.as_deref(), Some("STATE_CONFIRMED"));
    assert_eq!(
        transactions[0].created_at.as_deref(),
        Some("2024-07-14T21:13:08.819782Z")
    );
}

#[tokio::test]
async fn fetch_current_nonce_uses_documented_query_and_relayer_auth() {
    let server = MockServer::spawn("200 OK", r#"{"nonce":"31"}"#);
    let client = sample_client_for(server.base_url());

    let nonce = client
        .fetch_current_nonce(
            &sample_relayer_api_auth(),
            "0x5555555555555555555555555555555555555555",
            RelayerTransactionType::Proxy,
        )
        .await
        .expect("nonce should parse");
    let request = server.finish();

    assert!(request.starts_with("GET /nonce?"));
    assert!(request.contains("address=0x5555555555555555555555555555555555555555"));
    assert!(request.contains("type=PROXY"));
    assert!(request.contains("relayer-api-key: relayer-key-1"));
    assert!(request.contains("relayer-api-key-address: 0x6666666666666666666666666666666666666666"));
    assert_eq!(nonce, "31");
}

#[test]
fn relayer_transaction_pending_or_unknown_states_are_treated_as_pending() {
    let pending: RelayerTransaction =
        serde_json::from_str(r#"{"transactionID":"tx-2","state":"STATE_EXECUTED","type":"SAFE"}"#)
            .expect("pending tx should deserialize");
    let unknown: RelayerTransaction =
        serde_json::from_str(r#"{"transactionID":"tx-3","state":"STATE_MYSTERY","type":"SAFE"}"#)
            .expect("unknown tx should deserialize");

    assert!(pending.state_is_pending_or_unknown());
    assert!(unknown.state_is_pending_or_unknown());
    assert!(!pending.state_is_confirmed());
    assert!(!unknown.state_is_confirmed());
}

fn sample_signed_order() -> SignedOrderIdentity {
    SignedOrderIdentity {
        signed_order_hash: "0xhash".to_owned(),
        salt: "123".to_owned(),
        nonce: "7".to_owned(),
        signature: "0xsig".to_owned(),
    }
}

fn sample_order_id() -> OrderId {
    OrderId::new("ord_123")
}

fn sample_client() -> PolymarketRestClient {
    sample_client_for(Url::parse("https://clob.polymarket.com/").expect("clob host"))
}

fn sample_auth() -> L2AuthHeaders<'static> {
    sample_auth_with_funder("0xfunder")
}

fn sample_auth_with_funder(funder_address: &'static str) -> L2AuthHeaders<'static> {
    L2AuthHeaders {
        signer: SignerContext {
            address: "0xowner",
            funder_address,
            signature_type: SignatureType::Eoa,
            wallet_route: WalletRoute::Eoa,
        },
        api_key: "key-1",
        passphrase: "pass-1",
        timestamp: "1700000000",
        signature: "0xsig",
    }
}

fn sample_proxy_auth() -> L2AuthHeaders<'static> {
    L2AuthHeaders {
        signer: SignerContext {
            address: "0xproxyowner",
            funder_address: "0xproxyfunder",
            signature_type: SignatureType::Proxy,
            wallet_route: WalletRoute::Proxy,
        },
        api_key: "proxy-key-1",
        passphrase: "proxy-pass-1",
        timestamp: "1700000001",
        signature: "0xproxysig",
    }
}

fn sample_safe_auth() -> L2AuthHeaders<'static> {
    L2AuthHeaders {
        signer: SignerContext {
            address: "0xsafeowner",
            funder_address: "0xsafefunder",
            signature_type: SignatureType::Safe,
            wallet_route: WalletRoute::Safe,
        },
        api_key: "safe-key-1",
        passphrase: "safe-pass-1",
        timestamp: "1700000002",
        signature: "0xsafesig",
    }
}

fn sample_builder_relayer_auth() -> RelayerAuth<'static> {
    RelayerAuth::BuilderApiKey {
        api_key: "builder-key-1",
        timestamp: "1700000000",
        passphrase: "builder-pass-1",
        signature: "0xbuilder",
    }
}

fn sample_relayer_api_auth() -> RelayerAuth<'static> {
    RelayerAuth::RelayerApiKey {
        api_key: "relayer-key-1",
        address: "0x6666666666666666666666666666666666666666",
    }
}

fn header_value(headers: &reqwest::header::HeaderMap, key: &str) -> String {
    headers
        .get(key)
        .expect("header present")
        .to_str()
        .expect("header is valid utf-8")
        .to_owned()
}

fn sample_client_for(base_url: Url) -> PolymarketRestClient {
    let client = reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("test client");

    PolymarketRestClient::with_http_client(client, base_url.clone(), base_url.clone(), base_url)
}

struct MockServer {
    base_url: Url,
    request_rx: mpsc::Receiver<String>,
    handle: Option<thread::JoinHandle<()>>,
}

impl MockServer {
    fn spawn(status_line: &str, body: &str) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
        let address = listener.local_addr().expect("server addr");
        let (request_tx, request_rx) = mpsc::channel();
        let status_line = status_line.to_owned();
        let body = body.to_owned();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept request");
            let mut buffer = Vec::new();
            let mut chunk = [0_u8; 1024];

            loop {
                let read = stream.read(&mut chunk).expect("read request");
                if read == 0 {
                    break;
                }

                buffer.extend_from_slice(&chunk[..read]);
                if buffer.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }

            request_tx
                .send(String::from_utf8_lossy(&buffer).into_owned())
                .expect("send request");

            let response = format!(
                "HTTP/1.1 {status_line}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            stream
                .write_all(response.as_bytes())
                .expect("write response");
            stream.flush().expect("flush response");
        });

        Self {
            base_url: Url::parse(&format!("http://{address}/")).expect("base url"),
            request_rx,
            handle: Some(handle),
        }
    }

    fn base_url(&self) -> Url {
        self.base_url.clone()
    }

    fn finish(mut self) -> String {
        let request = self
            .request_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("capture request");

        if let Some(handle) = self.handle.take() {
            handle.join().expect("join server thread");
        }

        request
    }
}
