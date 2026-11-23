//! Settings > Data > "Push to your own server".
//!
//! Push itself is in core (`zeppbridge_core::push`): after a sync, one push round runs in the background. With no enabled target, it does nothing.
//! This module gives the UI five actions: read status, set site and connection code, take over from the local ZeppBridge Cloud component, push now, remove.
//! Keys go only to the system credential store. The UI only learns if a key exists. Errors carry only `err.push.*` codes.

use crate::{app_state::AppState, ipc_error::AppError};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use zeppbridge_core::auth::default_credential_backend_in;
use zeppbridge_core::push::{self, client, setup, PushError};

/// Run only one push round at a time (the user can click "Push now" many times).
static RUNNING: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Serialize, Default)]
pub struct PushState {
    /// A site is set (there is one PA target).
    pub configured: bool,
    pub enabled: bool,
    /// The value as the user typed it.
    pub site: Option<String>,
    /// The normalized site URL (`https://health.example.com`).
    pub site_url: Option<String>,
    /// The credential store has a key that can be decrypted.
    pub has_secrets: bool,
    /// Can the credential store save by name? (The environment variable backend cannot.)
    pub credential_store_ok: bool,
    pub last: Option<setup::LastPush>,
    pub running: bool,
    /// The code when the config file cannot be read.
    pub config_error_code: Option<String>,
    /// The site URL of the local ZeppBridge Cloud component, if it is installed and can be taken over (it has a site and an upload key in Credential Manager).
    pub companion_site_url: Option<String>,
    /// The component's scheduled tasks still exist: after the takeover both sides upload a copy, so the UI tells the user to delete them.
    pub companion_tasks: bool,
}

fn push_app_error(error: PushError) -> AppError {
    let message = match error.code {
        "err.push.site_invalid" => {
            "Invalid site URL: enter only a domain (for example example.com) or a full https URL"
        }
        "err.push.connection_code_invalid" => "Invalid connection code: paste the full code, it contains dk and writeKey",
        "err.push.pa_write_key_malformed" => "The write key in the connection code has a bad format",
        "err.push.pa_data_key_malformed" => "The data key in the connection code has a bad format",
        "err.push.pa_tenant_mismatch" => "The tenant in the connection code does not match the write key",
        "err.push.credential_store_unnamed" => "The current credential store cannot save the push key",
        "err.push.credential_store_failed" => "Could not save the push key in the system credential store",
        "err.push.not_configured" => "No push target is set yet",
        "err.push.companion_not_found" => "No ZeppBridge Cloud settings to take over were found on this computer",
        "err.push.config_has_secret" => "The push config has a field that looks like a key, so it was rejected",
        "err.push.state_unwritable" => "Cannot write the push config to the data folder",
        _ => "Cannot read the push config",
    };
    AppError::new(error.code, message)
}

/// The component's site (`https://health.example.com`) and upload key. None if not installed, not configured or no key.
fn companion() -> Option<(String, String)> {
    let site_url = super::cloud_companion::companion_site_url()?;
    let host = site_url
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(&site_url)
        .split('/')
        .next()?
        .to_string();
    let secret = setup::companion_upload_secret(&host)?;
    setup::parse_connection_code(&secret).ok()?;
    Some((site_url, secret))
}

fn collect(data_dir: &Path) -> PushState {
    let credentials = default_credential_backend_in(data_dir);
    let mut state = PushState {
        credential_store_ok: credentials.supports_named_entries(),
        running: RUNNING.load(Ordering::SeqCst),
        ..PushState::default()
    };
    match push::load_targets(data_dir) {
        Ok(targets) => {
            if let Some(target) = targets.iter().find(|t| t.id == setup::PA_TARGET_ID) {
                state.configured = true;
                state.enabled = target.enabled;
                state.site_url = client::normalize_pa_site(&target.site, target.dev_loopback)
                    .ok()
                    .map(|site| site.base);
                state.site = Some(target.site.clone());
                state.has_secrets = setup::has_secrets(credentials.as_ref(), &target.id);
                state.last = setup::last_push(data_dir, &target.id);
            }
        }
        Err(error) => state.config_error_code = Some(error.code.to_string()),
    }
    if let Some((url, _)) = companion() {
        // If the same site was already taken over, do not suggest it again.
        if state.site_url.as_deref() != Some(url.as_str()) || !state.has_secrets {
            state.companion_site_url = Some(url);
        }
    }
    state.companion_tasks = super::cloud_companion::companion_tasks_present();
    state
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> Result<T, AppError> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|_| AppError::new("err.push.state_unwritable", "Cannot write the push config to the data folder"))
}

#[derive(Debug, Clone, Deserialize)]
pub struct PushSaveInput {
    pub site: String,
    /// Empty = keep the key (change only the URL).
    pub connection_code: Option<String>,
    pub enabled: bool,
}

#[tauri::command]
pub async fn push_state(state: tauri::State<'_, AppState>) -> Result<PushState, AppError> {
    let dir = state.data_dir.clone();
    blocking(move || collect(&dir)).await
}

#[tauri::command]
pub async fn push_save(
    state: tauri::State<'_, AppState>,
    input: PushSaveInput,
) -> Result<PushState, AppError> {
    let dir = state.data_dir.clone();
    blocking(move || -> Result<PushState, AppError> {
        let credentials = default_credential_backend_in(&dir);
        // Validate both first, then save: a bad URL must not leave half a config.
        client::normalize_pa_site(&input.site, false).map_err(|code| {
            push_app_error(PushError {
                code: "err.push.site_invalid",
                detail: code.into(),
            })
        })?;
        match input.connection_code.as_deref().map(str::trim) {
            Some(code) if !code.is_empty() => {
                setup::save_connection_code(credentials.as_ref(), setup::PA_TARGET_ID, code)
                    .map_err(push_app_error)?;
            }
            _ if !setup::has_secrets(credentials.as_ref(), setup::PA_TARGET_ID) => {
                return Err(push_app_error(PushError {
                    code: "err.push.connection_code_invalid",
                    detail: String::new(),
                }));
            }
            _ => {}
        }
        setup::upsert_pa_target(&dir, &input.site, input.enabled).map_err(push_app_error)?;
        Ok(collect(&dir))
    })
    .await?
}

#[tauri::command]
pub async fn push_set_enabled(
    state: tauri::State<'_, AppState>,
    enabled: bool,
) -> Result<PushState, AppError> {
    let dir = state.data_dir.clone();
    blocking(move || -> Result<PushState, AppError> {
        setup::set_enabled(&dir, setup::PA_TARGET_ID, enabled).map_err(push_app_error)?;
        Ok(collect(&dir))
    })
    .await?
}

/// Take over the site and upload key from the local ZeppBridge Cloud component (only reads the component data, deletes nothing).
#[tauri::command]
pub async fn push_import_companion(
    state: tauri::State<'_, AppState>,
) -> Result<PushState, AppError> {
    let dir = state.data_dir.clone();
    blocking(move || -> Result<PushState, AppError> {
        let (site_url, secret) = companion().ok_or_else(|| {
            push_app_error(PushError {
                code: "err.push.companion_not_found",
                detail: String::new(),
            })
        })?;
        let credentials = default_credential_backend_in(&dir);
        setup::save_connection_code(credentials.as_ref(), setup::PA_TARGET_ID, &secret)
            .map_err(push_app_error)?;
        setup::upsert_pa_target(&dir, &site_url, true).map_err(push_app_error)?;
        Ok(collect(&dir))
    })
    .await?
}

/// Push one round now (do not wait for the next sync). Push failures go into the result, not a command failure.
#[tauri::command]
pub async fn push_run(state: tauri::State<'_, AppState>) -> Result<PushState, AppError> {
    let dir: PathBuf = state.data_dir.clone();
    if RUNNING
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return blocking(move || collect(&dir)).await;
    }
    struct Release;
    impl Drop for Release {
        fn drop(&mut self) {
            RUNNING.store(false, Ordering::SeqCst);
        }
    }
    let _release = Release;
    let credentials = default_credential_backend_in(&dir);
    let _ = push::run(&dir, credentials).await;
    drop(_release);
    blocking(move || collect(&dir)).await
}

/// Remove the push target: config, key, manifest and last result. Data on the site does not change.
#[tauri::command]
pub async fn push_remove(state: tauri::State<'_, AppState>) -> Result<PushState, AppError> {
    let dir = state.data_dir.clone();
    blocking(move || -> Result<PushState, AppError> {
        let credentials = default_credential_backend_in(&dir);
        setup::remove_target(&dir, credentials.as_ref(), setup::PA_TARGET_ID)
            .map_err(push_app_error)?;
        Ok(collect(&dir))
    })
    .await?
}
