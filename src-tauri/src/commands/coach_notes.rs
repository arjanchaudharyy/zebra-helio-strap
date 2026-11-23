//! Coach profile (AI memory inbox): notes the AI proposes through MCP and notes the user writes share this one table.
//! The UI confirms, rejects, edits or deletes each note, and can export them as one Markdown file.

use super::{spawn_independent_read, with_write};
use crate::app_state::AppState;
use crate::ipc_error::AppError;
use serde::Serialize;
use zeppbridge_core::storage::coach_notes::{CoachNote, NOTE_MAX_CHARS};
use zeppbridge_core::storage::write_lock::WritePurpose;

#[derive(Debug, Serialize)]
pub struct CoachNotes {
    /// Remembered notes, oldest first.
    pub accepted: Vec<CoachNote>,
    /// Notes waiting for user confirmation, newest first.
    pub proposed: Vec<CoachNote>,
}

/// Text problems are reported by code here, so the core message does not reach the UI directly.
fn checked(text: &str) -> Result<(), AppError> {
    let clean = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if clean.is_empty() {
        return Err(AppError::new("err.coach_notes.empty", "The content is empty"));
    }
    if clean.chars().count() > NOTE_MAX_CHARS {
        return Err(AppError::new("err.coach_notes.too_long", "A note can have at most 500 characters"));
    }
    Ok(())
}

fn stale() -> AppError {
    AppError::new("err.coach_notes.stale", "This note was already handled or deleted")
}

#[tauri::command]
pub async fn coach_notes_list(state: tauri::State<'_, AppState>) -> Result<CoachNotes, AppError> {
    spawn_independent_read(state.data_dir.clone(), |db| {
        Ok(CoachNotes {
            accepted: db.accepted_coach_notes()?,
            proposed: db.proposed_coach_notes()?,
        })
    })
    .await
}

#[tauri::command]
pub async fn coach_note_add(
    state: tauri::State<'_, AppState>,
    text: String,
) -> Result<i64, AppError> {
    checked(&text)?;
    with_write(&state.data_dir, &state.db, WritePurpose::Metadata, |db| {
        db.add_coach_note(&text)
    })
    .await
}

#[tauri::command]
pub async fn coach_note_decide(
    state: tauri::State<'_, AppState>,
    id: i64,
    accept: bool,
) -> Result<(), AppError> {
    let changed = with_write(&state.data_dir, &state.db, WritePurpose::Metadata, |db| {
        db.decide_coach_note(id, accept)
    })
    .await?;
    changed.then_some(()).ok_or_else(stale)
}

#[tauri::command]
pub async fn coach_note_edit(
    state: tauri::State<'_, AppState>,
    id: i64,
    text: String,
) -> Result<(), AppError> {
    checked(&text)?;
    let changed = with_write(&state.data_dir, &state.db, WritePurpose::Metadata, |db| {
        db.edit_coach_note(id, &text)
    })
    .await?;
    changed.then_some(()).ok_or_else(stale)
}

#[tauri::command]
pub async fn coach_note_delete(state: tauri::State<'_, AppState>, id: i64) -> Result<(), AppError> {
    with_write(&state.data_dir, &state.db, WritePurpose::Metadata, |db| {
        db.delete_coach_note(id)
    })
    .await
}

/// Exports "My coach profile": the UI builds the Markdown in its own language. This only writes it to the `ZeppBridge AI` folder on the desktop
/// (next to the files handed to AI) and returns the path so the UI can select it in the file manager. The file name keeps only safe characters from the name the UI gives.
#[tauri::command]
pub async fn coach_notes_export(
    state: tauri::State<'_, AppState>,
    file_stem: String,
    markdown: String,
) -> Result<String, AppError> {
    let failed = || AppError::new("err.coach_notes.export", "Could not export the coach profile");
    if markdown.len() > 4 * 1024 * 1024 {
        return Err(failed());
    }
    let stem: String = file_stem
        .chars()
        .filter(|c| !c.is_control() && !r#"\/:*?"<>|"#.contains(*c))
        .take(60)
        .collect();
    let stem = stem.trim().trim_matches('.');
    let stem = if stem.is_empty() { "coach-notes" } else { stem };
    let root = directories::UserDirs::new()
        .and_then(|dirs| dirs.desktop_dir().map(|path| path.join("ZeppBridge AI")))
        .unwrap_or_else(|| state.data_dir.join("exports").join("ai-tasks"));
    let path = root.join(format!("{stem}.md"));
    std::fs::create_dir_all(&root).map_err(|_| failed())?;
    std::fs::write(&path, markdown).map_err(|_| failed())?;
    Ok(path.to_string_lossy().into_owned())
}
