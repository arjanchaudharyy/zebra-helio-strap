//! "Take everything with me": exports all records the user wrote or curated as one JSON file (core::storage::my_records).

use std::path::PathBuf;

use crate::app_state::AppState;
use crate::commands::{join_blocking, spawn_independent_read};
use crate::ipc_error::AppError;

/// Writes to the location the user chose in the save dialog. Returns the total record count.
#[tauri::command]
pub async fn export_my_records(
    state: tauri::State<'_, AppState>,
    path: String,
) -> std::result::Result<u64, AppError> {
    let failed = || AppError::new("err.export.my_records", "Could not write this file");
    let path = PathBuf::from(path);
    if !path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
    {
        return Err(failed());
    }
    let export = spawn_independent_read(state.data_dir.clone(), |db| {
        db.export_my_records(chrono::Utc::now())
    })
    .await?;
    let total = export["counts"]
        .as_object()
        .map_or(0, |counts| counts.values().filter_map(|v| v.as_u64()).sum());
    join_blocking(
        tokio::task::spawn_blocking(move || {
            let text = serde_json::to_vec_pretty(&export).map_err(|_| failed())?;
            std::fs::write(&path, text).map_err(|_| failed())
        })
        .await,
    )??;
    Ok(total)
}
