use super::*;
use crate::models::WorkoutSeriesSummary;
use std::collections::HashMap;
use std::io::Cursor;
use std::net::TcpStream as ClientStream;

const TOKEN: &str = "zbk_0123456789abcdef";

/// Lifecycle tests bind a temporary port, not 43921: a dev machine often runs a
/// ZeppBridge, and a failure on the real port is an environment conflict, not product behavior.
/// The port is released when it can be bound again, or a connection to it is refused.
///
/// "Cannot connect" alone gives false alarms: on Linux, a connection to a just-released
/// temporary port can get the same source port and connect to itself (TCP simultaneous
/// open), so connect succeeds (seen on CI). "Can bind" alone also fails: Windows / macOS
/// refuse a rebind while an accepted connection is still open. Only when both fail does
/// something really listen there.
///
/// The OS does not release the port synchronously after shutdown (seen on macOS CI:
/// shutdown returned while the port was still in use). So allow two seconds and check
/// repeatedly. If something really keeps listening, the check still fails after two seconds.
fn port_is_released(address: &str) -> bool {
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    loop {
        if TcpListener::bind(address).is_ok() || ClientStream::connect(address).is_err() {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// One unique temporary port per test. Tests run in parallel: a port released by one
/// test can go straight to another test that starts a server on it, and then the first
/// test's "port released" assert fails. So this process never hands out a port twice.
fn ephemeral_address() -> String {
    static HANDED_OUT: Mutex<Vec<u16>> = Mutex::new(Vec::new());
    loop {
        let probe = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = probe.local_addr().unwrap().port();
        drop(probe);
        let mut handed_out = HANDED_OUT.lock().unwrap();
        if !handed_out.contains(&port) {
            handed_out.push(port);
            return format!("127.0.0.1:{port}");
        }
    }
}

#[derive(Default)]
struct MemoryCredentials {
    entries: Mutex<HashMap<String, String>>,
}

impl CredentialBackend for MemoryCredentials {
    fn set(&self, account: &str, token: &str) -> Result<(), String> {
        self.entries
            .lock()
            .unwrap()
            .insert(account.to_string(), token.to_string());
        Ok(())
    }

    fn get(&self, account: &str) -> Result<Option<String>, String> {
        Ok(self.entries.lock().unwrap().get(account).cloned())
    }

    fn delete(&self, account: &str) -> Result<(), String> {
        self.entries.lock().unwrap().remove(account);
        Ok(())
    }
}

fn request(method: &str, target: &str, authorization: Option<&str>) -> ParsedRequest {
    ParsedRequest {
        method: method.to_string(),
        target: target.to_string(),
        authorization: authorization.map(str::to_string),
    }
}

fn empty_series(id: &str) -> WorkoutSeries {
    WorkoutSeries {
        workout_id: id.to_string(),
        samples: vec![],
        route: vec![],
        pauses: vec![],
        splits: vec![],
        laps: vec![],
        summary: WorkoutSeriesSummary::default(),
        climbs: None,
        strength: None,
    }
}

fn controller(dir: &Path) -> LocalApiController {
    controller_at(dir, ephemeral_address())
}

fn controller_at(dir: &Path, address: String) -> LocalApiController {
    LocalApiController::with_bind_address(
        dir.to_path_buf(),
        address,
        Arc::new(MemoryCredentials::default()),
    )
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("zeppbridge-local-api-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn health_route_describes_running_service_without_cors() {
    let response = route_request(
        &request("GET", "/health", Some(&format!("Bearer {TOKEN}"))),
        TOKEN,
        |_| unreachable!(),
    );
    assert_eq!(response.status, 200);
    let body: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
    assert_eq!(body["status"], "ok");

    let mut output = Vec::new();
    write_response(&mut output, response).unwrap();
    let text = String::from_utf8(output).unwrap();
    assert!(!text
        .to_ascii_lowercase()
        .contains("access-control-allow-origin"));
    assert!(text.contains("Cache-Control: no-store"));
}

#[test]
fn workout_route_decodes_id_and_returns_clean_series_json() {
    let response = route_request(
        &request(
            "GET",
            "/workouts/run%2D123/series",
            Some(&format!("Bearer {TOKEN}")),
        ),
        TOKEN,
        |id| {
            assert_eq!(id, "run-123");
            Ok(Some(empty_series(id)))
        },
    );
    assert_eq!(response.status, 200);
    let body: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
    assert_eq!(body["workout_id"], "run-123");
    assert_eq!(body["samples"], json!([]));
    assert_eq!(body["route"], json!([]));
}

#[test]
fn unknown_workout_is_404_and_storage_errors_are_generic() {
    let auth = format!("Bearer {TOKEN}");
    let missing = route_request(
        &request("GET", "/workouts/404/series", Some(&auth)),
        TOKEN,
        |_| Ok(None),
    );
    assert_eq!(missing.status, 404);
    let failed = route_request(
        &request("GET", "/workouts/500/series", Some(&auth)),
        TOKEN,
        |_| Err("C:\\private\\zepp.db failed".to_string()),
    );
    assert_eq!(failed.status, 500);
    let text = String::from_utf8(failed.body).unwrap();
    assert!(!text.contains("private"));
    assert!(text.contains("local_data_unavailable"));
}

#[test]
fn rejects_other_methods_and_encoded_path_separators() {
    let auth = format!("Bearer {TOKEN}");
    let post = route_request(
        &request("POST", "/workouts/1/series", Some(&auth)),
        TOKEN,
        |_| unreachable!(),
    );
    assert_eq!(post.status, 405);
    assert!(post.allow_get);
    let invalid = route_request(
        &request("GET", "/workouts/a%2Fb/series", Some(&auth)),
        TOKEN,
        |_| unreachable!(),
    );
    assert_eq!(invalid.status, 400);
}

#[test]
fn every_route_requires_a_bearer_token_and_never_echoes_the_expected_value() {
    for target in ["/", "/health", "/workouts/1/series"] {
        for authorization in [
            None,
            Some("Bearer zbk_wrong"),
            Some("Basic zbk_0123456789abcdef"),
            Some("Bearer "),
            Some("zbk_0123456789abcdef"),
        ] {
            let response = route_request(&request("GET", target, authorization), TOKEN, |_| {
                unreachable!("an unauthorized request must not read the local database")
            });
            assert_eq!(response.status, 401, "{target} / {authorization:?}");
            let text = String::from_utf8(response.body).unwrap();
            assert!(!text.contains(TOKEN), "the 401 response leaked the expected token");
        }
    }
}

#[test]
fn unauthorized_response_carries_a_bearer_challenge() {
    let response = route_request(&request("GET", "/health", None), TOKEN, |_| unreachable!());
    let mut output = Vec::new();
    write_response(&mut output, response).unwrap();
    let text = String::from_utf8(output).unwrap();
    assert!(text.contains("WWW-Authenticate: Bearer"));
}

#[test]
fn header_parsing_is_bounded_and_rejects_conflicting_authorization() {
    let ok = parse_request(&mut Cursor::new(
        "GET /health HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer abc\r\n\r\n".as_bytes(),
    ))
    .unwrap();
    assert_eq!(ok.authorization.as_deref(), Some("Bearer abc"));

    let duplicate_same = parse_request(&mut Cursor::new(
        "GET /health HTTP/1.1\r\nAuthorization: Bearer abc\r\nAuthorization: Bearer abc\r\n\r\n"
            .as_bytes(),
    ));
    assert!(duplicate_same.is_ok());

    let conflicting = parse_request(&mut Cursor::new(
        "GET /health HTTP/1.1\r\nAuthorization: Bearer abc\r\nAuthorization: Bearer xyz\r\n\r\n"
            .as_bytes(),
    ));
    assert!(conflicting.is_err());

    let long_line = format!(
        "GET /health HTTP/1.1\r\nX-Long: {}\r\n\r\n",
        "a".repeat(MAX_HEADER_LINE_BYTES + 10)
    );
    assert!(parse_request(&mut Cursor::new(long_line.as_bytes())).is_err());

    let mut many = String::from("GET /health HTTP/1.1\r\n");
    for index in 0..(MAX_HEADER_LINES + 5) {
        many.push_str(&format!("X-{index}: v\r\n"));
    }
    many.push_str("\r\n");
    assert!(parse_request(&mut Cursor::new(many.as_bytes())).is_err());

    let long_target = format!(
        "GET /{} HTTP/1.1\r\n\r\n",
        "a".repeat(MAX_REQUEST_LINE_BYTES)
    );
    assert!(parse_request(&mut Cursor::new(long_target.as_bytes())).is_err());

    assert!(parse_request(&mut Cursor::new(
        "GET /health HTTP/1.1\r\nnot-a-header\r\n\r\n".as_bytes()
    ))
    .is_err());

    // Headers that do not end with an empty line must not route as a valid request.
    assert!(parse_request(&mut Cursor::new(
        "GET /health HTTP/1.1\r\nHost: x\r\n".as_bytes()
    ))
    .is_err());
}

#[test]
fn first_install_does_not_listen_until_the_user_enables_it() {
    let dir = temp_dir("first-install");
    // Hold this port, then let the controller restore. If it tried to bind, it would
    // get a "port in use" error. Only `error` = None proves that it did not try.
    // This is more reliable than "try to connect": another test can take a temporary port at any time.
    let squatter = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = squatter.local_addr().unwrap().to_string();
    let controller = controller_at(&dir, address);
    let status = controller.restore();
    assert!(!status.enabled);
    assert!(!status.running);
    assert!(
        status.error.is_none(),
        "with no state file, it must not even try to bind: {status:?}"
    );
    drop(squatter);
}

#[test]
fn enable_disable_cycle_binds_and_releases_the_port_without_restart() {
    let dir = temp_dir("lifecycle");
    let address = ephemeral_address();
    let controller = controller_at(&dir, address.clone());
    assert!(!controller.restore().running);

    let enabled = controller.set_enabled(true);
    assert!(enabled.enabled && enabled.running, "{enabled:?}");
    assert!(enabled.token_present);
    assert!(ClientStream::connect(address.as_str()).is_ok());

    // The saved enabled state is restored across processes.
    assert!(read_enabled_flag(&dir));

    let disabled = controller.set_enabled(false);
    assert!(!disabled.enabled && !disabled.running);
    assert!(!read_enabled_flag(&dir));
    assert!(port_is_released(&address), "the port must be released after disabling");

    // After disabling, enabling again works at once: the old listener does not hold the port.
    assert!(controller.set_enabled(true).running);
    controller.shutdown();
    assert!(port_is_released(&address));
}

/// A slow client must not block another normal client.
///
/// This is the real effect of the old single-threaded design: the read timeout is 2 s,
/// so a local process that connected but sent no byte made the next normal request
/// wait 2 s, and several of them made it wait more than ten seconds. Assert with a
/// budget clearly below the read timeout to pin the regression: if the pool falls
/// back to serial handling, this test fails.
#[test]
fn a_silent_client_does_not_block_the_next_request() {
    let dir = temp_dir("slowloris");
    let address = ephemeral_address();
    let controller = controller_at(&dir, address.clone());
    controller.set_enabled(true);
    let token = controller.reveal_token().unwrap();

    // Connect and stay silent, to hold one worker until its own read timeout.
    let _silent = ClientStream::connect(address.as_str()).unwrap();

    let started = std::time::Instant::now();
    let mut client = ClientStream::connect(address.as_str()).unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    client
        .write_all(
            format!(
                "GET /health HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {token}\r\n\r\n"
            )
            .as_bytes(),
        )
        .unwrap();
    // One read does not always return the full status line, so loop until it is enough to decide.
    let mut status_line = Vec::new();
    while status_line.len() < 12 {
        let mut chunk = [0u8; 64];
        let read = client.read(&mut chunk).unwrap();
        if read == 0 {
            break;
        }
        status_line.extend_from_slice(&chunk[..read]);
    }
    let elapsed = started.elapsed();

    assert!(
        String::from_utf8_lossy(&status_line).starts_with("HTTP/1.1 200"),
        "actual response: {}",
        String::from_utf8_lossy(&status_line)
    );
    assert!(
        elapsed < Duration::from_millis(1_500),
        "a silent client blocked the request for {elapsed:?}: the worker pool did not work"
    );
    controller.shutdown();
}

#[test]
fn disabling_does_not_wait_for_a_silent_client() {
    let dir = temp_dir("disable-fast");
    let address = ephemeral_address();
    let controller = controller_at(&dir, address.clone());
    controller.set_enabled(true);
    let _silent = ClientStream::connect(address.as_str()).unwrap();
    let started = std::time::Instant::now();
    let disabled = controller.set_enabled(false);
    let elapsed = started.elapsed();
    assert!(!disabled.running);
    assert!(
        elapsed < Duration::from_millis(800),
        "disabling the local API waited {elapsed:?}: shutdown still joins workers on the UI thread"
    );
    assert!(port_is_released(&address));
}

#[test]
#[should_panic(expected = "Local API may only bind to 127.0.0.1")]
fn refuses_a_non_loopback_bind_address() {
    let dir = temp_dir("not-loopback");
    let _ = LocalApiController::with_bind_address(
        dir,
        "0.0.0.0:43921".to_string(),
        Arc::new(MemoryCredentials::default()),
    );
}

#[test]
fn rotating_the_token_invalidates_the_previous_one_immediately() {
    let dir = temp_dir("rotate");
    let controller = controller(&dir);
    let first = controller.reveal_token().unwrap();
    assert!(first.starts_with(TOKEN_PREFIX));
    assert_eq!(controller.reveal_token().unwrap(), first);

    controller.set_enabled(true);
    let second = controller.rotate_token().unwrap();
    assert_ne!(first, second);
    assert_eq!(controller.reveal_token().unwrap(), second);
    assert!(!tokens_match(&second, &first));
    controller.shutdown();
}

#[test]
fn port_conflict_surfaces_as_an_api_error_without_pretending_to_run() {
    let dir = temp_dir("port-conflict");
    let squatter = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = squatter.local_addr().unwrap().to_string();
    let port = squatter.local_addr().unwrap().port().to_string();
    let controller = controller_at(&dir, address);
    let status = controller.set_enabled(true);
    assert!(status.enabled, "the user's enabled intent must be saved");
    assert!(!status.running, "with the port in use, it must not claim to be running");
    let message = status.error.expect("a port in use must give an explainable error");
    assert!(message.contains(&port), "{message}");
    assert_eq!(
        status.error_code.as_deref(),
        Some("err.local_api.port_in_use")
    );
    drop(squatter);
}
