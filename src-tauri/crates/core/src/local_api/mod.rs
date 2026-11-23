use crate::auth::{default_credential_backend_in, CredentialBackend};

use crate::models::WorkoutSeries;

use crate::storage::Database;

use serde::Serialize;

use serde_json::json;

use std::io::{self, BufRead, BufReader, Read, Write};

use std::net::{SocketAddr, TcpListener, TcpStream};

use std::path::{Path, PathBuf};

use std::sync::atomic::{AtomicBool, Ordering};

use std::sync::{mpsc, Arc, Mutex, RwLock};

use std::time::Duration;

mod headless;
mod http;
mod limit;
mod v1;

pub use headless::*;
pub use http::*;
pub use limit::{RateLimiter, Verdict};
pub use v1::*;

pub const LOCAL_API_ADDRESS: &str = "127.0.0.1:43921";

pub const LOCAL_API_BASE_URL: &str = "http://127.0.0.1:43921";

/// The account name for the local API token in the credential store. It shares
/// the service with the Zepp account token, but the account name is fixed, so a
/// Zepp account switch does not touch the local API credential.
pub const LOCAL_API_CREDENTIAL_ACCOUNT: &str = "local-api-token";

const ENABLED_STATE_FILE: &str = "local-api.json";

// Parse limits for the minimal HTTP server. Without them, a local process could
// block the receive thread with a header line that never ends, or fill memory
// with tens of thousands of headers.
const MAX_REQUEST_LINE_BYTES: usize = 8 * 1024;

const MAX_HEADER_LINE_BYTES: usize = 8 * 1024;

const MAX_HEADER_LINES: usize = 64;

const MAX_HEADER_TOTAL_BYTES: usize = 32 * 1024;

const MAX_WORKOUT_ID_BYTES: usize = 256;

const ACCEPT_POLL_INTERVAL: Duration = Duration::from_millis(50);

/// The number of worker threads that handle connections at the same time.
///
/// Before, the server accepted one connection, handled it until the response was
/// written, then accepted the next. With a 2 s read timeout and a 10 s write
/// timeout, one local client that sent headers slowly could block a normal AI
/// client for more than ten seconds. Tokio is not necessary: this service has
/// only three read-only endpoints, so a small bounded thread pool is enough.
///
/// The bound is the key point: without a bound on thread-per-connection, any
/// local process could exhaust the thread count by opening connections.
const WORKER_THREADS: usize = 4;

/// The accept queue limit. With an unbounded channel, any local process could fill memory by opening connections.
const ACCEPT_QUEUE: usize = WORKER_THREADS * 2;

const TOKEN_PREFIX: &str = "zbk_";

const TOKEN_RANDOM_BYTES: usize = 32;

/// The live status the settings page sees. `running` comes from the listener the
/// controller holds now, not from a snapshot at startup.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LocalApiStatus {
    /// The enabled intent that the user saved.
    pub enabled: bool,
    /// Whether the port really listens now.
    pub running: bool,
    pub base_url: String,
    pub address: String,
    pub workout_series_path: String,
    /// Whether a token was generated. It can be true while disabled (the token is kept).
    pub token_present: bool,
    /// Explainable errors of the API itself only (port in use, credential store unavailable, and so on).
    pub error: Option<String>,
    /// The stable code of the `error` text. The UI uses it to look up text in its own language.
    #[serde(default)]
    pub error_code: Option<String>,
}

impl LocalApiStatus {
    fn new(
        enabled: bool,
        running: bool,
        token_present: bool,
        error: Option<String>,
        error_code: Option<String>,
    ) -> Self {
        Self {
            enabled,
            running,
            base_url: LOCAL_API_BASE_URL.to_string(),
            address: LOCAL_API_ADDRESS.to_string(),
            workout_series_path: "/workouts/{id}/series".to_string(),
            token_present,
            error,
            error_code,
        }
    }
}

/// The tokens this service accepts. The two keys are independent:
///
/// - `local`: the local token from the desktop settings page. It can read all read-only routes.
/// - `pull`: the token for pull clients (SparkyFitness and others). It can only read `/v1/*`
///   and `/health`, and can be replaced or revoked alone without effect on the local token.
///   The desktop app never sets it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TokenSet {
    pub local: Option<String>,
    pub pull: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenScope {
    Local,
    Pull,
}

impl TokenSet {
    /// Compare both (in constant time). Do not return early when one matches first.
    pub fn authorize(&self, provided: &str) -> Option<TokenScope> {
        let local = self
            .local
            .as_deref()
            .is_some_and(|expected| tokens_match(expected, provided));
        let pull = self
            .pull
            .as_deref()
            .is_some_and(|expected| tokens_match(expected, provided));
        if local {
            Some(TokenScope::Local)
        } else if pull {
            Some(TokenScope::Pull)
        } else {
            None
        }
    }
}

/// All runtime state of one listener instance, shared by all worker threads.
pub(crate) struct ServerContext {
    pub(crate) data_dir: PathBuf,
    pub(crate) tokens: Arc<RwLock<TokenSet>>,
    /// Whether to accept connections from non-loopback sources. Always `false` on the desktop.
    pub(crate) allow_remote: bool,
    pub(crate) limiter: RateLimiter,
    pub(crate) cache: RecordsCache,
    /// Log one line per request (method, path, status code, count; no query parameters and no token).
    pub(crate) log_requests: bool,
}

/// The running server. `token` uses an `RwLock`, not a copy, so after a token is
/// regenerated the old token stops working at once, with no listener restart.
struct RunningServer {
    stop: Arc<AtomicBool>,
    token: Arc<RwLock<TokenSet>>,
    /// The address actually bound. In production it always equals `LOCAL_API_ADDRESS`.
    /// Tests use `127.0.0.1:0` to get a free port, so lifecycle tests do not depend on port 43921 being free.
    local_addr: std::net::SocketAddr,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl RunningServer {
    fn is_alive(&self) -> bool {
        self.handle
            .as_ref()
            .is_some_and(|handle| !handle.is_finished())
    }

    /// Stop accepting connections. The accept loop sleeps at most one more poll
    /// interval before it drops the listener, so this join does not block the UI
    /// thread on the read timeout of a slow client.
    fn shutdown(mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

struct ControllerInner {
    enabled: bool,
    server: Option<RunningServer>,
    error: Option<String>,
    error_code: Option<String>,
}

impl ControllerInner {
    fn set_fault(&mut self, code: &'static str, message: String) {
        self.error_code = Some(code.to_string());
        self.error = Some(message);
    }

    fn clear_fault(&mut self) {
        self.error = None;
        self.error_code = None;
    }
}

/// The only lifecycle manager of the local API.
///
/// The listener, enabled state, stop signal, token, thread handle and errors are all
/// here, so the settings page always reads the current state, not a startup snapshot.
pub struct LocalApiController {
    data_dir: PathBuf,
    /// The address to bind. Always loopback: this field only lets tests use a free
    /// port. It is not a switch to expose the API on the LAN, and no command can change it.
    bind_address: String,
    credentials: Arc<dyn CredentialBackend>,
    inner: Mutex<ControllerInner>,
}

impl std::fmt::Debug for LocalApiController {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalApiController")
            .field("address", &LOCAL_API_ADDRESS)
            .finish_non_exhaustive()
    }
}

impl LocalApiController {
    pub fn new(data_dir: PathBuf) -> Self {
        let credentials = default_credential_backend_in(&data_dir);
        Self::with_credential_backend(data_dir, credentials)
    }

    pub fn with_credential_backend(
        data_dir: PathBuf,
        credentials: Arc<dyn CredentialBackend>,
    ) -> Self {
        Self::with_bind_address(data_dir, LOCAL_API_ADDRESS.to_string(), credentials)
    }

    fn with_bind_address(
        data_dir: PathBuf,
        bind_address: String,
        credentials: Arc<dyn CredentialBackend>,
    ) -> Self {
        assert!(
            is_loopback_bind_address(&bind_address),
            "Local API may only bind to 127.0.0.1, got: {bind_address}"
        );
        Self {
            data_dir,
            bind_address,
            credentials,
            inner: Mutex::new(ControllerInner {
                enabled: false,
                server: None,
                error: None,
                error_code: None,
            }),
        }
    }

    /// At startup, restore the enabled state that the user saved explicitly. No
    /// state file means "never enabled", so it stays off: a first install never
    /// listens on a port.
    ///
    /// A port in use or an unavailable credential store only puts the API in an
    /// explainable error state. The caller must not block the desktop app start because of it.
    pub fn restore(&self) -> LocalApiStatus {
        let enabled = read_enabled_flag(&self.data_dir);
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.enabled = enabled;
        self.reap_dead_server(&mut inner);
        if enabled {
            self.spawn_locked(&mut inner);
        }
        self.status_locked(&inner)
    }

    pub fn status(&self) -> LocalApiStatus {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        self.reap_dead_server(&mut inner);
        self.status_locked(&inner)
    }

    /// Takes effect at once: no app restart after enabling, and the port is released at once after disabling.
    pub fn set_enabled(&self, enabled: bool) -> LocalApiStatus {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.enabled = enabled;
        inner.clear_fault();
        self.reap_dead_server(&mut inner);
        if enabled {
            if inner.server.is_none() {
                self.spawn_locked(&mut inner);
            }
        } else if let Some(server) = inner.server.take() {
            server.shutdown();
        }
        // Report a save failure. This switch means "still on at next start". If
        // it cannot be saved, it only works in this process, and the user has a
        // right to know.
        //
        // But if `spawn_locked` already reported a more specific fault (for
        // example, port in use), do not overwrite it. When both fail, the user
        // must see the real cause (port in use), not a later disk write failure.
        if let Err(error) = write_enabled_flag(&self.data_dir, enabled) {
            if inner.error_code.is_none() {
                inner.set_fault("err.local_api.state_write_failed", error);
            }
        }
        self.status_locked(&inner)
    }

    /// Read the current token so the UI can show or copy it. The UI masks it by default and calls this only after a user click.
    pub fn reveal_token(&self) -> Result<String, String> {
        self.ensure_token()
    }

    /// Regenerate the token. The old token stops working at once: the running
    /// server shares the same `RwLock`, so the next request compares with the new value.
    pub fn rotate_token(&self) -> Result<String, String> {
        let token = generate_token()?;
        self.credentials
            .set(LOCAL_API_CREDENTIAL_ACCOUNT, &token)
            .map_err(|error| format!("cannot write the local API credential: {error}"))?;
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(server) = inner.server.as_ref() {
            let mut current = server.token.write().unwrap_or_else(|e| e.into_inner());
            current.local = Some(token.clone());
        }
        Ok(token)
    }

    /// Release the port when the app exits.
    pub fn shutdown(&self) {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(server) = inner.server.take() {
            server.shutdown();
        }
    }

    fn status_locked(&self, inner: &ControllerInner) -> LocalApiStatus {
        let token_present = matches!(
            self.credentials.get(LOCAL_API_CREDENTIAL_ACCOUNT),
            Ok(Some(_))
        );
        let mut status = LocalApiStatus::new(
            inner.enabled,
            inner.server.as_ref().is_some_and(RunningServer::is_alive),
            token_present,
            inner.error.clone(),
            inner.error_code.clone(),
        );
        if let Some(server) = inner.server.as_ref() {
            status.address = server.local_addr.to_string();
            status.base_url = format!("http://{}", server.local_addr);
        } else if self.bind_address != LOCAL_API_ADDRESS {
            status.address = self.bind_address.clone();
        }
        status
    }

    fn ensure_token(&self) -> Result<String, String> {
        match self.credentials.get(LOCAL_API_CREDENTIAL_ACCOUNT) {
            Ok(Some(token)) if !token.trim().is_empty() => Ok(token),
            Ok(_) => {
                let token = generate_token()?;
                self.credentials
                    .set(LOCAL_API_CREDENTIAL_ACCOUNT, &token)
                    .map_err(|error| format!("cannot write the local API credential: {error}"))?;
                Ok(token)
            }
            Err(error) => Err(format!("cannot read the local API credential: {error}")),
        }
    }

    fn spawn_locked(&self, inner: &mut ControllerInner) {
        let token = match self.ensure_token() {
            Ok(token) => token,
            Err(error) => {
                inner.set_fault("err.local_api.token_unavailable", error);
                return;
            }
        };
        let listener = match TcpListener::bind(self.bind_address.as_str()) {
            Ok(listener) => listener,
            Err(error) => {
                inner.set_fault(
                    bind_error_code(&error),
                    bind_error_message(&self.bind_address, &error),
                );
                return;
            }
        };
        let local_addr = match listener.local_addr() {
            Ok(address) => address,
            Err(error) => {
                inner.set_fault(
                    bind_error_code(&error),
                    bind_error_message(&self.bind_address, &error),
                );
                return;
            }
        };
        if !local_addr.ip().is_loopback() {
            inner.set_fault(
                "err.local_api.bind_failed",
                "Local API may only bind to loopback".to_string(),
            );
            return;
        }
        if let Err(error) = listener.set_nonblocking(true) {
            inner.set_fault(
                bind_error_code(&error),
                bind_error_message(&self.bind_address, &error),
            );
            return;
        }

        let stop = Arc::new(AtomicBool::new(false));
        let token = Arc::new(RwLock::new(TokenSet {
            local: Some(token),
            pull: None,
        }));
        // Desktop: accept loopback sources only, with no request limit (a local AI
        // client reads many workouts in a row). Only auth failures are limited.
        let context = Arc::new(ServerContext {
            data_dir: self.data_dir.clone(),
            tokens: token.clone(),
            allow_remote: false,
            limiter: RateLimiter::new(None),
            cache: RecordsCache::default(),
            log_requests: false,
        });
        let thread_stop = stop.clone();
        match std::thread::Builder::new()
            .name("zeppbridge-local-api".to_string())
            .spawn(move || serve(listener, context, thread_stop))
        {
            Ok(handle) => {
                inner.clear_fault();
                inner.server = Some(RunningServer {
                    stop,
                    token,
                    local_addr,
                    handle: Some(handle),
                });
            }
            Err(error) => {
                inner.set_fault(
                    "err.local_api.thread_failed",
                    format!("cannot start the local API thread: {error}"),
                );
            }
        }
    }

    fn reap_dead_server(&self, inner: &mut ControllerInner) {
        if inner.server.as_ref().is_some_and(RunningServer::is_alive) {
            return;
        }
        if let Some(mut server) = inner.server.take() {
            if let Some(handle) = server.handle.take() {
                let _ = handle.join();
            }
            if inner.enabled && inner.error.is_none() {
                inner.set_fault(
                    "err.local_api.thread_failed",
                    "the local API listener thread exited".to_string(),
                );
            }
        }
    }
}

fn is_loopback_bind_address(address: &str) -> bool {
    address.starts_with("127.0.0.1:")
}

fn bind_error_code(error: &io::Error) -> &'static str {
    if error.kind() == io::ErrorKind::AddrInUse {
        "err.local_api.port_in_use"
    } else {
        "err.local_api.bind_failed"
    }
}

fn bind_error_message(address: &str, error: &io::Error) -> String {
    let port = address.rsplit_once(':').map_or(address, |(_, port)| port);
    if error.kind() == io::ErrorKind::AddrInUse {
        format!("local port {port} is already in use by another program: {error}")
    } else {
        format!("cannot start the local API: {error}")
    }
}

fn state_file(data_dir: &Path) -> PathBuf {
    data_dir.join(ENABLED_STATE_FILE)
}

fn read_enabled_flag(data_dir: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(state_file(data_dir)) else {
        return false;
    };
    serde_json::from_str::<serde_json::Value>(&text)
        .ok()
        .and_then(|value| value.get("enabled").and_then(serde_json::Value::as_bool))
        .unwrap_or(false)
}

/// Write the "local API on or off" flag atomically.
///
/// Before, this was `let _ = std::fs::write(...)`: on a read-only or full disk the
/// error was lost. The UI showed "on", the setting was gone after a restart, and
/// nothing told the user.
///
/// Write a temporary file first, then rename. A direct write to the target can
/// leave half a JSON file if the process is killed. The next `read_enabled_flag`
/// then fails to parse it and silently treats it as "off".
fn write_enabled_flag(data_dir: &Path, enabled: bool) -> Result<(), String> {
    let target = state_file(data_dir);
    let temporary = target.with_extension("json.tmp");
    let body = json!({ "enabled": enabled }).to_string();
    let describe = |error: std::io::Error| format!("cannot save the local API switch state: {error}");
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(describe)?;
    }
    std::fs::write(&temporary, body).map_err(describe)?;
    // On Windows, rename may overwrite an existing file (std uses
    // MoveFileEx + REPLACE_EXISTING).
    std::fs::rename(&temporary, &target).map_err(|error| {
        // Remove the temporary file, or .tmp files pile up in the data folder.
        let _ = std::fs::remove_file(&temporary);
        describe(error)
    })
}

fn generate_token() -> Result<String, String> {
    let mut bytes = [0u8; TOKEN_RANDOM_BYTES];
    getrandom::getrandom(&mut bytes)
        .map_err(|_| "cannot generate a local API token from the system secure random source".to_string())?;
    Ok(format!("{TOKEN_PREFIX}{}", hex::encode(bytes)))
}

/// Constant-time compare, so response time does not reveal a token prefix.
fn tokens_match(expected: &str, provided: &str) -> bool {
    let expected = expected.as_bytes();
    let provided = provided.as_bytes();
    if expected.len() != provided.len() {
        return false;
    }
    let mut diff = 0u8;
    for (a, b) in expected.iter().zip(provided.iter()) {
        diff |= a ^ b;
    }
    diff == 0
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod v1_tests;
