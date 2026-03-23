use domain::{
    OrderId, RuntimeMode, RuntimeOverlay, SignatureType, SignedOrderIdentity, WalletRoute,
};
use url::Url;
use venue_polymarket::{
    map_venue_status, BusinessErrorKind, HttpRetryContext, L2AuthHeaders, PolymarketRestClient,
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
    PolymarketRestClient::new(
        Url::parse("https://clob.polymarket.com/").expect("clob host"),
        Url::parse("https://data-api.polymarket.com/").expect("data api host"),
        Url::parse("https://relayer-v2.polymarket.com/").expect("relayer host"),
    )
}

fn sample_auth() -> L2AuthHeaders<'static> {
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

fn header_value(headers: &reqwest::header::HeaderMap, key: &str) -> String {
    headers
        .get(key)
        .expect("header present")
        .to_str()
        .expect("header is valid utf-8")
        .to_owned()
}
