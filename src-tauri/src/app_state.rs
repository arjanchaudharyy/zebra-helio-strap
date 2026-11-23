use crate::auth::AuthManager;
use crate::connectors::ZeppConnector;
use crate::fetcher::DataFetcher;
use crate::ipc_types::LoginStatus;
use crate::models::{error::Result, AuthInfo, ZeppBridgeError};
use crate::paths;
use crate::storage::Database;
use crate::sync::SyncManager;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

/// In-process web-login session.  The epoch is incremented to cancel a
/// running poll without holding a task join handle across commands.
pub(crate) struct LoginSession {
    pub(crate) status: Arc<RwLock<LoginStatus>>,
    pub(crate) epoch: Arc<AtomicU64>,
    pub(crate) pending_credentials: Arc<std::sync::Mutex<Option<PendingLogin>>>,
}

// Never derive Serialize or Debug: this receipt contains a live App Token.
#[derive(Clone)]
pub(crate) struct PendingLogin {
    pub(crate) auth: AuthInfo,
    pub(crate) epoch: u64,
    pub(crate) confidence: Option<&'static str>,
    pub(crate) received: std::time::Instant,
}

impl LoginSession {
    fn new() -> Self {
        Self {
            status: Arc::new(RwLock::new(LoginStatus::idle())),
            epoch: Arc::new(AtomicU64::new(0)),
            pending_credentials: Arc::new(std::sync::Mutex::new(None)),
        }
    }
}

/// Shared process state for the Tauri commands.
///
/// The primary database connection is kept here for read paths.  A sync
/// manager owns an independent connection to the same SQLite database so a
/// long-running network sync never holds the command-side database lock.
pub struct AppState {
    pub(crate) data_dir: PathBuf,
    pub(crate) db: Arc<Mutex<Database>>,
    pub(crate) auth: Arc<AuthManager>,
    pub(crate) sync: Arc<RwLock<Option<Arc<SyncManager>>>>,
    pub(crate) sync_command_lock: Arc<Mutex<()>>,
    pub(crate) login: LoginSession,
    pub(crate) auth_state: Arc<RwLock<String>>,
    /// One-time warnings produced while restoring state at startup (legacy
    /// migration, raw-record replay, initial auth restore).
    pub(crate) startup_warning: Arc<RwLock<Option<String>>>,
    /// Runtime auth warnings (verify/save failures).  Kept separate from
    /// `startup_warning` so a successful sync can clear the transient auth
    /// warning without erasing one-time startup notices.
    pub(crate) auth_warning: Arc<RwLock<Option<String>>>,
    /// At the last sign-in, the reason to treat the current `region_host` as this account's region.
    ///
    /// `identified`, `hinted` or `unconfirmed`, plus `unknown` after a process restart, when it cannot be known.
    /// It is kept because "the region guess was wrong" and "this account really has no data for this period"
    /// look the same in the UI: in both cases the sync succeeds with zero records. See
    /// `commands::login::RegionWinner`.
    pub(crate) region_confidence: Arc<RwLock<String>>,
}

impl AppState {
    /// Tests must never call the startup migration against the host user's XDG
    /// directories. This constructor uses only an explicitly supplied new fixture.
    #[cfg(all(test, unix))]
    pub(crate) fn isolated_test_state(data_dir: PathBuf) -> Result<Self> {
        std::fs::create_dir(&data_dir)?;
        let db = Database::open_migrated(&data_dir.join("zepp.db"))?;
        Ok(Self {
            auth: Arc::new(AuthManager::new(data_dir.clone())),
            data_dir,
            db: Arc::new(Mutex::new(db)),
            sync: Arc::new(RwLock::new(None)),
            sync_command_lock: Arc::new(Mutex::new(())),
            login: LoginSession::new(),
            auth_state: Arc::new(RwLock::new("unconfigured".into())),
            startup_warning: Arc::new(RwLock::new(None)),
            auth_warning: Arc::new(RwLock::new(None)),
            region_confidence: Arc::new(RwLock::new("unknown".into())),
        })
    }

    /// Initialize local storage and restore the saved authentication state.
    ///
    /// A malformed or stale credential is intentionally recoverable: the
    /// application still starts with an empty sync manager and an actionable
    /// warning for the settings screen to display.
    pub fn new(data_dir: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(&data_dir)?;

        let migration_warning = paths::relocate_legacy_data(&data_dir);
        let (db, db_warning) = match Database::open_resilient(data_dir.join("zepp.db")) {
            Ok(opened) => opened,
            Err(error) if error.is_busy() => {
                crate::diagnostics::log(&format!(
                    "Startup: another writer holds the library, opening read-only ({error})"
                ));
                // Do not rebuild an empty library. Startup fails only when the read-only open fails (file missing, schema mismatch).
                let db = Database::open_read_only(data_dir.join("zepp.db"))?;
                (db, Some(error.user_message()))
            }
            Err(error) => return Err(error),
        };
        // Do not replay every raw payload during startup. A full reprocess of a
        // large local library blocks window creation and looks like a hang.
        // Settings still has an explicit reprocess action; the next cloud sync
        // also refreshes workout source/type fields.
        let auth = Arc::new(AuthManager::new(data_dir.clone()));

        let (sync_manager, auth_state, auth_warning) = match auth.load_auth() {
            Ok(Some(auth_info)) => match Self::build_sync_manager(auth_info, &data_dir) {
                Ok(manager) => (Some(manager), "configured".to_string(), None),
                Err(error) => (
                    None,
                    "needs_reauth".to_string(),
                    Some(startup_warning(error)),
                ),
            },
            Ok(None) => (None, "unconfigured".to_string(), None),
            Err(error) => (
                None,
                "needs_reauth".to_string(),
                Some(startup_warning(error)),
            ),
        };
        let startup_warning = merge_startup_warnings(
            merge_startup_warnings(migration_warning, db_warning),
            auth_warning,
        );

        Ok(Self {
            data_dir,
            db: Arc::new(Mutex::new(db)),
            auth,
            sync: Arc::new(RwLock::new(sync_manager)),
            sync_command_lock: Arc::new(Mutex::new(())),
            login: LoginSession::new(),
            auth_state: Arc::new(RwLock::new(auth_state)),
            startup_warning: Arc::new(RwLock::new(startup_warning)),
            auth_warning: Arc::new(RwLock::new(None)),
            // Saved credentials do not record how the region was decided, so after a restart we can only say unknown.
            region_confidence: Arc::new(RwLock::new("unknown".to_string())),
        })
    }

    /// Appends a startup notice to the content the settings page shows.
    ///
    /// Recovery runs before `AppState` exists, but the user must see its result. On success it says
    /// "Restored from backup". On failure it says "The current library is unchanged".
    pub fn push_startup_warning(&self, notice: String) {
        let mut current = self.startup_warning.blocking_write();
        *current = Some(match current.take() {
            Some(existing) => format!(
                "{existing}
{notice}"
            ),
            None => notice,
        });
    }

    /// Build a synchronizer with its own SQLite connection.
    pub(crate) fn build_sync_manager(auth: AuthInfo, data_dir: &Path) -> Result<Arc<SyncManager>> {
        let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let account = auth.user_id.clone();
        let connector = ZeppConnector::with_cancel(auth, cancel.clone())?;
        let fetcher = DataFetcher::new(connector);
        // The primary connection in `AppState::new` already migrated the
        // schema; re-running DDL here could collide with an active sync.
        let db = Database::open_without_migration(data_dir.join("zepp.db"))?;
        // With the data folder passed in, sync also takes the cross-process write lock: the CLI and the desktop app can never
        // write the same library at the same time.
        Ok(Arc::new(
            SyncManager::new(fetcher, db, cancel)
                .with_data_dir(data_dir.to_path_buf())
                .with_account(account),
        ))
    }

    /// Kick any in-flight `start_*_sync` so it can drop `sync_command_lock`,
    /// then acquire that lock. Hold the guard across the rest of an auth change.
    pub(crate) async fn lock_sync_commands(&self) -> tokio::sync::MutexGuard<'_, ()> {
        if let Some(manager) = self.sync.read().await.clone() {
            manager.request_cancel();
        }
        self.sync_command_lock.lock().await
    }

    /// Wait for the current synchronizer to release its run lock, then replace it.
    ///
    /// Callers must already hold `sync_command_lock` (see `lock_sync_commands`)
    /// so a waiting `start_*_sync` cannot install work against the old handle
    /// after we swap.
    pub(crate) async fn replace_sync_manager(&self, next: Option<Arc<SyncManager>>) {
        let current = self.sync.read().await.clone();
        if let Some(manager) = current {
            manager.cancel_and_wait().await;
        }
        *self.sync.write().await = next;
    }
}

/// Mask an account identifier while retaining enough context for the user to
/// recognize which account is configured.
pub(crate) fn mask_user_id(user_id: &str) -> String {
    let chars: Vec<char> = user_id.chars().collect();
    match chars.len() {
        0 => String::new(),
        1 => "•".to_string(),
        2 => format!("{}•", chars[0]),
        _ => {
            let prefix = chars[0];
            let suffix = chars[chars.len() - 1];
            format!("{prefix}•••{suffix}")
        }
    }
}

fn startup_warning(error: ZeppBridgeError) -> String {
    format!(
        "Cannot restore Zepp authentication. Reconnect in Settings and try again: {}",
        error.user_message()
    )
}

fn merge_startup_warnings(
    migration_warning: Option<String>,
    auth_warning: Option<String>,
) -> Option<String> {
    match (migration_warning, auth_warning) {
        (Some(migration), Some(auth)) => Some(format!("{migration}\n{auth}")),
        (Some(warning), None) | (None, Some(warning)) => Some(warning),
        (None, None) => None,
    }
}
