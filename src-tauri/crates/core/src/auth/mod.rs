use crate::models::error::HeadlessProblem;

use crate::models::{error::Result, AuthInfo, ZeppBridgeError};

use chrono::Utc;

use serde::{Deserialize, Serialize};

use serde_json::Value;

use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::Arc,
};

mod backends;
mod select;

pub use backends::*;
pub use select::*;

/// The single service name used for the app token in the platform credential
/// store.  The user id is used as the credential account name so that an
/// account switch cannot accidentally read another account's token.
///
/// Windows, macOS and Linux all consume this now — Credential Manager, Keychain
/// and Secret Service respectively — so the `allow(dead_code)` that used to sit
/// here for the platforms without a real backend is gone, as its own comment
/// said it should be once one appeared.
#[cfg(feature = "store")]
pub const CREDENTIAL_SERVICE: &str = "com.zeppbridge.app.store";
#[cfg(not(feature = "store"))]
pub const CREDENTIAL_SERVICE: &str = "com.zeppbridge.app";

/// The maximum number of UTF-16 code units in a token.
///
/// `CRED_MAX_CREDENTIAL_BLOB_SIZE` in Windows Credential Manager is 2560 bytes. Credentials
/// are stored as UTF-16, so the limit is 1280 code units. This once allowed up to 16 KB, six times
/// more than can be stored: a token that was too long failed only at `CredWrite`, and the user saw only
/// "cannot write to Windows Credential Manager", with no hint about the length.
///
/// A real Zepp App Token has only a few dozen characters. Values that hit this limit are almost always a full JSON blob
/// taken from page storage. That is not a token, so it is better to catch it early than to fail on write.
pub const CREDENTIAL_MAX_UTF16_UNITS: usize = 1280;

const AUTH_FILE_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredAuth {
    #[serde(default = "default_version")]
    version: u32,
    user_id: String,
    region_host: String,
    #[serde(default)]
    updated_at: String,
}

fn default_version() -> u32 {
    AUTH_FILE_VERSION
}

/// Public, non-sensitive view of the saved authentication state.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuthStatus {
    pub configured: bool,
    /// Metadata survives a denied/missing platform credential, so recovery remains reachable.
    pub configuration_present: bool,
    pub credential_error: Option<String>,
    pub user_id: Option<String>,
    pub region_host: Option<String>,
    pub token_masked: Option<String>,
    pub version: Option<u32>,
    pub updated_at: Option<String>,
}

/// Authentication metadata and credential-store access.
pub struct AuthManager {
    auth_file: PathBuf,
    /// Best-effort copy of the user id kept beside `auth.json` so
    /// `clear_auth` can still remove the credential-store entry when the
    /// metadata file itself is unreadable/corrupt.
    user_id_file: PathBuf,
    // Injected backends stay fixed. Production resolves its explicit/default
    // choice at each operation so consented file storage also works immediately.
    credentials: Option<Arc<dyn CredentialBackend>>,
}

impl std::fmt::Debug for AuthManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuthManager")
            .field("auth_file", &self.auth_file)
            .finish_non_exhaustive()
    }
}

impl AuthManager {
    pub fn new(data_dir: PathBuf) -> Self {
        Self {
            auth_file: data_dir.join("auth.json"),
            user_id_file: data_dir.join("auth.user-id"),
            credentials: None,
        }
    }

    /// Construct an auth manager with an injected backend.  Production code
    /// uses the Windows Credential Manager backend; tests can supply an in
    /// memory backend and avoid changing the user's credentials.
    pub fn with_credential_backend(
        data_dir: PathBuf,
        credentials: Arc<dyn CredentialBackend>,
    ) -> Self {
        Self {
            auth_file: data_dir.join("auth.json"),
            user_id_file: data_dir.join("auth.user-id"),
            credentials: Some(credentials),
        }
    }

    fn credential_backend(&self) -> Arc<dyn CredentialBackend> {
        self.credentials.clone().unwrap_or_else(|| {
            default_credential_backend_in(self.auth_file.parent().expect("auth file has a parent"))
        })
    }

    /// Saves metadata atomically and stores the token only in the credential
    /// manager.  Inputs are trimmed and validated before either store is
    /// changed.
    pub fn save_auth(&self, auth: &AuthInfo) -> Result<()> {
        let credentials = self.credential_backend();
        let user_id = validate_user_id(&auth.user_id)?;
        let token = validate_token(&auth.app_token)?;
        let region_host = normalize_region_host(&auth.region_host)?;
        let previous_user_id = self.read_stored().ok().map(|(stored, _)| stored.user_id);
        // Reading an old Keychain entry may be denied while replacing it is
        // allowed. A fresh login must still get the chance to write its token.
        let previous = credentials.get(&user_id);

        credentials
            .set(&user_id, &token)
            .map_err(credential_error)?;

        let stored = StoredAuth {
            version: AUTH_FILE_VERSION,
            user_id: user_id.clone(),
            region_host,
            updated_at: Utc::now().to_rfc3339(),
        };

        if let Err(error) = self.write_stored(&stored) {
            // Best-effort rollback keeps metadata and the platform store
            // consistent if the atomic file replacement fails.
            // Do not delete the previous account's credential: the new id
            // never became current.
            match previous {
                Ok(Some(old)) => {
                    let _ = credentials.set(&user_id, &old);
                }
                Ok(None) => {
                    let _ = credentials.delete(&user_id);
                }
                Err(_) => {
                    // The previous value is unknown; do not delete an entry
                    // that may have existed before this recovery attempt.
                    tracing::warn!("Auth metadata save failed and the old credential is unreadable, so the credential replace cannot be rolled back");
                }
            }
            return Err(error);
        }

        // After an A to B account switch, A's keyring entry must not stay: the next clear knows only the current id.
        if let Some(raw) = previous_user_id {
            if let Ok(old_id) = validate_user_id(&raw) {
                if old_id != user_id {
                    let _ = credentials.delete(&old_id);
                }
            }
        }

        // The user-id hint is best-effort: a failure here must not roll back
        // an otherwise successful save, it only degrades the clear_auth
        // fallback path.
        let _ = self.write_user_id_hint(&user_id);

        Ok(())
    }

    /// Loads metadata and the token from the credential manager.  A metadata
    /// file without a credential is reported as an actionable auth error, not
    /// as a partially populated `AuthInfo`.
    pub fn load_auth(&self) -> Result<Option<AuthInfo>> {
        let credentials = self.credential_backend();
        if !self.auth_file.exists() {
            return Ok(None);
        }

        let (stored, legacy_token) = self.read_stored()?;
        let user_id = validate_user_id(&stored.user_id)?;
        let region_host = normalize_region_host(&stored.region_host)?;
        let token = match credentials.get(&user_id).map_err(credential_error)? {
            Some(value) => validate_token(&value)?,
            None => {
                if let Some(ref value) = legacy_token {
                    let value = validate_token(value)?;
                    credentials
                        .set(&user_id, &value)
                        .map_err(credential_error)?;
                    value
                } else {
                    // The most common cause of this is not "the credential is broken", but **someone copied the whole
                    // data folder from another machine** (issue #40): the database and
                    // auth.json are there, but the token was never in a file. It is in that machine's
                    // credential manager / keychain / Secret Service. So it is not enough to say
                    // "pair again". The message must say where to pair.
                    //
                    // Use `HeadlessProblem`, not `AuthError`, so it has its own
                    // error code: the CLI has no i18n layer and can show English only by code, and
                    // people who hit this are most likely on a headless Linux machine.
                    return Err(ZeppBridgeError::Headless(HeadlessProblem::TokenNotInStore));
                }
            }
        };

        // Older files lacked version/timestamp and could contain app_token.
        // Rewrite them after the credential has been safely copied to the
        // platform store, removing the legacy secret from disk.
        if stored.version != AUTH_FILE_VERSION
            || stored.updated_at.is_empty()
            || legacy_token.is_some()
            || stored.region_host != region_host
        {
            self.write_stored(&StoredAuth {
                version: AUTH_FILE_VERSION,
                user_id: user_id.clone(),
                region_host: region_host.clone(),
                updated_at: if stored.updated_at.is_empty() {
                    Utc::now().to_rfc3339()
                } else {
                    stored.updated_at
                },
            })?;
        }

        Ok(Some(AuthInfo {
            app_token: token,
            user_id,
            region_host,
        }))
    }

    /// Returns status without exposing the token.  The optional masked value
    /// is deliberately short and suitable for a settings screen.
    /// The user id stored in `auth.json`. Does not touch the credential store. `None` when not set or not readable.
    pub fn saved_user_id(&self) -> Option<String> {
        if !self.auth_file.exists() {
            return None;
        }
        let (stored, _) = self.read_stored().ok()?;
        validate_user_id(&stored.user_id).ok()
    }

    pub fn status(&self) -> Result<AuthStatus> {
        let credentials = self.credential_backend();
        if !self.auth_file.exists() {
            return Ok(AuthStatus {
                configured: false,
                configuration_present: false,
                credential_error: None,
                user_id: None,
                region_host: None,
                token_masked: None,
                version: None,
                updated_at: None,
            });
        }

        // Invalid metadata must not break the settings snapshot and hide the
        // very controls needed to replace or clear it.
        let recoverable = |error: ZeppBridgeError| AuthStatus {
            configured: false,
            configuration_present: true,
            credential_error: Some(error.user_message()),
            user_id: self
                .read_user_id_hint()
                .and_then(|id| validate_user_id(&id).ok()),
            region_host: None,
            token_masked: None,
            version: None,
            updated_at: None,
        };
        let (stored, legacy_token) = match self.read_stored() {
            Ok(stored) => stored,
            Err(error) => return Ok(recoverable(error)),
        };
        let user_id = match validate_user_id(&stored.user_id) {
            Ok(id) => id,
            Err(error) => return Ok(recoverable(error)),
        };
        let region_host = match normalize_region_host(&stored.region_host) {
            Ok(host) => host,
            Err(error) => return Ok(recoverable(error)),
        };
        let token = credentials
            .get(&user_id)
            .map_err(credential_error)
            .and_then(|token| {
                token
                    .or(legacy_token)
                    .ok_or(ZeppBridgeError::Headless(HeadlessProblem::TokenNotInStore))
            })
            .and_then(|token| validate_token(&token));
        let (token, error) = match token {
            Ok(token) => (Some(token), None),
            Err(error) => (None, Some(error.user_message())),
        };

        Ok(AuthStatus {
            configured: token.is_some(),
            configuration_present: true,
            credential_error: error,
            user_id: Some(user_id),
            region_host: Some(region_host),
            token_masked: token.as_deref().map(mask_token),
            version: Some(stored.version),
            updated_at: (!stored.updated_at.is_empty()).then_some(stored.updated_at),
        })
    }

    #[cfg(test)]
    pub fn masked_token(&self) -> Result<Option<String>> {
        Ok(self.status()?.token_masked)
    }

    /// Removes both metadata and the corresponding credential-store entry.
    /// A corrupt `auth.json` no longer leaves the credential behind: the
    /// best-effort user-id hint file is consulted as a fallback.
    ///
    /// When the credential store refuses to delete (on macOS an ad-hoc signed new build cannot read the old keychain entry, and the
    /// user cannot answer that auth prompt, #145), **sign out anyway**: local metadata is still deleted, the remaining entry is never
    /// read again, and signing in again with the same account overwrites it. This once returned an error and kept `auth.json`,
    /// so the user could neither sign out nor sign in. The store's own error goes back to the caller for the log.
    pub fn clear_auth(&self) -> Result<Option<String>> {
        let user_id = self
            .read_stored()
            .ok()
            .map(|(stored, _)| stored.user_id)
            .and_then(|id| validate_user_id(&id).ok())
            .or_else(|| {
                self.read_user_id_hint()
                    .and_then(|id| validate_user_id(&id).ok())
            });

        let credentials = self.credential_backend();
        let mut left_behind = None;
        if let Some(user_id) = user_id {
            left_behind = credentials.delete(&user_id).err();
        }
        if self.auth_file.exists() {
            fs::remove_file(&self.auth_file)?;
        }
        if self.user_id_file.exists() {
            let _ = fs::remove_file(&self.user_id_file);
        }
        Ok(left_behind)
    }

    fn write_user_id_hint(&self, user_id: &str) -> Result<()> {
        let parent = self
            .user_id_file
            .parent()
            .ok_or_else(|| ZeppBridgeError::ConfigError("Invalid auth directory".into()))?;
        fs::create_dir_all(parent)?;
        let temp_path = parent.join(format!(
            ".auth.user-id.tmp-{}-{}",
            std::process::id(),
            crate::unique_suffix()
        ));
        let result = (|| -> io::Result<()> {
            let mut file = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&temp_path)?;
            file.write_all(user_id.as_bytes())?;
            file.sync_all()?;
            drop(file);
            replace_file(&temp_path, &self.user_id_file)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temp_path);
        }
        result.map_err(ZeppBridgeError::IoError)
    }

    fn read_user_id_hint(&self) -> Option<String> {
        let content = fs::read_to_string(&self.user_id_file).ok()?;
        let trimmed = content.trim().to_string();
        (!trimmed.is_empty()).then_some(trimmed)
    }

    fn read_stored(&self) -> Result<(StoredAuth, Option<String>)> {
        let content = fs::read_to_string(&self.auth_file)?;
        let value: Value = serde_json::from_str(&content)
            .map_err(|e| ZeppBridgeError::ParseError(format!("Invalid auth metadata format: {e}")))?;
        let legacy_token = value
            .get("app_token")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        let stored: StoredAuth = serde_json::from_value(value)
            .map_err(|e| ZeppBridgeError::ParseError(format!("Invalid auth metadata field: {e}")))?;
        Ok((stored, legacy_token))
    }

    fn write_stored(&self, stored: &StoredAuth) -> Result<()> {
        let parent = self
            .auth_file
            .parent()
            .ok_or_else(|| ZeppBridgeError::ConfigError("Invalid auth directory".to_string()))?;
        fs::create_dir_all(parent)?;

        let json = serde_json::to_vec_pretty(stored)
            .map_err(|e| ZeppBridgeError::ParseError(e.to_string()))?;
        let suffix = crate::unique_suffix();
        let temp_path = parent.join(format!(
            ".{}.tmp-{}-{}",
            self.auth_file
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("auth.json"),
            std::process::id(),
            suffix
        ));

        let result = (|| -> io::Result<()> {
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(&temp_path)?;
            file.write_all(&json)?;
            file.sync_all()?;
            drop(file);
            replace_file(&temp_path, &self.auth_file)
        })();

        if result.is_err() {
            let _ = fs::remove_file(&temp_path);
        }
        result.map_err(ZeppBridgeError::IoError)
    }
}

#[cfg(test)]
mod credential_error_tests {
    use super::*;

    #[test]
    fn secret_service_absence_stays_headless_not_generic_store() {
        let error = credential_error(format!("{HEADLESS_NO_STORE_MARKER}cannot talk to dbus"));
        assert!(matches!(
            error,
            ZeppBridgeError::Headless(HeadlessProblem::NoCredentialStore { .. })
        ));
        assert_eq!(error.code(), "err.headless.no_credential_store");
        let generic = credential_error("cannot write to Windows Credential Manager".into());
        assert!(matches!(generic, ZeppBridgeError::CredentialStore(_)));
        assert_eq!(generic.code(), "err.core.credential_store");
    }
}

#[cfg(test)]
mod tests;
