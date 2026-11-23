//! Choose the credential backend per platform, and validate token / user id / regional host (split out of auth/mod.rs, same logic).

use super::*;

/// The platform's default credential store.
///
/// macOS and Linux can explicitly opt into a file store when the system
/// store is inaccessible. Reuse an existing credential file after a restart,
/// but never fall back to a file in response to a system-store error.
pub fn default_credential_backend_in(data_dir: &Path) -> Arc<dyn CredentialBackend> {
    #[cfg(windows)]
    {
        let _ = data_dir;
        Arc::new(WindowsCredentialBackend)
    }
    #[cfg(target_os = "macos")]
    {
        macos_credential_backend(
            data_dir,
            std::env::var(CREDENTIAL_STORE_ENV).ok().as_deref(),
        )
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        linux_credential_backend(data_dir)
    }
    #[cfg(all(not(windows), not(unix)))]
    {
        let _ = data_dir;
        Arc::new(UnavailableCredentialBackend)
    }
}

/// Resolve the macOS choice without accessing Keychain or changing process
/// environment, so the opt-in and restart rules can be tested on every host.
#[cfg(any(target_os = "macos", test))]
pub(super) fn macos_file_store_selected(
    requested: Option<&str>,
    credential_file_exists: bool,
) -> std::result::Result<bool, String> {
    match requested.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) if value.eq_ignore_ascii_case("file") => Ok(true),
        Some(value)
            if value.eq_ignore_ascii_case("keychain") || value.eq_ignore_ascii_case("keyring") =>
        {
            Ok(false)
        }
        Some(value) => Err(value.to_string()),
        // This fork keeps credentials in the 0600 file store by default on macOS:
        // ad-hoc and self-signed builds otherwise trigger a Keychain prompt on every rebuild.
        None => {
            let _ = credential_file_exists;
            Ok(true)
        }
    }
}

#[cfg(target_os = "macos")]
pub(super) fn macos_credential_backend(
    data_dir: &Path,
    requested: Option<&str>,
) -> Arc<dyn CredentialBackend> {
    match macos_file_store_selected(requested, data_dir.join(CREDENTIAL_FILE).is_file()) {
        Ok(true) => Arc::new(FileCredentialBackend::new(data_dir)),
        Ok(false) => Arc::new(MacOsCredentialBackend),
        Err(value) => Arc::new(InvalidCredentialStoreBackend { value }),
    }
}

/// On Linux, choose a store from the environment and the current state.
#[cfg(all(unix, not(target_os = "macos")))]
pub(super) fn linux_credential_backend(data_dir: &Path) -> Arc<dyn CredentialBackend> {
    let requested = std::env::var(CREDENTIAL_STORE_ENV)
        .ok()
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty());

    match requested.as_deref() {
        Some("secret-service") | Some("secretservice") | Some("keyring") => {
            Arc::new(SecretServiceCredentialBackend)
        }
        Some("file") => Arc::new(FileCredentialBackend::new(data_dir)),
        Some("env") => Arc::new(EnvCredentialBackend),
        Some(other) => Arc::new(InvalidCredentialStoreBackend {
            value: other.to_string(),
        }),
        // With no explicit choice, infer from "what already exists on this machine" instead of
        // always assuming a desktop:
        //
        // 1. A token in the environment: a clear signal. The deployer just gave the token
        //    to this process.
        // 2. A credential file in the data directory: the last sign-in used the file store. If it is
        //    ignored, a second run without the env var becomes "you are not signed in".
        // 3. Neither: use Secret Service. On a desktop this is correct. Off a desktop,
        //    the error names the other two options.
        None if EnvCredentialBackend::token().is_some() => Arc::new(EnvCredentialBackend),
        None if data_dir.join(CREDENTIAL_FILE).is_file() => {
            Arc::new(FileCredentialBackend::new(data_dir))
        }
        None => Arc::new(SecretServiceCredentialBackend),
    }
}

/// Linux Secret Service uses this prefix when "the machine has no keyring", so that
/// [`credential_error`] turns it back into [`HeadlessProblem`] and does not fold it into
/// the generic `CredentialStore` (which would give the CLI exit code 1 and the raw text).
pub(super) const HEADLESS_NO_STORE_MARKER: &str = "\u{1e}headless.no_credential_store\u{1e}";

pub(super) fn credential_error(error: String) -> ZeppBridgeError {
    // Backends are not allowed to include secret values in their error text.
    //
    // This is "the system credential store does not cooperate", not "the auth data is wrong". Keeping them apart lets the UI
    // give the matching advice: one tells the user to reconnect, the other to check the credential manager.
    if let Some(detail) = error.strip_prefix(HEADLESS_NO_STORE_MARKER) {
        return ZeppBridgeError::Headless(HeadlessProblem::NoCredentialStore {
            detail: detail.to_string(),
        });
    }
    ZeppBridgeError::CredentialStore(error)
}

pub(super) fn validate_token(raw: &str) -> Result<String> {
    let value = raw.trim();
    if value.is_empty() || value.chars().any(char::is_control) {
        return Err(ZeppBridgeError::AuthError("Token is empty or has an invalid format".to_string()));
    }
    if value.encode_utf16().count() > CREDENTIAL_MAX_UTF16_UNITS {
        return Err(ZeppBridgeError::CredentialStore(format!(
            "Token has {} characters, more than the {CREDENTIAL_MAX_UTF16_UNITS} the system credential manager can store. \
             This most likely means the value read is not the App Token itself",
            value.chars().count()
        )));
    }
    Ok(value.to_string())
}

pub(super) fn validate_user_id(raw: &str) -> Result<String> {
    let value = raw.trim();
    if value.is_empty()
        || value.len() > 256
        || value.chars().any(char::is_control)
        || value.contains('/')
    {
        return Err(ZeppBridgeError::AuthError(
            "User ID is empty or has an invalid format".to_string(),
        ));
    }
    Ok(value.to_string())
}

/// Normalize and validate the region host.  Auth metadata only stores an
/// HTTPS origin (no path, query, fragment, or userinfo).
pub fn normalize_region_host(raw: &str) -> Result<String> {
    let value = raw.trim();
    let parsed = reqwest::Url::parse(value)
        .map_err(|_| ZeppBridgeError::AuthError("Invalid regional host address".to_string()))?;
    if parsed.scheme() != "https"
        || parsed.host_str().is_none()
        || parsed.username() != ""
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
        || (!parsed.path().is_empty() && parsed.path() != "/")
    {
        return Err(ZeppBridgeError::AuthError(
            "Regional host must be an https://host address".to_string(),
        ));
    }

    let host = parsed
        .host_str()
        .ok_or_else(|| ZeppBridgeError::AuthError("Invalid regional host address".to_string()))?
        .to_ascii_lowercase();
    // The address stored in the credential file is later passed to `connectors::zepp::validate_region_host`
    // to open the real connection. That check allows no port and accepts only `api-mifit*.zepp.com` /
    // `api-mifit*.huami.com`. If this check were looser, you would get a dead end: "sign-in saves fine,
    // status shows connected, but every sync is rejected at the connection layer", and only clearing
    // the credentials and signing in again gets out of it. The two rules must match.
    if parsed.port().is_some() {
        return Err(ZeppBridgeError::AuthError(
            "Regional host must not include a port".to_string(),
        ));
    }
    let valid_zepp = host.starts_with("api-mifit") && host.ends_with(".zepp.com");
    let valid_huami = host.starts_with("api-mifit") && host.ends_with(".huami.com");
    if !(valid_zepp || valid_huami) {
        return Err(ZeppBridgeError::AuthError(
            "Regional host must be api-mifit*.zepp.com or api-mifit*.huami.com".to_string(),
        ));
    }
    Ok(format!("https://{host}"))
}

pub fn mask_token(token: &str) -> String {
    let chars: Vec<char> = token.chars().collect();
    if chars.len() <= 4 {
        return "••••".to_string();
    }
    let prefix: String = chars.iter().take(2).collect();
    let suffix: String = chars
        .iter()
        .rev()
        .take(2)
        .copied()
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("{prefix}…{suffix}")
}

pub(super) fn replace_file(temp: &Path, destination: &Path) -> io::Result<()> {
    // Windows `std::fs::rename` already replaces (`MoveFileExW` +
    // `MOVEFILE_REPLACE_EXISTING`). Removing the destination first is not
    // atomic: a crash in between leaves the target missing.
    fs::rename(temp, destination)
}
