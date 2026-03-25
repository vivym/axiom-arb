use std::{
    collections::BTreeMap,
    io::{Read, Write},
    net::TcpListener,
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
    time::Duration,
};

use domain::{SignatureType, WalletRoute};
use reqwest::Url;
use tracing::{
    field::{Field, Visit},
    span::{Attributes, Id, Record},
    Event, Metadata, Subscriber,
};
use venue_polymarket::{L2AuthHeaders, PolymarketRestClient, RelayerAuth, SignerContext};

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
pub fn sample_client_for(base_url: Url) -> PolymarketRestClient {
    let client = reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("test client");

    PolymarketRestClient::with_http_client(client, base_url.clone(), base_url.clone(), base_url)
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
