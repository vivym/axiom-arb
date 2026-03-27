use std::{
    collections::BTreeMap,
    future::Future,
    io::{Read, Write},
    net::TcpListener,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

use domain::{SignatureType, WalletRoute};
use observability::RuntimeMetricsRecorder;
use reqwest::Url;
use tracing::{
    Event, Metadata, Subscriber,
    field::{Field, Visit},
    span::{Attributes, Id, Record},
};
use venue_polymarket::{
    L2AuthHeaders, PolymarketRestClient, RelayerAuth, SignerContext, VenueProducerInstrumentation,
};

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct CapturedSpan {
    pub name: String,
    pub fields: BTreeMap<String, String>,
}

#[allow(dead_code)]
impl CapturedSpan {
    pub fn field(&self, key: &str) -> Option<&String> {
        self.fields.get(key)
    }
}

#[allow(dead_code)]
pub fn capture_spans<T>(f: impl FnOnce() -> T) -> (Vec<CapturedSpan>, T) {
    let spans = Arc::new(Mutex::new(BTreeMap::<u64, CapturedSpan>::new()));
    let subscriber = CaptureSubscriber {
        spans: Arc::clone(&spans),
        next_id: Arc::new(AtomicU64::new(1)),
    };

    let result = tracing::subscriber::with_default(subscriber, f);
    let captured = spans
        .lock()
        .expect("capture lock poisoned")
        .values()
        .cloned()
        .collect::<Vec<_>>();

    (captured, result)
}

#[allow(dead_code)]
pub async fn capture_spans_async<T, F>(f: impl FnOnce() -> F) -> (Vec<CapturedSpan>, T)
where
    F: Future<Output = T>,
{
    let spans = Arc::new(Mutex::new(BTreeMap::<u64, CapturedSpan>::new()));
    let subscriber = CaptureSubscriber {
        spans: Arc::clone(&spans),
        next_id: Arc::new(AtomicU64::new(1)),
    };
    let _guard = tracing::subscriber::set_default(subscriber);
    let result = f().await;
    let captured = spans
        .lock()
        .expect("capture lock poisoned")
        .values()
        .cloned()
        .collect::<Vec<_>>();

    (captured, result)
}

#[allow(dead_code)]
pub fn sample_client_for(base_url: Url) -> PolymarketRestClient {
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

#[allow(dead_code)]
pub fn sample_client_with_instrumentation(
    recorder: RuntimeMetricsRecorder,
) -> (PolymarketRestClient, ScriptedServer) {
    sample_metadata_client(
        vec![
            ScriptedResponse {
                expected_query_fragments: &["limit=2", "offset=0"],
                status_line: "200 OK",
                body: SUCCESS_METADATA_PAGE_ONE,
            },
            ScriptedResponse {
                expected_query_fragments: &["limit=2", "offset=2"],
                status_line: "200 OK",
                body: SUCCESS_METADATA_PAGE_TWO,
            },
        ],
        recorder,
    )
}

#[allow(dead_code)]
pub fn sample_failing_client_with_instrumentation(
    recorder: RuntimeMetricsRecorder,
) -> (PolymarketRestClient, ScriptedServer) {
    sample_metadata_client(
        vec![ScriptedResponse {
            expected_query_fragments: &["limit=2", "offset=0"],
            status_line: "200 OK",
            body: FAILURE_METADATA_PAGE,
        }],
        recorder,
    )
}

#[allow(dead_code)]
pub fn sample_refresh_then_fail_client_with_instrumentation(
    recorder: RuntimeMetricsRecorder,
) -> (PolymarketRestClient, ScriptedServer) {
    sample_metadata_client(
        vec![
            ScriptedResponse {
                expected_query_fragments: &["limit=2", "offset=0"],
                status_line: "200 OK",
                body: SUCCESS_METADATA_PAGE_ONE,
            },
            ScriptedResponse {
                expected_query_fragments: &["limit=2", "offset=2"],
                status_line: "200 OK",
                body: SUCCESS_METADATA_PAGE_TWO,
            },
            ScriptedResponse {
                expected_query_fragments: &["limit=2", "offset=0"],
                status_line: "200 OK",
                body: FAILURE_METADATA_PAGE,
            },
        ],
        recorder,
    )
}

#[allow(dead_code)]
pub fn sample_builder_relayer_auth() -> RelayerAuth<'static> {
    RelayerAuth::BuilderApiKey {
        api_key: "builder-key-1",
        timestamp: "1700000000",
        passphrase: "builder-pass-1",
        signature: "0xbuilder",
    }
}

#[allow(dead_code)]
pub fn sample_relayer_api_auth() -> RelayerAuth<'static> {
    RelayerAuth::RelayerApiKey {
        api_key: "relayer-key-1",
        address: "0x6666666666666666666666666666666666666666",
    }
}

#[allow(dead_code)]
pub fn sample_auth() -> L2AuthHeaders<'static> {
    sample_auth_with_funder("0xfunder")
}

#[allow(dead_code)]
pub fn sample_auth_with_funder(funder_address: &'static str) -> L2AuthHeaders<'static> {
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

#[allow(dead_code)]
pub fn sample_proxy_auth() -> L2AuthHeaders<'static> {
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

#[allow(dead_code)]
pub fn sample_safe_auth() -> L2AuthHeaders<'static> {
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

#[allow(dead_code)]
pub struct MockServer {
    base_url: Url,
    #[allow(dead_code)]
    request_rx: mpsc::Receiver<String>,
    #[allow(dead_code)]
    handle: Option<thread::JoinHandle<()>>,
}

#[allow(dead_code)]
impl MockServer {
    pub fn spawn(status_line: &str, body: &str) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
        let address = listener.local_addr().expect("server addr");
        let (request_tx, request_rx) = mpsc::channel();
        let status_line = status_line.to_owned();
        let body = body.to_owned();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept request");
            let mut buffer = Vec::new();
            let mut chunk = [0_u8; 1024];
            let mut header_end = None;
            let mut content_length = 0_usize;

            loop {
                let read = stream.read(&mut chunk).expect("read request");
                if read == 0 {
                    break;
                }

                buffer.extend_from_slice(&chunk[..read]);
                if header_end.is_none() {
                    header_end = buffer
                        .windows(4)
                        .position(|window| window == b"\r\n\r\n")
                        .map(|index| index + 4);
                    if let Some(end) = header_end {
                        let headers = String::from_utf8_lossy(&buffer[..end]);
                        content_length = headers
                            .lines()
                            .find_map(|line| {
                                let (name, value) = line.split_once(':')?;
                                if name.eq_ignore_ascii_case("content-length") {
                                    value.trim().parse::<usize>().ok()
                                } else {
                                    None
                                }
                            })
                            .unwrap_or(0);
                    }
                }

                if let Some(end) = header_end {
                    let expected_len = end + content_length;
                    if buffer.len() >= expected_len {
                        break;
                    }
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

    pub fn base_url(&self) -> Url {
        self.base_url.clone()
    }

    #[allow(dead_code)]
    pub fn finish(mut self) -> String {
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

const SUCCESS_METADATA_PAGE_ONE: &str = r#"
[
  {
    "id": "event-1",
    "title": "Championship Winner",
    "parentEvent": "family-1",
    "negRisk": true,
    "markets": [
      {
        "conditionId": "condition-1",
        "clobTokenIds": "[\"token-1\",\"token-no-1\"]",
        "groupItemTitle": "Alice",
        "negRisk": true
      }
    ]
  },
  {
    "id": "event-2",
    "title": "Championship Winner",
    "parentEvent": "family-1",
    "negRisk": true,
    "markets": [
      {
        "conditionId": "condition-2",
        "clobTokenIds": "[\"token-2\",\"token-no-2\"]",
        "groupItemTitle": "Bob",
        "negRisk": true
      }
    ]
  }
]
"#;

const SUCCESS_METADATA_PAGE_TWO: &str = "[]";

const FAILURE_METADATA_PAGE: &str = r#"
[
  {
    "title": "Broken Event",
    "parentEvent": "family-bad",
    "negRisk": true,
    "markets": [
      {
        "conditionId": "condition-bad",
        "clobTokenIds": "[\"token-bad\",\"token-no-bad\"]",
        "groupItemTitle": "Broken",
        "negRisk": true
      }
    ]
  }
]
"#;

#[allow(dead_code)]
fn sample_metadata_client(
    scripted_responses: Vec<ScriptedResponse>,
    recorder: RuntimeMetricsRecorder,
) -> (PolymarketRestClient, ScriptedServer) {
    let server = ScriptedServer::spawn(scripted_responses);
    let base_url = server.base_url();
    let client = reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("test client");

    let client = PolymarketRestClient::with_http_client(
        client,
        base_url.clone(),
        base_url.clone(),
        base_url,
        Some(VenueProducerInstrumentation::enabled(recorder)),
    );

    (client, server)
}

fn read_request(
    stream: &mut std::net::TcpStream,
    deadline: Instant,
    request_index: usize,
    expected_requests: usize,
) -> String {
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 1024];

    loop {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(read) => {
                buffer.extend_from_slice(&chunk[..read]);
                if buffer.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            Err(err)
                if matches!(
                    err.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                if Instant::now() >= deadline {
                    panic!(
                        "timed out reading scripted request {} of {}",
                        request_index + 1,
                        expected_requests
                    );
                }

                thread::sleep(Duration::from_millis(10));
            }
            Err(err) => panic!("read request: {err}"),
        }
    }

    String::from_utf8_lossy(&buffer).into_owned()
}

#[derive(Debug)]
struct ScriptedResponse {
    expected_query_fragments: &'static [&'static str],
    status_line: &'static str,
    body: &'static str,
}

#[derive(Debug)]
pub struct ScriptedServer {
    base_url: Url,
    handle: Option<thread::JoinHandle<()>>,
}

impl ScriptedServer {
    fn spawn(scripted_responses: Vec<ScriptedResponse>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
        listener
            .set_nonblocking(true)
            .expect("configure test server");
        let address = listener.local_addr().expect("server addr");
        let expected_requests = scripted_responses.len();
        let handle = thread::spawn(move || {
            for (request_index, response) in scripted_responses.into_iter().enumerate() {
                let deadline = Instant::now() + Duration::from_secs(5);
                let mut stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            stream
                                .set_read_timeout(Some(Duration::from_millis(100)))
                                .expect("configure test stream");
                            break stream;
                        }
                        Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                            if Instant::now() >= deadline {
                                panic!(
                                    "timed out waiting for scripted request {} of {}",
                                    request_index + 1,
                                    expected_requests
                                );
                            }

                            thread::sleep(Duration::from_millis(10));
                        }
                        Err(err) => panic!("accept request: {err}"),
                    }
                };
                let request = read_request(&mut stream, deadline, request_index, expected_requests);

                assert!(
                    request.starts_with("GET /events?"),
                    "unexpected request line: {request}"
                );
                for fragment in response.expected_query_fragments {
                    assert!(
                        request.contains(fragment),
                        "request missing fragment `{fragment}`: {request}"
                    );
                }

                let wire_response = format!(
                    "HTTP/1.1 {}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    response.status_line,
                    response.body.len(),
                    response.body
                );
                stream
                    .write_all(wire_response.as_bytes())
                    .expect("write response");
                stream.flush().expect("flush response");
            }
        });

        Self {
            base_url: Url::parse(&format!("http://{address}/")).expect("base url"),
            handle: Some(handle),
        }
    }

    pub fn base_url(&self) -> Url {
        self.base_url.clone()
    }
}

impl Drop for ScriptedServer {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            if thread::panicking() {
                return;
            }

            handle.join().expect("join scripted server thread");
        }
    }
}

#[allow(dead_code)]
#[derive(Clone)]
struct CaptureSubscriber {
    spans: Arc<Mutex<BTreeMap<u64, CapturedSpan>>>,
    next_id: Arc<AtomicU64>,
}

#[allow(dead_code)]
impl Subscriber for CaptureSubscriber {
    fn enabled(&self, _metadata: &Metadata<'_>) -> bool {
        true
    }

    fn register_callsite(
        &self,
        _metadata: &'static Metadata<'static>,
    ) -> tracing::subscriber::Interest {
        tracing::subscriber::Interest::always()
    }

    fn new_span(&self, attrs: &Attributes<'_>) -> Id {
        let raw_id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let id = Id::from_u64(raw_id);
        let mut fields = BTreeMap::new();
        let mut visitor = FieldVisitor {
            fields: &mut fields,
        };
        attrs.record(&mut visitor);

        self.spans.lock().expect("capture lock poisoned").insert(
            raw_id,
            CapturedSpan {
                name: attrs.metadata().name().to_owned(),
                fields,
            },
        );

        id
    }

    fn record(&self, span: &Id, values: &Record<'_>) {
        let span_id = span.clone().into_u64();
        let mut spans = self.spans.lock().expect("capture lock poisoned");
        if let Some(captured) = spans.get_mut(&span_id) {
            let mut visitor = FieldVisitor {
                fields: &mut captured.fields,
            };
            values.record(&mut visitor);
        }
    }

    fn record_follows_from(&self, _span: &Id, _follows: &Id) {}

    fn event(&self, _event: &Event<'_>) {}

    fn enter(&self, _span: &Id) {}

    fn exit(&self, _span: &Id) {}

    fn clone_span(&self, id: &Id) -> Id {
        id.clone()
    }

    fn try_close(&self, _id: Id) -> bool {
        true
    }
}

#[allow(dead_code)]
struct FieldVisitor<'a> {
    fields: &'a mut BTreeMap<String, String>,
}

#[allow(dead_code)]
impl Visit for FieldVisitor<'_> {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.fields
            .insert(field.name().to_owned(), format!("{value:?}"));
    }
}
