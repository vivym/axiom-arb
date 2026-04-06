mod support;

use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    path::PathBuf,
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc, Arc,
    },
    thread,
    time::Duration,
};

use support::{apply_db, cli, discover_db};
use tokio_tungstenite::tungstenite::{accept as accept_websocket, Message as WsMessage};
use toml_edit::{value, DocumentMut};

static NEXT_TEMP_CONFIG_ID: AtomicU64 = AtomicU64::new(1);

#[test]
fn bootstrap_empty_db_runs_discover_then_waits_for_explicit_adoption_confirmation() {
    let database = discover_db::TestDatabase::new();
    let venue = MockDiscoverVenue::spawn();
    let config_path = temp_smoke_config_path(|config| with_mock_discover_venue(config, &venue));

    let output = app_live_command()
        .arg("bootstrap")
        .arg("--config")
        .arg(&config_path)
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("app-live bootstrap should execute");

    let text = cli::combined(&output);
    assert!(output.status.success(), "{text}");
    assert!(
        text.contains("Smoke bootstrap needs discovery artifacts; starting discovery"),
        "{text}"
    );
    assert!(text.contains("Starting discovery"), "{text}");
    assert!(text.contains("Fetching Polymarket metadata"), "{text}");
    assert!(text.contains("Materializing strategy artifacts"), "{text}");
    assert!(text.contains("Discovery completed"), "{text}");
    assert!(text.contains("Adoptable revisions:"), "{text}");
    assert!(text.contains("Recommended:"), "{text}");
    assert!(
        text.contains("Waiting for explicit adoption confirmation"),
        "{text}"
    );
    assert!(!text.contains("targets candidates"), "{text}");
    assert!(!text.contains("targets adopt"), "{text}");

    assert!(database.has_strategy_candidate_rows());
    assert!(database.has_strategy_adoptable_rows());
    assert!(!database.has_strategy_provenance_rows());

    database.cleanup();
    let _ = fs::remove_file(config_path);
}

#[test]
fn bootstrap_advisory_only_discovery_stops_at_discovery_ready_not_adoptable() {
    let database = discover_db::TestDatabase::new();
    database.seed_advisory_candidate("candidate-9", "market metadata incomplete");
    let config_path = temp_smoke_config_path(with_unreachable_discover_venue);

    let output = app_live_command()
        .arg("bootstrap")
        .arg("--config")
        .arg(&config_path)
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("app-live bootstrap should execute");

    let text = cli::combined(&output);
    assert!(output.status.success(), "{text}");
    assert!(
        text.contains("Using persisted discovery artifacts"),
        "{text}"
    );
    assert!(
        text.contains("No adoptable revisions were produced"),
        "{text}"
    );
    assert!(
        text.contains("Reasons: market metadata incomplete"),
        "{text}"
    );
    assert!(text.contains("Next: rerun app-live discover"), "{text}");
    assert!(
        !text.contains("Waiting for explicit adoption confirmation"),
        "{text}"
    );

    database.cleanup();
    let _ = fs::remove_file(config_path);
}

#[test]
fn bootstrap_multiple_adoptables_show_recommendation_but_require_manual_choice() {
    let database = discover_db::TestDatabase::new();
    database.seed_adoptable_revision_without_provenance(
        "adoptable-8",
        "candidate-8",
        "targets-rev-8",
    );
    database.seed_adoptable_revision_without_provenance(
        "adoptable-9",
        "candidate-9",
        "targets-rev-9",
    );
    let config_path = temp_smoke_config_path(with_unreachable_discover_venue);

    let output = app_live_command()
        .arg("bootstrap")
        .arg("--config")
        .arg(&config_path)
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("app-live bootstrap should execute");

    let text = cli::combined(&output);
    assert!(output.status.success(), "{text}");
    assert!(
        text.contains("Using persisted discovery artifacts"),
        "{text}"
    );
    assert!(!text.contains("Discovery completed"), "{text}");
    assert!(text.contains("Adoptable revisions:"), "{text}");
    assert!(text.contains("adoptable-9"), "{text}");
    assert!(text.contains("adoptable-8"), "{text}");
    assert!(text.contains("Recommended: adoptable-9"), "{text}");
    assert!(
        text.contains("Waiting for explicit adoption confirmation"),
        "{text}"
    );

    database.cleanup();
    let _ = fs::remove_file(config_path);
}

#[test]
fn bootstrap_can_adopt_selected_revision_then_reach_smoke_rollout_boundary() {
    let database = apply_db::TestDatabase::new();
    database.seed_adoptable_revision("adoptable-9", "candidate-9", "targets-rev-9");
    let venue = MockDoctorVenue::success();
    let config_path = temp_smoke_config_path(|config| with_mock_doctor_venue(config, &venue));

    let output = run_bootstrap_with_stdin(
        &config_path,
        database.database_url(),
        "adoptable-9\npreflight-only\n",
    );

    let text = cli::combined(&output);
    assert!(output.status.success(), "{text}");
    assert!(
        text.contains("Using persisted discovery artifacts"),
        "{text}"
    );
    assert!(text.contains("Config: PASS"), "{text}");
    assert!(text.contains("Connectivity: PASS"), "{text}");
    assert!(text.contains("Overall: PASS"), "{text}");
    assert!(
        text.contains("Smoke rollout readiness is currently preflight-only."),
        "{text}"
    );
    assert!(
        text.contains("Smoke bootstrap reached preflight-ready smoke startup"),
        "{text}"
    );
    assert!(
        !text.contains("Waiting for explicit adoption confirmation"),
        "{text}"
    );

    let rewritten = fs::read_to_string(&config_path).expect("rewritten config should load");
    assert!(
        rewritten.contains("[strategy_control]"),
        "{rewritten}"
    );
    assert!(
        rewritten.contains("operator_strategy_revision = \"targets-rev-9\""),
        "{rewritten}"
    );
    assert!(
        !rewritten.contains("operator_target_revision ="),
        "{rewritten}"
    );

    let latest = database.latest_history().expect("history row should exist");
    assert_eq!(latest.action_kind, "adopt");
    assert_eq!(latest.operator_strategy_revision, "targets-rev-9");
    assert_eq!(
        latest.adoptable_strategy_revision.as_deref(),
        Some("adoptable-9")
    );
    assert_eq!(
        latest.strategy_candidate_revision.as_deref(),
        Some("candidate-9")
    );

    database.cleanup();
    let _ = fs::remove_file(config_path);
}

#[test]
fn bootstrap_already_adopted_smoke_path_falls_through_to_doctor_and_rollout() {
    let database = apply_db::TestDatabase::new();
    database.seed_adopted_target_with_active_revision("targets-rev-9", None);
    let venue = MockDoctorVenue::success();
    let config_path = temp_config_fixture_path("app-live-ux-smoke.toml", |config| {
        with_mock_doctor_venue(config, &venue)
    });

    let output =
        run_bootstrap_with_stdin(&config_path, database.database_url(), "preflight-only\n");

    let text = cli::combined(&output);
    assert!(output.status.success(), "{text}");
    assert!(!text.contains("Adoptable revisions:"), "{text}");
    assert!(!text.contains("Recommended:"), "{text}");
    assert!(text.contains("Config: PASS"), "{text}");
    assert!(text.contains("Connectivity: PASS"), "{text}");
    assert!(text.contains("Overall: PASS"), "{text}");
    assert!(
        text.contains("Smoke rollout readiness is currently preflight-only."),
        "{text}"
    );
    assert!(
        text.contains("Smoke bootstrap reached preflight-ready smoke startup"),
        "{text}"
    );
    assert_eq!(database.history_count(), 0);

    database.cleanup();
    let _ = fs::remove_file(config_path);
}

fn temp_smoke_config_path(edit: impl FnOnce(String) -> String) -> PathBuf {
    let source = cli::config_fixture("app-live-ux-smoke.toml");
    let text = fs::read_to_string(&source).expect("fixture should be readable");
    let edited = edit(text.replace("operator_target_revision = \"targets-rev-9\"\n", ""));
    let mut path = std::env::temp_dir();
    path.push(format!(
        "app-live-bootstrap-{}-{}.toml",
        std::process::id(),
        NEXT_TEMP_CONFIG_ID.fetch_add(1, Ordering::Relaxed)
    ));
    fs::write(&path, edited).expect("temp fixture should be writable");
    path
}

fn temp_config_fixture_path(relative: &str, edit: impl FnOnce(String) -> String) -> PathBuf {
    let source = cli::config_fixture(relative);
    let text = fs::read_to_string(&source).expect("fixture should be readable");
    let edited = edit(text);
    let mut path = std::env::temp_dir();
    path.push(format!(
        "app-live-bootstrap-{}-{}.toml",
        std::process::id(),
        NEXT_TEMP_CONFIG_ID.fetch_add(1, Ordering::Relaxed)
    ));
    fs::write(&path, edited).expect("temp fixture should be writable");
    path
}

fn app_live_command() -> Command {
    let mut command = Command::new(cli::app_live_binary());
    for key in [
        "all_proxy",
        "ALL_PROXY",
        "http_proxy",
        "HTTP_PROXY",
        "https_proxy",
        "HTTPS_PROXY",
    ] {
        command.env_remove(key);
    }
    command
        .env("no_proxy", "127.0.0.1,localhost")
        .env("NO_PROXY", "127.0.0.1,localhost");
    command
}

fn run_bootstrap_with_stdin(
    config_path: &std::path::Path,
    database_url: &str,
    stdin_input: &str,
) -> std::process::Output {
    let mut command = app_live_command();
    command
        .arg("bootstrap")
        .arg("--config")
        .arg(config_path)
        .env("DATABASE_URL", database_url)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = command.spawn().expect("app-live bootstrap should spawn");
    child
        .stdin
        .as_mut()
        .expect("stdin should be piped")
        .write_all(stdin_input.as_bytes())
        .expect("stdin should write");

    child
        .wait_with_output()
        .expect("app-live bootstrap should finish")
}

fn with_mock_discover_venue(config: String, venue: &MockDiscoverVenue) -> String {
    let mut document = config
        .parse::<DocumentMut>()
        .expect("smoke config fixture should parse as TOML");

    let polymarket = document
        .get_mut("polymarket")
        .and_then(|item| item.as_table_like_mut())
        .expect("smoke config fixture should contain [polymarket]");
    let source = polymarket
        .get_mut("source_overrides")
        .and_then(|item| item.as_table_like_mut())
        .expect("config fixture should contain [polymarket.source_overrides]");

    for key in ["clob_host", "data_api_host", "relayer_host"] {
        source.insert(key, value(venue.base_url()));
    }

    document.to_string()
}

fn with_unreachable_discover_venue(config: String) -> String {
    let mut document = config
        .parse::<DocumentMut>()
        .expect("smoke config fixture should parse as TOML");

    let polymarket = document
        .get_mut("polymarket")
        .and_then(|item| item.as_table_like_mut())
        .expect("smoke config fixture should contain [polymarket]");
    let source = polymarket
        .get_mut("source_overrides")
        .and_then(|item| item.as_table_like_mut())
        .expect("config fixture should contain [polymarket.source_overrides]");

    for key in ["clob_host", "data_api_host", "relayer_host"] {
        source.insert(key, value("http://127.0.0.1:1/"));
    }

    document.to_string()
}

fn with_mock_doctor_venue(config: String, venue: &MockDoctorVenue) -> String {
    let mut document = config
        .parse::<DocumentMut>()
        .expect("smoke config fixture should parse as TOML");

    let polymarket = document
        .get_mut("polymarket")
        .and_then(|item| item.as_table_like_mut())
        .expect("smoke config fixture should contain [polymarket]");
    let source = polymarket
        .get_mut("source_overrides")
        .and_then(|item| item.as_table_like_mut())
        .expect("smoke config fixture should contain [polymarket.source_overrides]");

    for (key, rewritten) in [
        ("clob_host", venue.http_base_url()),
        ("data_api_host", venue.http_base_url()),
        ("relayer_host", venue.http_base_url()),
        ("market_ws_url", venue.market_ws_url()),
        ("user_ws_url", venue.user_ws_url()),
    ] {
        source.insert(key, value(rewritten));
    }

    document.to_string()
}

struct MockDoctorVenue {
    http: ProbeHttpServer,
    market_ws: ProbeWsServer,
    user_ws: ProbeWsServer,
}

impl MockDoctorVenue {
    fn success() -> Self {
        Self {
            http: ProbeHttpServer::spawn(ProbeHttpBehavior::success()),
            market_ws: ProbeWsServer::spawn(WsProbeKind::Market),
            user_ws: ProbeWsServer::spawn(WsProbeKind::User),
        }
    }

    fn http_base_url(&self) -> &str {
        self.http.base_url()
    }

    fn market_ws_url(&self) -> &str {
        self.market_ws.url()
    }

    fn user_ws_url(&self) -> &str {
        self.user_ws.url()
    }
}

struct MockDiscoverVenue {
    http: ScriptedServer,
}

impl MockDiscoverVenue {
    fn spawn() -> Self {
        Self {
            http: spawn_local_listener(vec![page_one_ok(), page_two_empty()]),
        }
    }

    fn base_url(&self) -> &str {
        self.http.base_url()
    }
}

struct ScriptedServer {
    base_url: String,
    shutdown: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

struct ProbeHttpServer {
    base_url: String,
    shutdown_tx: Option<mpsc::Sender<()>>,
    handle: Option<thread::JoinHandle<()>>,
}

impl ProbeHttpServer {
    fn spawn(behavior: ProbeHttpBehavior) -> Self {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind http probe server");
        let address = listener.local_addr().expect("http probe server address");
        listener
            .set_nonblocking(true)
            .expect("http probe server should be nonblocking");
        let (shutdown_tx, shutdown_rx) = mpsc::channel();
        let handle = thread::spawn(move || loop {
            if shutdown_rx.try_recv().is_ok() {
                break;
            }

            match listener.accept() {
                Ok((stream, _)) => {
                    stream
                        .set_nonblocking(false)
                        .expect("accepted http stream should be blocking");
                    handle_http_probe_connection(stream, &behavior)
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("http probe server accept failed: {error}"),
            }
        });

        Self {
            base_url: format!("http://{address}"),
            shutdown_tx: Some(shutdown_tx),
            handle: Some(handle),
        }
    }

    fn base_url(&self) -> &str {
        &self.base_url
    }
}

impl Drop for ProbeHttpServer {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        if let Some(handle) = self.handle.take() {
            handle.join().expect("join http probe server");
        }
    }
}

#[derive(Clone)]
struct ProbeHttpBehavior {
    orders_status_line: String,
    orders_body: String,
    heartbeat_status_line: String,
    heartbeat_body: String,
    transactions_status_line: String,
    transactions_body: String,
}

impl ProbeHttpBehavior {
    fn success() -> Self {
        Self {
            orders_status_line: "200 OK".to_owned(),
            orders_body: "[]".to_owned(),
            heartbeat_status_line: "200 OK".to_owned(),
            heartbeat_body: r#"{"success":true,"heartbeat_id":"hb-1"}"#.to_owned(),
            transactions_status_line: "200 OK".to_owned(),
            transactions_body: "[]".to_owned(),
        }
    }
}

fn handle_http_probe_connection(mut stream: std::net::TcpStream, behavior: &ProbeHttpBehavior) {
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 1024];
    let mut header_end = None;
    let mut content_length = 0_usize;

    loop {
        let read = stream.read(&mut chunk).expect("read probe request");
        if read == 0 {
            break;
        }

        buffer.extend_from_slice(&chunk[..read]);
        if header_end.is_none() {
            header_end = find_header_end(&buffer);
            if let Some(index) = header_end {
                let headers = String::from_utf8_lossy(&buffer[..index]);
                content_length = content_length_from_headers(&headers);
            }
        }

        if let Some(index) = header_end {
            let body_bytes = buffer.len().saturating_sub(index + 4);
            if body_bytes >= content_length {
                break;
            }
        }
    }

    let request = String::from_utf8_lossy(&buffer);
    let request_line = request.lines().next().unwrap_or_default();
    let (status_line, body) = http_probe_response(request_line, behavior);
    let response = format!(
        "HTTP/1.1 {status_line}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    );

    stream
        .write_all(response.as_bytes())
        .expect("write http probe response");
    stream.flush().expect("flush http probe response");
}

fn find_header_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == b"\r\n\r\n")
}

fn content_length_from_headers(headers: &str) -> usize {
    headers
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            if name.trim().eq_ignore_ascii_case("content-length") {
                value.trim().parse::<usize>().ok()
            } else {
                None
            }
        })
        .unwrap_or(0)
}

fn http_probe_response<'a>(
    request_line: &str,
    behavior: &'a ProbeHttpBehavior,
) -> (&'a str, &'a str) {
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let target = parts.next().unwrap_or_default();
    let path = target.split('?').next().unwrap_or_default();

    match (method, path) {
        ("GET", "/orders") => (&behavior.orders_status_line, &behavior.orders_body),
        ("POST", "/heartbeat") => (&behavior.heartbeat_status_line, &behavior.heartbeat_body),
        ("GET", "/transactions") => (
            &behavior.transactions_status_line,
            &behavior.transactions_body,
        ),
        _ => ("404 Not Found", r#"{"error":"not found"}"#),
    }
}

struct ProbeWsServer {
    url: String,
    shutdown_tx: Option<mpsc::Sender<()>>,
    handle: Option<thread::JoinHandle<()>>,
}

impl ProbeWsServer {
    fn spawn(kind: WsProbeKind) -> Self {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind ws probe server");
        let address = listener.local_addr().expect("ws probe server address");
        listener
            .set_nonblocking(true)
            .expect("ws probe server should be nonblocking");
        let (shutdown_tx, shutdown_rx) = mpsc::channel();
        let handle = thread::spawn(move || loop {
            if shutdown_rx.try_recv().is_ok() {
                break;
            }

            match listener.accept() {
                Ok((stream, _)) => {
                    stream
                        .set_nonblocking(false)
                        .expect("accepted ws stream should be blocking");
                    let mut websocket =
                        accept_websocket(stream).expect("accept websocket connection");
                    let mut responded = false;
                    loop {
                        match websocket.read() {
                            Ok(WsMessage::Text(_)) if !responded => {
                                websocket
                                    .send(WsMessage::Text(kind.response_payload().into()))
                                    .expect("send ws probe response");
                                responded = true;
                            }
                            Ok(_) => {}
                            Err(_) => break,
                        }
                    }
                    break;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("ws probe server accept failed: {error}"),
            }
        });

        Self {
            url: format!("ws://{address}"),
            shutdown_tx: Some(shutdown_tx),
            handle: Some(handle),
        }
    }

    fn url(&self) -> &str {
        &self.url
    }
}

impl Drop for ProbeWsServer {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        if let Some(handle) = self.handle.take() {
            handle.join().expect("join ws probe server");
        }
    }
}

#[derive(Clone, Copy)]
enum WsProbeKind {
    Market,
    User,
}

impl WsProbeKind {
    fn response_payload(self) -> &'static str {
        match self {
            Self::Market => {
                r#"{"event":"book","asset_id":"token-1","best_bid":"0.40","best_ask":"0.41"}"#
            }
            Self::User => {
                r#"{"event":"trade","trade_id":"trade-1","order_id":"order-1","status":"MATCHED","condition_id":"condition-1","price":"0.41","size":"100","fee_rate_bps":"15","transaction_hash":"0xtrade"}"#
            }
        }
    }
}

impl ScriptedServer {
    fn base_url(&self) -> &str {
        &self.base_url
    }
}

impl Drop for ScriptedServer {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            handle.join().expect("join server thread");
        }
    }
}

struct ScriptedResponse {
    expected_query_fragments: &'static [&'static str],
    body: &'static str,
}

fn spawn_local_listener(scripted_responses: Vec<ScriptedResponse>) -> ScriptedServer {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
    listener
        .set_nonblocking(true)
        .expect("listener should become nonblocking");
    let address = listener.local_addr().expect("server addr");
    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_for_thread = Arc::clone(&shutdown);
    let handle = thread::spawn(move || {
        for response in scripted_responses {
            let (mut stream, _) = loop {
                if shutdown_for_thread.load(Ordering::Relaxed) {
                    return;
                }

                match listener.accept() {
                    Ok(accepted) => break accepted,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) => panic!("accept request: {error}"),
                }
            };
            stream
                .set_nonblocking(false)
                .expect("accepted stream should become blocking");
            let request = read_request(&mut stream);
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
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                response.body.len(),
                response.body
            );
            stream
                .write_all(wire_response.as_bytes())
                .expect("write response");
            stream.flush().expect("flush response");
        }
    });

    ScriptedServer {
        base_url: format!("http://{address}/"),
        shutdown,
        handle: Some(handle),
    }
}

fn read_request(stream: &mut std::net::TcpStream) -> String {
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 1024];

    loop {
        let read = match stream.read(&mut chunk) {
            Ok(read) => read,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10));
                continue;
            }
            Err(error) => panic!("read request: {error}"),
        };
        if read == 0 {
            break;
        }

        buffer.extend_from_slice(&chunk[..read]);
        if buffer.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
    }

    String::from_utf8_lossy(&buffer).into_owned()
}

fn page_one_ok() -> ScriptedResponse {
    ScriptedResponse {
        expected_query_fragments: &["active=true", "closed=false", "limit=2", "offset=0"],
        body: r#"[{"id":"event-1","parentEvent":"family-a","negRisk":true,"enableNegRisk":true,"negRiskAugmented":false,"markets":[{"conditionId":"condition-1","clobTokenIds":"token-1","outcomes":"Alpha","shortOutcomes":"Alpha","negRisk":true,"negRiskOther":false}]},{"id":"event-2","parentEvent":"family-a","negRisk":true,"enableNegRisk":true,"negRiskAugmented":false,"markets":[{"conditionId":"condition-2","clobTokenIds":"token-2","outcomes":"Beta","shortOutcomes":"Beta","negRisk":true,"negRiskOther":false}]}]"#,
    }
}

fn page_two_empty() -> ScriptedResponse {
    ScriptedResponse {
        expected_query_fragments: &["active=true", "closed=false", "limit=2", "offset=2"],
        body: r#"[]"#,
    }
}
