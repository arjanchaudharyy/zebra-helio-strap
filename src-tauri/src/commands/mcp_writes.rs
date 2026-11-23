//! Settings "Local AI" card: recent AI changes (undo one by one) and the "shared tasks only" switch.
//!
//! The ledger and undo rules are in core `storage::mcp_writes`. This file only maps the outcome to stable codes.

use super::{spawn_independent_read, with_write};
use crate::app_state::AppState;
use crate::ipc_error::AppError;
use zeppbridge_core::storage::mcp_writes::{McpWrite, UndoOutcome};
use zeppbridge_core::storage::write_lock::WritePurpose;

/// The maximum number of entries the UI lists.
const LIST_LIMIT: usize = 30;

#[tauri::command]
pub async fn mcp_writes_list(state: tauri::State<'_, AppState>) -> Result<Vec<McpWrite>, AppError> {
    spawn_independent_read(state.data_dir.clone(), |db| db.mcp_writes(LIST_LIMIT)).await
}

#[tauri::command]
pub async fn mcp_write_undo(
    state: tauri::State<'_, AppState>,
    id: i64,
) -> Result<Vec<McpWrite>, AppError> {
    let outcome = with_write(
        &state.data_dir,
        &state.db,
        WritePurpose::Metadata,
        move |db| db.undo_mcp_write(id),
    )
    .await?;
    match outcome {
        UndoOutcome::Undone => {}
        UndoOutcome::Stale => {
            return Err(AppError::new(
                "err.mcp_writes.stale",
                "This record changed again after the AI edit, so it cannot go back to its state before the AI edit. Change it by hand on its page",
            ))
        }
        UndoOutcome::Missing => {
            return Err(AppError::new(
                "err.mcp_writes.missing",
                "This change was already undone, or it cannot be found",
            ))
        }
    }
    spawn_independent_read(state.data_dir.clone(), |db| db.mcp_writes(LIST_LIMIT)).await
}

/// When on, the generated config has `--scope task`: the AI sees only tasks shared through "Hand to AI", and all write tools are refused.
#[tauri::command]
pub async fn set_mcp_task_only(
    state: tauri::State<'_, AppState>,
    on: bool,
) -> Result<bool, AppError> {
    with_write(
        &state.data_dir,
        &state.db,
        WritePurpose::Metadata,
        move |db| {
            db.set_mcp_task_scope_only(on)?;
            db.mcp_task_scope_only()
        },
    )
    .await
}
