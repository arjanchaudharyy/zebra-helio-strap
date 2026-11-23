use super::status::build_app_status;
use crate::app_state::AppState;
use crate::connectors::ZeppConnector;
use crate::ipc_error::AppError;
use crate::ipc_types::AppStatus;
use crate::models::{error::ZeppBridgeError, AuthInfo};
use chrono::{Duration, Utc};
use serde_json::Value;

/// Save authentication metadata and install a ready-to-use synchronizer.
///
/// The credential itself is accepted only by `AuthManager`; this command
/// never includes it in a status value or an error message.  Building the
/// synchronizer opens a separate database connection, so the command-side
/// database lock is not held while doing setup.
pub async fn save_auth(
    state: tauri::State<'_, AppState>,
    app_token: String,
    user_id: String,
    region_host: String,
) -> std::result::Result<AppStatus, AppError> {
    let epoch = state
        .login
        .epoch
        .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        + 1;
    super::login::recovery::clear(&state);
    *state.login.status.write().await = crate::ipc_types::LoginStatus::idle();
    let auth = AuthInfo {
        app_token,
        user_id,
        region_host,
    };

    // Cancel first so a running start_*_sync can drop this lock; then hold it
    // across save + manager swap so the old handle cannot keep writing.
    let _command_guard = state.lock_sync_commands().await;

    // If the database belongs to a different account, do not save this token.
    super::claim_library_for_login(&state.data_dir, &auth.user_id).await?;
    if state.login.epoch.load(std::sync::atomic::Ordering::SeqCst) != epoch {
        return Err(AppError::new("err.login.cancelled", "Login cancelled"));
    }
    if let Err(error) = state.auth.save_auth(&auth) {
        if super::login::recovery::is_store_error(&error) {
            super::login::recovery::remember(&state, auth, epoch, None);
            if let Some(status) = super::login::recovery::status(&state) {
                *state.login.status.write().await = status;
            }
        }
        return Err(error.into());
    }
    install_saved_auth(&state, auth).await
}

pub(super) async fn install_saved_auth(
    state: &AppState,
    auth: AuthInfo,
) -> Result<AppStatus, AppError> {
    let manager = match AppState::build_sync_manager(auth, &state.data_dir) {
        Ok(manager) => manager,
        Err(error) => {
            // Metadata and the credential store must not be left configured
            // when the connector/database cannot be initialized.  Clearing is
            // deliberately best effort; the original setup error is the
            // actionable result returned to the caller.
            let message = error.user_message();
            let _ = state.auth.clear_auth();
            state.replace_sync_manager(None).await;
            {
                let mut auth_state = state.auth_state.write().await;
                *auth_state = "unconfigured".to_string();
            }
            let failure = AppError::new(
                "err.auth.sync_init_failed",
                format!("Cannot start sync. Check the auth region and try again: {message}"),
            );
            {
                let mut warning = state.auth_warning.write().await;
                *warning = Some(failure.message.clone());
            }
            return Err(failure);
        }
    };

    state.replace_sync_manager(Some(manager)).await;
    {
        let mut auth_state = state.auth_state.write().await;
        *auth_state = "configured".to_string();
    }
    {
        let mut warning = state.startup_warning.write().await;
        *warning = None;
    }
    {
        let mut warning = state.auth_warning.write().await;
        *warning = None;
    }

    build_app_status(state).await
}

/// Verify the saved credential with a real, bounded recent heart-rate query.
///
/// A structured empty response is still a successful authentication check:
/// the account may simply have no heart-rate samples in the two-hour window.
/// Scalars, malformed payloads, and explicit non-success response codes are
/// rejected so that an HTML/error body cannot be reported as verified.
#[tauri::command]
pub async fn verify_auth(
    state: tauri::State<'_, AppState>,
) -> std::result::Result<AppStatus, AppError> {
    let auth = match state.auth.load_auth() {
        Ok(Some(auth)) => auth,
        Ok(None) => {
            return verify_failure(
                &state,
                ZeppBridgeError::AuthError("Auth is not configured yet".to_string()),
            )
            .await
        }
        Err(error) => return verify_failure(&state, error).await,
    };

    if let Err(error) = verify_recent_heart_rate(&auth).await {
        return verify_failure(&state, error).await;
    }

    let _command_guard = state.lock_sync_commands().await;

    let manager = match AppState::build_sync_manager(auth, &state.data_dir) {
        Ok(manager) => manager,
        Err(error) => return verify_failure(&state, error).await,
    };

    state.replace_sync_manager(Some(manager)).await;
    {
        let mut auth_state = state.auth_state.write().await;
        *auth_state = "verified".to_string();
    }
    {
        let mut warning = state.startup_warning.write().await;
        *warning = None;
    }
    {
        let mut warning = state.auth_warning.write().await;
        *warning = None;
    }

    build_app_status(&state).await
}

/// Remove credentials and reset only the in-memory authentication/sync
/// state.  The SQLite database is intentionally retained so clearing an
/// account cannot erase historical health data.
#[tauri::command]
pub async fn clear_auth(
    state: tauri::State<'_, AppState>,
) -> std::result::Result<AppStatus, AppError> {
    state
        .login
        .epoch
        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    super::login::recovery::clear(&state);
    {
        let mut login = state.login.status.write().await;
        *login = crate::ipc_types::LoginStatus::idle();
    }

    let _command_guard = state.lock_sync_commands().await;
    state.replace_sync_manager(None).await;

    if let Some(left_behind) = state.auth.clear_auth()? {
        crate::diagnostics::log(&format!(
            "Sign out: local data cleared, but the old entry in the system credential store could not be deleted ({left_behind})"
        ));
    }
    {
        let mut warning = state.auth_warning.write().await;
        *warning = None;
    }
    {
        let mut auth_state = state.auth_state.write().await;
        *auth_state = "unconfigured".to_string();
    }
    {
        let mut warning = state.startup_warning.write().await;
        *warning = None;
    }
    {
        let mut region = state.region_confidence.write().await;
        *region = "unknown".to_string();
    }

    build_app_status(&state).await
}

/// Start over with a different account: delete the tokens of both channels, queue a move of the whole data folder, then restart.
///
/// The rename must happen after the restart and before any file opens (`paths::fresh_start`). Old data is not deleted:
/// it moves to `data-before-<time>`. We do not revoke the official authorization: the user may want to authorize the same account again.
#[tauri::command]
pub async fn start_fresh_library(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> std::result::Result<(), AppError> {
    state
        .login
        .epoch
        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    zeppbridge_core::official::invalidate_pending_logins();
    let _command_guard = state.lock_sync_commands().await;
    state.replace_sync_manager(None).await;

    if let Ok(Some(left_behind)) = state.auth.clear_auth() {
        crate::diagnostics::log(&format!(
            "Fresh start: the old channel token could not be deleted from the credential store ({left_behind})"
        ));
    }
    let official = zeppbridge_core::official::OfficialStore::new(&state.data_dir);
    if let Ok(Some(left_behind)) = official.disconnect() {
        crate::diagnostics::log(&format!(
            "Fresh start: the official token could not be deleted from the credential store ({left_behind})"
        ));
    }
    crate::paths::request_fresh_start(&state.data_dir).map_err(|error| {
        AppError::new(
            "err.auth.fresh_start_failed",
            format!("Could not prepare a new data folder: {error}"),
        )
    })?;
    crate::diagnostics::log("Fresh start: data folder move queued, restarting");
    app.request_restart();
    Ok(())
}

/// Verify a credential with the same bounded recent heart-rate query used by
/// `verify_auth`.  The request is made without holding application locks.
pub(crate) async fn verify_recent_heart_rate(
    auth: &AuthInfo,
) -> std::result::Result<(), ZeppBridgeError> {
    let connector = ZeppConnector::new(auth.clone())?;
    let end = Utc::now();
    let start = end - Duration::hours(2);
    let payload = connector
        .fetch_heart_rate(start.timestamp(), end.timestamp())
        .await?;
    validate_verify_payload(&payload)
}

/// Validate only the response shape needed to establish authentication.  An
/// object/array may legitimately be empty (no recent samples), while an
/// object carrying a response code must explicitly report a known success
/// value.
pub(crate) fn validate_verify_payload(value: &Value) -> std::result::Result<(), ZeppBridgeError> {
    let object = match value {
        Value::Array(_) => return Ok(()),
        Value::Object(object) => object,
        _ => {
            return Err(ZeppBridgeError::ParseError(
                "Auth verification returned an empty or invalid JSON structure".to_string(),
            ))
        }
    };

    let Some(code) = object.get("code") else {
        return Ok(());
    };

    let success = match code {
        Value::Number(number) => matches!(number.as_i64(), Some(0) | Some(1) | Some(200)),
        // Some regional API versions serialize numeric response codes as
        // strings.  Accept only the same three exact success values.
        Value::String(code) => matches!(code.trim(), "0" | "1" | "200"),
        _ => false,
    };

    if success {
        Ok(())
    } else {
        Err(ZeppBridgeError::ParseError(
            "Auth verification returned a failure response code".to_string(),
        ))
    }
}

/// Strength of the region evidence at login.
///
/// "Are these credentials still valid" and "is this host the region of this account" are two questions. Before,
/// one check answered both. `validate_verify_payload` answers the first, and there "empty" is valid: the user
/// may not have worn the watch in the last two hours. The second question cannot accept that: a region that does
/// not know this account also returns a structured empty response, and because it does not look up data, it often
/// answers faster. So in a race of about ten candidate regions, the first answer was often the wrong one and was
/// saved as `region_host`. Every later sync went there: the UI showed "connected", and the database had no
/// records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum RegionEvidence {
    /// The request worked, but nothing proves that this host knows this account.
    Empty,
    /// This host returned the devices bound to this account for `user_id`: it knows this person.
    Identified,
}

/// Decide if a region host is the region of this account.
///
/// Use `/users/{user_id}/devices`: the path contains `user_id`, so "correct region but no data in the last two
/// hours" and "this region does not know this user" no longer return the same thing. Every
/// ZeppBridge user has bound a watch, so devices are strong evidence.
///
/// If the device endpoint fails for a reason other than auth, fall back to the same heart rate check as `verify_auth`,
/// and record the result as weak evidence. It is better to use the old check than to make a login path that never
/// failed before fail because of a new endpoint.
pub(crate) async fn probe_region_evidence(
    auth: &AuthInfo,
) -> std::result::Result<RegionEvidence, ZeppBridgeError> {
    let connector = ZeppConnector::new(auth.clone())?;
    match connector.fetch_devices().await {
        Ok(payload) => {
            if super::data::parse_device_profiles(&payload).is_empty() {
                Ok(RegionEvidence::Empty)
            } else {
                Ok(RegionEvidence::Identified)
            }
        }
        // A clear credential rejection is a result. Do not ask a second endpoint.
        Err(error @ ZeppBridgeError::NeedsReauth(_)) => Err(error),
        Err(_) => verify_recent_heart_rate(auth)
            .await
            .map(|()| RegionEvidence::Empty),
    }
}

/// Record a verification warning and return a Chinese, token-free error.
/// Only an explicit `NeedsReauth` error changes the auth state; transient
/// transport/format failures leave the saved configuration available for a
/// retry while still surfacing the warning in the status stream.
async fn verify_failure(
    state: &AppState,
    error: ZeppBridgeError,
) -> std::result::Result<AppStatus, AppError> {
    let failure = user_facing_verify_error(&error);
    if error.needs_reauth() {
        let mut auth_state = state.auth_state.write().await;
        *auth_state = "needs_reauth".to_string();
    }
    {
        let mut warning = state.auth_warning.write().await;
        *warning = Some(failure.message.clone());
    }
    Err(failure)
}

/// Verification failures have three types, each with its own next step: for network problems, retry; for invalid
/// credentials, reconnect; the rest are "unclear". The previous version merged them into one message that English UI users could not read.
fn user_facing_verify_error(error: &ZeppBridgeError) -> AppError {
    match error {
        ZeppBridgeError::NetworkError(_) => AppError::new(
            "err.auth.verify_network",
            "Auth verification failed: cannot reach the Zepp service. Check your network and try again.",
        ),
        ZeppBridgeError::NeedsReauth(_) => AppError::new(
            "err.auth.verify_needs_reauth",
            "Auth verification failed: auth is no longer valid. Save the auth info again.",
        ),
        _ => AppError::new(
            "err.auth.verify_failed",
            format!("Auth verification failed: {}", error.user_message()),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn verify_payload_accepts_empty_structured_success() {
        for value in [
            json!([]),
            json!({}),
            json!({"code": 0}),
            json!({"code": "200"}),
        ] {
            assert!(validate_verify_payload(&value).is_ok(), "rejected {value}");
        }
    }

    #[test]
    fn verify_payload_rejects_scalars_and_failure_codes() {
        for value in [json!(null), json!("ok"), json!(42), json!({"code": 500})] {
            assert!(validate_verify_payload(&value).is_err(), "accepted {value}");
        }
    }
}

/// Manually enter authentication credentials.
///
/// This is a convenience wrapper around `save_auth` that accepts the same
/// three parameters. The frontend can use this for a manual entry form or
/// call `save_auth` directly.
#[tauri::command]
pub async fn manual_auth(
    state: tauri::State<'_, AppState>,
    app_token: String,
    user_id: String,
    region_host: String,
) -> std::result::Result<AppStatus, AppError> {
    save_auth(state, app_token, user_id, region_host).await
}
