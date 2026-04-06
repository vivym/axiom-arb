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
    assert!(text.contains("Starting discovery"), "{text}");
    assert!(text.contains("Fetching Polymarket metadata"), "{text}");
    assert!(text.contains("Materializing strategy artifacts"), "{text}");
    assert!(text.contains("candidate_count = 2"), "{text}");
    assert!(text.contains("adoptable_count = 2"), "{text}");
    assert!(text.contains("recommended_adoptable_revision = "), "{text}");

    assert!(database.has_strategy_candidate_rows());
    assert!(database.has_strategy_adoptable_rows());
    assert!(!database.has_strategy_provenance_rows());

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
            "{config}\n[polymarket.source_overrides]\nclob_host = \"https://clob.polymarket.com\"\ndata_api_host = \"https://gamma-api.polymarket.com\"\nrelayer_host = \"https://relayer-v2.polymarket.com\"\nmarket_ws_url = \"wss://ws-subscriptions-clob.polymarket.com/ws/market\"\nuser_ws_url = \"wss://ws-subscriptions-clob.polymarket.com/ws/user\"\nheartbeat_interval_seconds = 15\nrelayer_poll_interval_seconds = 5\nmetadata_refresh_interval_seconds = 60\n"
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
    assert!(text.contains("Starting discovery"), "{text}");
    assert!(text.contains("Fetching Polymarket metadata"), "{text}");
    assert!(text.contains("Materializing strategy artifacts"), "{text}");
    assert!(text.contains("candidate_count = 2"), "{text}");
    assert!(text.contains("adoptable_count = 2"), "{text}");
    assert!(text.contains("recommended_adoptable_revision = "), "{text}");

    assert!(database.has_strategy_candidate_rows());
    assert!(database.has_strategy_adoptable_rows());
    assert!(!database.has_strategy_provenance_rows());

    database.cleanup();
    let _ = fs::remove_file(config_path);
}

#[test]
fn discover_noop_rediscovery_reuses_strategy_bundle_identity() {
    let database = discover_db::TestDatabase::new();
    let venue = MockDiscoverVenue::with_responses(vec![
        page_one_ok(),
        page_two_empty(),
        page_one_ok(),
        page_two_empty(),
    ]);
    let config_path = temp_config_fixture_path("app-live-ux-smoke.toml", |config| {
        let config = config.replace("operator_target_revision = \"targets-rev-9\"\n", "");
        with_mock_discover_venue(config, &venue)
    });

    let first_output = app_live_command()
        .arg("discover")
        .arg("--config")
        .arg(&config_path)
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("first discover should execute");
    let first_text = cli::combined(&first_output);
    assert!(first_output.status.success(), "{first_text}");
    let first_revision = recommended_adoptable_revision(&first_text);

    let second_output = app_live_command()
        .arg("discover")
        .arg("--config")
        .arg(&config_path)
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("second discover should execute");
    let second_text = cli::combined(&second_output);
    assert!(second_output.status.success(), "{second_text}");
    let second_revision = recommended_adoptable_revision(&second_text);

    assert_eq!(first_revision, second_revision);
    assert!(
        second_text.contains("route_diff_count = 0"),
        "{second_text}"
    );
    assert!(second_text.contains("route_diff = none"), "{second_text}");
    assert_eq!(database.strategy_candidate_row_count(), 1);
    assert_eq!(database.strategy_adoptable_row_count(), 1);

    database.cleanup();
    let _ = fs::remove_file(config_path);
}

#[test]
fn discover_reports_route_local_diffs_to_stdout_when_bundle_changes() {
    let database = discover_db::TestDatabase::new();
    let venue = MockDiscoverVenue::with_responses(vec![
        page_one_ok(),
        page_two_empty(),
        page_one_changed(),
        page_two_empty(),
    ]);
    let config_path = temp_config_fixture_path("app-live-ux-smoke.toml", |config| {
        let config = config.replace("operator_target_revision = \"targets-rev-9\"\n", "");
        with_mock_discover_venue(config, &venue)
    });

    let first_output = app_live_command()
        .arg("discover")
        .arg("--config")
        .arg(&config_path)
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("first discover should execute");
    let first_text = cli::combined(&first_output);
    assert!(first_output.status.success(), "{first_text}");

    let second_output = app_live_command()
        .arg("discover")
        .arg("--config")
        .arg(&config_path)
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("second discover should execute");
    let second_text = cli::combined(&second_output);
    assert!(second_output.status.success(), "{second_text}");

    assert!(
        second_text.contains("route_diff_count = 2"),
        "{second_text}"
    );
    assert!(
        second_text.contains("route_diff = changed route=neg-risk scope=family-a"),
        "{second_text}"
    );
    assert!(
        second_text.contains("route_diff = added route=neg-risk scope=family-b"),
        "{second_text}"
    );
    assert!(
        !second_text.contains("route_diff = added route=full-set scope=default"),
        "{second_text}"
    );

    database.cleanup();
    let _ = fs::remove_file(config_path);
}

#[test]
fn discover_keeps_full_set_route_digest_stable_when_neg_risk_metadata_changes() {
    let database = discover_db::TestDatabase::new();
    let venue = MockDiscoverVenue::with_responses(vec![
        page_one_ok(),
        page_two_empty(),
        page_one_changed(),
        page_two_empty(),
    ]);
    let config_path = temp_config_fixture_path("app-live-ux-smoke.toml", |config| {
        let config = config.replace("operator_target_revision = \"targets-rev-9\"\n", "");
        with_mock_discover_venue(config, &venue)
    });

    let first_output = app_live_command()
        .arg("discover")
        .arg("--config")
        .arg(&config_path)
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("first discover should execute");
    let first_text = cli::combined(&first_output);
    assert!(first_output.status.success(), "{first_text}");

    let first_candidate = database
        .strategy_candidate_rows()
        .into_iter()
        .next()
        .expect("first strategy candidate row");
    let first_full_set_digest =
        route_digest(&first_candidate.payload, "full-set", "default").to_owned();
    let first_revision = first_candidate.strategy_candidate_revision.clone();

    let second_output = app_live_command()
        .arg("discover")
        .arg("--config")
        .arg(&config_path)
        .env("DATABASE_URL", database.database_url())
        .output()
        .expect("second discover should execute");
    let second_text = cli::combined(&second_output);
    assert!(second_output.status.success(), "{second_text}");

    let second_candidate = database
        .strategy_candidate_rows()
        .into_iter()
        .find(|row| row.strategy_candidate_revision != first_revision)
        .expect("second strategy candidate row");

    assert_eq!(database.strategy_candidate_row_count(), 2);
    assert_ne!(
        second_candidate.strategy_candidate_revision, first_revision,
        "neg-risk route change should change bundle identity"
    );
    assert_eq!(
        route_digest(&second_candidate.payload, "full-set", "default"),
        first_full_set_digest,
        "full-set route digest should be reused across rediscovery"
    );

    database.cleanup();
    let _ = fs::remove_file(config_path);
}

#[test]
fn discover_emits_debug_logs_when_rust_log_requests_them() {
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
        .env("RUST_LOG", "debug")
        .output()
        .expect("app-live discover should execute");

    let text = cli::combined(&output);
    assert!(output.status.success(), "{text}");
    assert!(text.contains("discover loaded live config"), "{text}");
    assert!(text.contains("discover fetched metadata rows"), "{text}");
    assert!(
        text.contains("discover materialized strategy bundle"),
        "{text}"
    );

    database.cleanup();
    let _ = fs::remove_file(config_path);
}

fn recommended_adoptable_revision(output: &str) -> String {
    output
        .lines()
        .find_map(|line| {
            line.strip_prefix("recommended_adoptable_revision = ")
                .map(str::to_owned)
        })
        .expect("discover output should include recommended adoptable revision")
}

fn route_digest<'a>(payload: &'a serde_json::Value, route: &str, scope: &str) -> &'a str {
    payload["route_artifacts"]
        .as_array()
        .expect("route_artifacts should be present")
        .iter()
        .find(|artifact| artifact["key"]["route"] == route && artifact["key"]["scope"] == scope)
        .and_then(|artifact| artifact["semantic_digest"].as_str())
        .expect("route artifact digest should be present")
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
        Self::with_responses(vec![page_one_ok(), page_two_empty()])
    }

    fn with_responses(scripted_responses: Vec<ScriptedResponse>) -> Self {
        Self {
            http: spawn_local_listener(scripted_responses),
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

fn page_one_changed() -> ScriptedResponse {
    ScriptedResponse {
        expected_query_fragments: &["active=true", "closed=false", "limit=2", "offset=0"],
        body: r#"[{"id":"event-1","parentEvent":"family-a","negRisk":true,"enableNegRisk":true,"negRiskAugmented":false,"markets":[{"conditionId":"condition-1","clobTokenIds":"token-1","outcomes":"Alpha","shortOutcomes":"Alpha","negRisk":true,"negRiskOther":false}]},{"id":"event-3","parentEvent":"family-b","negRisk":true,"enableNegRisk":true,"negRiskAugmented":false,"markets":[{"conditionId":"condition-3","clobTokenIds":"token-3","outcomes":"Gamma","shortOutcomes":"Gamma","negRisk":true,"negRiskOther":false}]}]"#,
    }
}
