mod support;

use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    path::PathBuf,
    process::Command,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    thread,
    time::Duration,
};

use support::{cli, discover_db};
use toml_edit::{value, DocumentMut};

static NEXT_TEMP_CONFIG_ID: AtomicU64 = AtomicU64::new(1);

#[test]
fn discover_subcommand_is_exposed() {
    let output = Command::new(cli::app_live_binary())
        .arg("discover")
        .arg("--help")
        .output()
        .expect("app-live discover --help should execute");

    let text = cli::combined(&output);
    assert!(output.status.success(), "{text}");
    assert!(text.contains("--config"), "{text}");
}

#[test]
fn discover_materializes_candidate_and_adoptable_artifacts_from_smoke_config() {
    let database = discover_db::TestDatabase::new();
    let venue = MockDiscoverVenue::spawn();
    let config_path = temp_config_fixture_path("app-live-ux-smoke.toml", |config| {
        let config = config.replace("operator_target_revision = \"targets-rev-9\"\n", "");
        with_mock_discover_venue(config, &venue)
    });

    let output = app_live_command()
        .arg("discover")
        .arg("--config")
        .arg(&config_path)
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("app-live discover should execute");

    let text = cli::combined(&output);
    assert!(output.status.success(), "{text}");
    assert!(text.contains("candidate_count = 1"), "{text}");
    assert!(text.contains("adoptable_count = 1"), "{text}");
    assert!(text.contains("recommended_adoptable_revision = "), "{text}");

    assert!(database.has_candidate_rows());
    assert!(database.has_adoptable_rows());
    assert!(!database.has_candidate_provenance_rows());

    database.cleanup();
    let _ = fs::remove_file(config_path);
}

#[test]
fn discover_materializes_candidate_and_adoptable_artifacts_from_live_adopted_config() {
    let database = discover_db::TestDatabase::new();
    let venue = MockDiscoverVenue::spawn();
    let config_path = temp_config_fixture_path("app-live-ux-live.toml", |config| {
        let config = config.replace("operator_target_revision = \"targets-rev-9\"\n", "");
        let config = format!(
            "{config}\n[polymarket.source_overrides]\nclob_host = \"https://clob.polymarket.com\"\ndata_api_host = \"https://data-api.polymarket.com\"\nrelayer_host = \"https://relayer-v2.polymarket.com\"\nmarket_ws_url = \"wss://ws-subscriptions-clob.polymarket.com/ws/market\"\nuser_ws_url = \"wss://ws-subscriptions-clob.polymarket.com/ws/user\"\nheartbeat_interval_seconds = 15\nrelayer_poll_interval_seconds = 5\nmetadata_refresh_interval_seconds = 60\n"
        );
        with_mock_discover_venue(config, &venue)
    });

    let output = app_live_command()
        .arg("discover")
        .arg("--config")
        .arg(&config_path)
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("app-live discover should execute");

    let text = cli::combined(&output);
    assert!(output.status.success(), "{text}");
    assert!(text.contains("candidate_count = 1"), "{text}");
    assert!(text.contains("adoptable_count = 1"), "{text}");
    assert!(text.contains("recommended_adoptable_revision = "), "{text}");

    assert!(database.has_candidate_rows());
    assert!(database.has_adoptable_rows());
    assert!(!database.has_candidate_provenance_rows());

    database.cleanup();
    let _ = fs::remove_file(config_path);
}

fn temp_config_fixture_path(relative: &str, edit: impl FnOnce(String) -> String) -> PathBuf {
    let source = cli::config_fixture(relative);
    let text = fs::read_to_string(&source).expect("fixture should be readable");
    let edited = edit(text);
    let mut path = std::env::temp_dir();
    path.push(format!(
        "app-live-discover-{}-{}.toml",
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

fn with_mock_discover_venue(config: String, venue: &MockDiscoverVenue) -> String {
    let mut document = config
        .parse::<DocumentMut>()
        .expect("smoke config fixture should parse as TOML");

    let polymarket = document
        .get_mut("polymarket")
        .and_then(|item| item.as_table_like_mut())
        .expect("smoke config fixture should contain [polymarket]");
    let source = if let Some(item) = polymarket.get_mut("source_overrides") {
        item.as_table_like_mut()
    } else if let Some(item) = polymarket.get_mut("source") {
        item.as_table_like_mut()
    } else {
        None
    }
    .expect("config fixture should contain [polymarket.source] or [polymarket.source_overrides]");

    for key in ["clob_host", "data_api_host", "relayer_host"] {
        source.insert(key, value(venue.base_url()));
    }

    document.to_string()
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
