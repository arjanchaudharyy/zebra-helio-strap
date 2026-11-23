//! The resident read-only service of the headless version (`zeppbridge-cli serve`).
//!
//! It shares the HTTP handling and routes with the desktop [`LocalApiController`].
//! It differs in three ways only:
//!
//! - **The listen address** is set by the user with [`LISTEN_ENV`]. Unset, it listens
//!   only on `127.0.0.1`. The desktop has no such entry: its address is fixed in
//!   [`LOCAL_API_ADDRESS`], and no setting can change it.
//! - **Auth uses the pull token**, a separate key from the desktop local token. The
//!   pull token can only read `/v1/*` and `/health`, and can be replaced or revoked
//!   alone. A non-loopback listen address requires it, or the start is refused.
//! - **Rate limit per source IP**, against token brute force from devices on the same network.
//!
//! The transport is plaintext HTTP. For cross-machine access, put it behind a
//! reverse proxy (HTTPS) or inside a container network. The startup log says this.

use super::limit::RateLimiter;
use super::*;
use std::net::SocketAddr;

/// The listen address environment variable, for example `0.0.0.0:43921`. Unset = `127.0.0.1:43921`.
pub const LISTEN_ENV: &str = "ZEPPBRIDGE_LISTEN";

/// The environment variable that gives the pull token directly (easiest in Docker: it goes in the same .env as the Sparky config).
pub const PULL_TOKEN_ENV: &str = "ZEPPBRIDGE_PULL_TOKEN";

/// The account name of the pull token in the credential store. Separate from the local token ([`LOCAL_API_CREDENTIAL_ACCOUNT`]).
pub const PULL_CREDENTIAL_ACCOUNT: &str = "local-api-pull-token";

/// The minimum length of a user-supplied token: a short token can be guessed on a LAN in minutes.
pub const MIN_PULL_TOKEN_CHARS: usize = 32;

/// The request limit per source per minute for the headless version.
pub const HEADLESS_REQUESTS_PER_MINUTE: u32 = 120;

const PULL_TOKEN_PREFIX: &str = "zbp_";

/// How often to reread the token from the credential store: a revoke takes effect in seconds, with no service restart.
const TOKEN_REFRESH: Duration = Duration::from_secs(5);

/// Where the pull token comes from.
#[derive(Clone)]
pub enum PullTokenSource {
    /// A fixed value from an environment variable. A change needs a service restart.
    Env(String),
    /// The credential store (managed by `zeppbridge-cli pull-token`). The running service rereads it at intervals.
    Store(Arc<dyn CredentialBackend>),
}

impl std::fmt::Debug for PullTokenSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Env(_) => f.write_str("PullTokenSource::Env(<redacted>)"),
            Self::Store(_) => f.write_str("PullTokenSource::Store"),
        }
    }
}

/// Why the start failed. `code` is a stable code, `message` is English text for people.
#[derive(Debug)]
pub enum HeadlessError {
    InvalidListen(String),
    NeedsToken,
    WeakToken,
    Credentials(String),
    Bind(io::Error),
}

impl HeadlessError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidListen(_) => "invalid_listen_address",
            Self::NeedsToken => "pull_token_required",
            Self::WeakToken => "pull_token_too_short",
            Self::Credentials(_) => "credential_store_unavailable",
            Self::Bind(error) if error.kind() == io::ErrorKind::AddrInUse => "port_in_use",
            Self::Bind(_) => "bind_failed",
        }
    }

    pub fn message(&self) -> String {
        match self {
            Self::InvalidListen(value) => format!(
                "{LISTEN_ENV}={value} is not an address like 127.0.0.1:43921 or 0.0.0.0:43921"
            ),
            Self::NeedsToken => format!(
                "Refusing to listen beyond this machine without a pull token. Set {PULL_TOKEN_ENV} \
                 (at least {MIN_PULL_TOKEN_CHARS} characters) or create one with `zeppbridge-cli pull-token rotate`."
            ),
            Self::WeakToken => format!(
                "{PULL_TOKEN_ENV} is shorter than {MIN_PULL_TOKEN_CHARS} characters; use a long random value"
            ),
            Self::Credentials(reason) => format!("The credential store is unavailable: {reason}"),
            Self::Bind(error) => format!("Could not listen: {error}"),
        }
    }
}

/// Parse [`LISTEN_ENV`]. `None` or blank = listen locally only.
pub fn parse_listen(value: Option<&str>) -> Result<SocketAddr, HeadlessError> {
    let text = value.map(str::trim).filter(|value| !value.is_empty());
    let text = text.unwrap_or(LOCAL_API_ADDRESS);
    text.parse::<SocketAddr>()
        .map_err(|_| HeadlessError::InvalidListen(text.to_string()))
}

/// The pull token may only live in a store that keeps entries by name. The env
/// backend returns the Zepp App Token for any name, so using it as a pull token
/// would hand the account token to the LAN.
fn named_store(credentials: &dyn CredentialBackend) -> Result<(), String> {
    if credentials.supports_named_entries() {
        Ok(())
    } else {
        Err(format!(
            "this credential store cannot hold a separate pull token; set {PULL_TOKEN_ENV} instead"
        ))
    }
}

pub fn read_pull_token(credentials: &dyn CredentialBackend) -> Result<Option<String>, String> {
    named_store(credentials)?;
    Ok(credentials
        .get(PULL_CREDENTIAL_ACCOUNT)?
        .filter(|token| !token.trim().is_empty()))
}

/// Generate a new pull token and store it. The old one stops working at once (a running service follows within seconds).
pub fn rotate_pull_token(credentials: &dyn CredentialBackend) -> Result<String, String> {
    named_store(credentials)?;
    let mut bytes = [0u8; TOKEN_RANDOM_BYTES];
    getrandom::getrandom(&mut bytes).map_err(|_| "no secure random source".to_string())?;
    let token = format!("{PULL_TOKEN_PREFIX}{}", hex::encode(bytes));
    credentials.set(PULL_CREDENTIAL_ACCOUNT, &token)?;
    Ok(token)
}

/// Revoke: delete the pull token. The local token is not affected.
pub fn revoke_pull_token(credentials: &dyn CredentialBackend) -> Result<(), String> {
    named_store(credentials)?;
    credentials.delete(PULL_CREDENTIAL_ACCOUNT)
}

/// The running headless service.
pub struct HeadlessServer {
    pub local_addr: SocketAddr,
    /// This start generated a new pull token (loopback listener and no token before).
    pub created_token: bool,
    stop: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl HeadlessServer {
    /// Stop the service (the accept loop exits after at most one more sleep).
    pub fn stop_flag(&self) -> Arc<AtomicBool> {
        self.stop.clone()
    }

    /// The accept loop is still alive.
    pub fn is_running(&self) -> bool {
        self.handle
            .as_ref()
            .is_some_and(|handle| !handle.is_finished())
    }

    /// Wait for the service to exit.
    pub fn wait(mut self) {
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

/// Start the headless service. When it returns, it already listens.
pub fn start_headless(
    data_dir: PathBuf,
    listen: SocketAddr,
    source: PullTokenSource,
) -> Result<HeadlessServer, HeadlessError> {
    let remote = !listen.ip().is_loopback();
    let mut created_token = false;
    let token = match &source {
        PullTokenSource::Env(value) => {
            let value = value.trim().to_string();
            if value.chars().count() < MIN_PULL_TOKEN_CHARS {
                return Err(HeadlessError::WeakToken);
            }
            value
        }
        PullTokenSource::Store(credentials) => {
            match read_pull_token(credentials.as_ref()).map_err(HeadlessError::Credentials)? {
                Some(value) => value,
                None if remote => return Err(HeadlessError::NeedsToken),
                None => {
                    // Listening locally with no token: generate one. Unauthenticated access stays closed.
                    created_token = true;
                    rotate_pull_token(credentials.as_ref()).map_err(HeadlessError::Credentials)?
                }
            }
        }
    };

    let listener = TcpListener::bind(listen).map_err(HeadlessError::Bind)?;
    let local_addr = listener.local_addr().map_err(HeadlessError::Bind)?;
    listener
        .set_nonblocking(true)
        .map_err(HeadlessError::Bind)?;

    let stop = Arc::new(AtomicBool::new(false));
    let tokens = Arc::new(RwLock::new(TokenSet {
        local: None,
        pull: Some(token),
    }));
    if let PullTokenSource::Store(credentials) = source {
        spawn_token_refresher(credentials, tokens.clone(), stop.clone());
    }
    let context = Arc::new(ServerContext {
        data_dir,
        tokens,
        allow_remote: true,
        limiter: RateLimiter::new(Some(HEADLESS_REQUESTS_PER_MINUTE)),
        cache: v1::RecordsCache::default(),
        log_requests: true,
    });
    let thread_stop = stop.clone();
    let handle = std::thread::Builder::new()
        .name("zeppbridge-serve".to_string())
        .spawn(move || serve(listener, context, thread_stop))
        .map_err(HeadlessError::Bind)?;
    Ok(HeadlessServer {
        local_addr,
        created_token,
        stop,
        handle: Some(handle),
    })
}

fn spawn_token_refresher(
    credentials: Arc<dyn CredentialBackend>,
    tokens: Arc<RwLock<TokenSet>>,
    stop: Arc<AtomicBool>,
) {
    let _ = std::thread::Builder::new()
        .name("zeppbridge-serve-token".to_string())
        .spawn(move || {
            let mut waited = Duration::ZERO;
            while !stop.load(Ordering::SeqCst) {
                std::thread::sleep(ACCEPT_POLL_INTERVAL);
                waited += ACCEPT_POLL_INTERVAL;
                if waited < TOKEN_REFRESH {
                    continue;
                }
                waited = Duration::ZERO;
                // If the read fails (store temporarily unavailable), keep the current value. Revoke only on an explicit "none".
                if let Ok(current) = read_pull_token(credentials.as_ref()) {
                    let mut guard = tokens.write().unwrap_or_else(|e| e.into_inner());
                    if guard.pull != current {
                        guard.pull = current;
                        eprintln!("serve: pull token changed in the credential store; applied");
                    }
                }
            }
        });
}

/// The notes written to the log for operators at startup (no token).
pub fn startup_notes(local_addr: SocketAddr) -> Vec<String> {
    let mut notes = vec![format!(
        "Read-only API listening on http://{local_addr} (routes: /v1/records, /v1, /health)"
    )];
    if !local_addr.ip().is_loopback() {
        notes.push(
            "Plaintext HTTP: keep it on a private container network, and put a reverse proxy \
             with HTTPS in front for access from other machines."
                .to_string(),
        );
    }
    notes
}
