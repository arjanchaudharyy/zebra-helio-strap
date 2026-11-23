//! Official tokens live in the system credential store; `official.json` in the data directory holds only non-sensitive metadata.
//!
//! It shares the credential backend with the legacy channel, but entry names have an `official:` prefix. Legacy entry names
//! are plain numeric user IDs, so the two never collide. The access token and refresh token are stored as separate entries:
//! Windows Credential Manager limits one entry to 2560 bytes, and the official token length is not documented.

use super::OfficialTokens;
use crate::auth::{default_credential_backend_in, CredentialBackend};
use crate::models::{error::Result, ZeppBridgeError};
use crate::storage::write_lock::{
    acquire_official, ExclusiveWriteGuard, OfficialLock, WriteLockError,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

const META_FILE: &str = "official.json";

/*
 * Commit coordination for the official account state.
 *
 * Login, cancel, disconnect and refresh all write credentials after a network wait. Without coordination, the
 * result of an old action can write a token back to a disconnected account, overwrite a new login, or change a just-renewed state to
 * "needs reauthorization". So:
 * - All credential / metadata writes happen inside one commit lock. It has two layers: an in-process
 *   `Mutex`, and a file lock in the data directory held by the OS. When the desktop disconnects, the CLI / MCP
 *   can refresh in another process. With only the in-process lock, that "check, then write back" can
 *   fall in the middle of this disconnect and write the token back to the disconnected account;
 * - `GENERATION` is the generation of the account state: start login, cancel and disconnect each add one. A login keeps
 *   the generation from its start and checks it before writing. A changed generation means the user cancelled or changed their mind. Login and
 *   cancel happen only in the desktop process, so an in-process generation is enough;
 * - Before a refresh writes back, it checks that the stored refresh token is still the one it used (compare-and-swap).
 */
static COMMIT: Mutex<()> = Mutex::new(());
static GENERATION: AtomicU64 = AtomicU64::new(0);

/// The maximum wait for another process to release the commit lock. The lock covers a few credential reads and writes, normally milliseconds.
const COMMIT_WAIT: Duration = Duration::from_secs(10);
/// The maximum wait for another process to finish a refresh: a bit more than the timeout of one refresh request (20 seconds).
const REFRESH_WAIT: Duration = Duration::from_secs(25);

fn in_process() -> MutexGuard<'static, ()> {
    COMMIT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// While held, no ZeppBridge process on this machine can change the official authorization state of this data directory.
struct CommitGuard {
    // Release the file lock first, then the in-process lock (fields drop in declaration order).
    _file: ExclusiveWriteGuard,
    _local: MutexGuard<'static, ()>,
}

/// Cancels all in-flight logins (called on new login, cancel and disconnect) and returns the new generation.
pub fn invalidate_pending_logins() -> u64 {
    let _commit = in_process();
    GENERATION.fetch_add(1, Ordering::SeqCst) + 1
}

/// The current generation. A login records it at the start and gives it to [`OfficialStore::save_login`] to check.
pub fn current_generation() -> u64 {
    GENERATION.load(Ordering::SeqCst)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OfficialMeta {
    pub user_id: String,
    /// The Unix seconds when authorization completed.
    pub connected_at: i64,
    pub expires_at: Option<i64>,
    /// Zepp explicitly refused the refresh: tokens are cleared, waiting for the user to authorize again.
    #[serde(default)]
    pub needs_reauth: bool,
    /// The nickname from the official profile: the settings page uses it to show which account is connected. Stored only locally.
    #[serde(default)]
    pub nickname: Option<String>,
}

pub struct OfficialStore {
    dir: PathBuf,
    backend: Arc<dyn CredentialBackend>,
}

fn access_key(user_id: &str) -> String {
    format!("official:{user_id}:access")
}

fn refresh_key(user_id: &str) -> String {
    format!("official:{user_id}:refresh")
}

impl OfficialStore {
    pub fn new(data_dir: &Path) -> Self {
        Self::with_backend(data_dir, default_credential_backend_in(data_dir))
    }

    pub fn with_backend(data_dir: &Path, backend: Arc<dyn CredentialBackend>) -> Self {
        Self {
            dir: data_dir.to_path_buf(),
            backend,
        }
    }

    /// In-process lock first, file lock second: two threads of one process do not spin on the file lock.
    fn commit(&self) -> Result<CommitGuard> {
        let local = in_process();
        let file =
            acquire_official(&self.dir, OfficialLock::Commit, COMMIT_WAIT).map_err(|error| {
                ZeppBridgeError::CredentialStore(format!(
                    "Another ZeppBridge process is updating the official authorization: {error}"
                ))
            })?;
        Ok(CommitGuard {
            _file: file,
            _local: local,
        })
    }

    /// Cross-process single-flight refresh (the in-process layer is in `refresh_tokens`). If the lock is busy, it retries
    /// after a short wait, up to the limit of one refresh round trip. If it still fails, it continues: the write-back is
    /// compare-and-swap, so the worst case is one extra refresh.
    pub(crate) async fn refresh_flight(&self) -> Option<ExclusiveWriteGuard> {
        let deadline = tokio::time::Instant::now() + REFRESH_WAIT;
        loop {
            match acquire_official(&self.dir, OfficialLock::Refresh, Duration::ZERO) {
                Ok(guard) => return Some(guard),
                Err(WriteLockError::Busy { .. }) if tokio::time::Instant::now() < deadline => {
                    tokio::time::sleep(Duration::from_millis(120)).await;
                }
                Err(error) => {
                    tracing::warn!("Did not get the cross-process refresh lock for the official token, refreshing anyway: {error}");
                    return None;
                }
            }
        }
    }

    fn meta_path(&self) -> PathBuf {
        self.dir.join(META_FILE)
    }

    pub fn meta(&self) -> Result<Option<OfficialMeta>> {
        let path = self.meta_path();
        if !path.exists() {
            return Ok(None);
        }
        let text = std::fs::read_to_string(&path)?;
        serde_json::from_str(&text).map(Some).map_err(|error| {
            ZeppBridgeError::ParseError(format!("official.json cannot be parsed: {error}"))
        })
    }

    fn write_meta(&self, meta: &OfficialMeta) -> Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        let text = serde_json::to_string_pretty(meta)
            .map_err(|error| ZeppBridgeError::ParseError(error.to_string()))?;
        // The temp file name differs each time: two writers with one fixed name would overwrite each other's partial content.
        let nonce = crate::unique_suffix();
        let temp = self
            .dir
            .join(format!("{META_FILE}.{}.{nonce}.tmp", std::process::id()));
        std::fs::write(&temp, text)?;
        std::fs::rename(temp, self.meta_path())?;
        Ok(())
    }

    /// Saves the tokens after a login claims them. `generation` is the generation at login start: if the user cancelled,
    /// disconnected or started again meanwhile (the generation changed), it writes nothing and returns `false`.
    pub fn save_login(
        &self,
        tokens: &OfficialTokens,
        nickname: Option<&str>,
        connected_at: i64,
        generation: u64,
    ) -> Result<bool> {
        let _commit = self.commit()?;
        if GENERATION.load(Ordering::SeqCst) != generation {
            return Ok(false);
        }
        self.save(tokens, connected_at)?;
        // The nickname is only for display: failing to store it does not affect the authorization.
        let _ = self.set_nickname(nickname);
        Ok(true)
    }

    /// Writes back after a successful refresh: only if the store still holds the token used for the refresh (same account, same
    /// refresh token). If it was disconnected, the account changed or someone else refreshed, it returns `false`.
    pub fn replace_if_current(
        &self,
        used: &OfficialTokens,
        fresh: &OfficialTokens,
        now: i64,
    ) -> Result<bool> {
        let _commit = self.commit()?;
        if !self.holds(used)? {
            return Ok(false);
        }
        self.save(fresh, now)?;
        Ok(true)
    }

    /// Zepp explicitly refused the refresh: mark "needs reauthorization" only if the store still holds that token.
    /// A concurrent refresh can already have stored a new token, and this refusal must not erase it.
    pub fn mark_needs_reauth_if_current(&self, used: &OfficialTokens) -> Result<bool> {
        let _commit = self.commit()?;
        if !self.holds(used)? {
            return Ok(false);
        }
        self.mark_needs_reauth()?;
        Ok(true)
    }

    /// Disconnect: cancel in-flight logins, then delete tokens and metadata. **Never touches any health data.**
    ///
    /// A keychain delete failure still counts as disconnected (see [`Self::clear`]); it returns the store's raw text for the caller to log.
    pub fn disconnect(&self) -> Result<Option<String>> {
        let _commit = self.commit()?;
        GENERATION.fetch_add(1, Ordering::SeqCst);
        self.clear()
    }

    fn holds(&self, used: &OfficialTokens) -> Result<bool> {
        Ok(self.load()?.is_some_and(|current| current.same_grant(used)))
    }

    /// Writes credentials first, then metadata: when metadata says "connected", the token is already there.
    /// On an account change, it also deletes the previous account's credential entries, so no unreferenced token stays.
    ///
    /// Does not take the commit lock: production paths use the checked entry points above.
    pub(crate) fn save(&self, tokens: &OfficialTokens, connected_at: i64) -> Result<()> {
        if !self.backend.supports_named_entries() {
            return Err(ZeppBridgeError::CredentialStore(
                "The current credential store holds only one legacy token and cannot store the official authorization".into(),
            ));
        }
        self.backend
            .set(&access_key(&tokens.user_id), &tokens.access_token)
            .map_err(ZeppBridgeError::CredentialStore)?;
        match tokens.refresh_token.as_deref() {
            Some(refresh) => self.backend.set(&refresh_key(&tokens.user_id), refresh),
            None => self.backend.delete(&refresh_key(&tokens.user_id)),
        }
        .map_err(ZeppBridgeError::CredentialStore)?;
        let before = self.meta()?;
        let replaced_user = before
            .as_ref()
            .filter(|meta| meta.user_id != tokens.user_id)
            .map(|meta| meta.user_id.clone());
        let previous = before.filter(|meta| meta.user_id == tokens.user_id);
        self.write_meta(&OfficialMeta {
            user_id: tokens.user_id.clone(),
            connected_at: previous
                .as_ref()
                .map_or(connected_at, |meta| meta.connected_at),
            expires_at: tokens.expires_at,
            needs_reauth: false,
            nickname: previous.and_then(|meta| meta.nickname),
        })?;
        if let Some(old_user) = replaced_user {
            self.delete_secrets(&old_user)?;
        }
        Ok(())
    }

    /// Records the nickname from the official profile (called after authorization succeeds). A blank nickname is not stored.
    pub(crate) fn set_nickname(&self, nickname: Option<&str>) -> Result<()> {
        let Some(mut meta) = self.meta()? else {
            return Ok(());
        };
        meta.nickname = nickname
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_string);
        self.write_meta(&meta)
    }

    /// Reads the full token set. Returns `None` if never connected or the tokens were cleared.
    pub fn load(&self) -> Result<Option<OfficialTokens>> {
        let Some(meta) = self.meta()? else {
            return Ok(None);
        };
        if meta.needs_reauth || !self.backend.supports_named_entries() {
            return Ok(None);
        }
        let Some(access_token) = self
            .backend
            .get(&access_key(&meta.user_id))
            .map_err(ZeppBridgeError::CredentialStore)?
        else {
            return Ok(None);
        };
        let refresh_token = self
            .backend
            .get(&refresh_key(&meta.user_id))
            .map_err(ZeppBridgeError::CredentialStore)?;
        Ok(Some(OfficialTokens {
            access_token,
            refresh_token,
            expires_at: meta.expires_at,
            user_id: meta.user_id,
        }))
    }

    /// Refresh refused: clear the tokens, keep the metadata and mark it "needs reauthorization", so the UI can explain what happened.
    pub(crate) fn mark_needs_reauth(&self) -> Result<()> {
        let Some(mut meta) = self.meta()? else {
            return Ok(());
        };
        self.delete_secrets(&meta.user_id)?;
        meta.needs_reauth = true;
        self.write_meta(&meta)
    }

    /// Deletes tokens and metadata. **Never touches any health data.** Production paths use [`Self::disconnect`].
    ///
    /// The metadata is the only source of "connected", so it is deleted even when the credential store refuses the delete. Before, an error here
    /// kept `official.json`, and people who could not answer the macOS keychain prompt could never disconnect (#145).
    /// The remaining token has no metadata pointing at it and is never read again; it returns the store's raw text.
    pub(crate) fn clear(&self) -> Result<Option<String>> {
        let left_behind = match self.meta()? {
            Some(meta) => self
                .delete_secrets(&meta.user_id)
                .err()
                .map(|e| e.to_string()),
            None => None,
        };
        match std::fs::remove_file(self.meta_path()) {
            Ok(()) => Ok(left_behind),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(left_behind),
            Err(error) => Err(error.into()),
        }
    }

    fn delete_secrets(&self, user_id: &str) -> Result<()> {
        if !self.backend.supports_named_entries() {
            return Ok(());
        }
        self.backend
            .delete(&access_key(user_id))
            .map_err(ZeppBridgeError::CredentialStore)?;
        self.backend
            .delete(&refresh_key(user_id))
            .map_err(ZeppBridgeError::CredentialStore)
    }
}
