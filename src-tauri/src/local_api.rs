//! Tauri adapter for the local API.
//!
//! Lifecycle, auth, parse limits and data reads are in `zeppbridge_core::local_api`.
//! This file only exposes the controller's live state and three user actions as commands.

use crate::ipc_error::AppError;
use std::sync::Arc;
use zeppbridge_core::local_api::{LocalApiController, LocalApiStatus};

/// The controller handle that Tauri manages.
pub struct LocalApi(pub Arc<LocalApiController>);

#[tauri::command]
pub fn get_local_api_status(state: tauri::State<'_, LocalApi>) -> LocalApiStatus {
    state.0.status()
}

#[tauri::command]
pub fn set_local_api_enabled(
    state: tauri::State<'_, LocalApi>,
    enabled: bool,
) -> Result<LocalApiStatus, AppError> {
    Ok(state.0.set_enabled(enabled))
}

/// The UI masks the token by default. This runs only when the user clicks "Show" or "Copy".
#[tauri::command]
pub fn reveal_local_api_token(state: tauri::State<'_, LocalApi>) -> Result<String, AppError> {
    state
        .0
        .reveal_token()
        .map_err(|message| AppError::new("err.local_api.token_unavailable", message))
}

#[tauri::command]
pub fn rotate_local_api_token(state: tauri::State<'_, LocalApi>) -> Result<String, AppError> {
    state
        .0
        .rotate_token()
        .map_err(|message| AppError::new("err.local_api.token_rotate_failed", message))
}
