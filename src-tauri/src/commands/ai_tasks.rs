//! The ten `ai_task*` / `ai_template*` commands.
//!
//! The split follows the protocol:
//! - Read commands use `spawn_independent_read` (read-only connection, no write lock).
//! - Write commands use `with_write(WritePurpose::Metadata)`.
//! - `ai_task_prepare` has two stages: the bundle build uses `spawn_independent_read`,
//!   and the disk write is plain file IO on `spawn_blocking`. It writes new files under exports,
//!   not zepp.db, so it does not hold `state.db` and needs no cross-process write lock.
//! - `ai_task_attachment_stat` only runs stat and does not touch the database. It uses `spawn_blocking`.
//!
//! All error codes come from core (`err.ai_task.*` / `err.ai_template.*`).
//! This layer is only a thin adapter. It does not guess entities at the command layer.

use super::{join_blocking, spawn_independent_read, with_write};
use crate::app_state::AppState;
use crate::ipc_error::AppError;
use zeppbridge_core::ai_tasks::export::markdown::{DEFAULT_TOKEN_BUDGET, SUBSCRIBED_TOKEN_BUDGET};
use zeppbridge_core::ai_tasks::export::{AiTaskMarkdownParts, AiTaskPromptParts};
use zeppbridge_core::ai_tasks::{
    stat_attachment_paths, AiTask, AiTaskAttachmentStat, AiTaskPrepareResult, AiTaskPreview,
    AiTaskSummary, AiTaskTemplate, DeletedAiTask,
};
use zeppbridge_core::storage::card_collection::CardPick;
use zeppbridge_core::storage::write_lock::WritePurpose;

/// Maximum number of paths for one `ai_task_attachment_stat` call. It is only a batch stat interface,
/// not a place for the frontend to send a full disk file list.
const MAX_STAT_PATHS: usize = 256;

/// Lists all tasks (newest update first). Reads only the index columns.
#[tauri::command]
pub async fn ai_task_list(
    state: tauri::State<'_, AppState>,
) -> std::result::Result<Vec<AiTaskSummary>, AppError> {
    spawn_independent_read(state.data_dir.clone(), |db| db.list_ai_tasks()).await
}

/// Gets the full object of one task. If it does not exist: `err.ai_task.not_found`.
#[tauri::command]
pub async fn ai_task_get(
    state: tauri::State<'_, AppState>,
    id: String,
) -> std::result::Result<AiTask, AppError> {
    spawn_independent_read(state.data_dir.clone(), move |db| db.require_ai_task(&id)).await
}

/// Saves a task (new or update). The backend fills the id and timestamps. The `mcp_shared` switch uses this command.
#[tauri::command]
pub async fn ai_task_save(
    state: tauri::State<'_, AppState>,
    task: AiTask,
) -> std::result::Result<AiTask, AppError> {
    with_write(&state.data_dir, &state.db, WritePurpose::Metadata, |db| {
        db.save_ai_task(&task)
    })
    .await
}

/// Pins or unpins a saved task.
#[tauri::command]
pub async fn ai_task_set_pinned(
    state: tauri::State<'_, AppState>,
    id: String,
    pinned: bool,
) -> std::result::Result<(), AppError> {
    with_write(
        &state.data_dir,
        &state.db,
        WritePurpose::Metadata,
        move |db| db.set_ai_task_pinned(&id, pinned),
    )
    .await
}

/// Deletes a task row. The original attachments stay.
#[tauri::command]
pub async fn ai_task_delete(
    state: tauri::State<'_, AppState>,
    id: String,
) -> std::result::Result<(), AppError> {
    with_write(&state.data_dir, &state.db, WritePurpose::Metadata, |db| {
        db.delete_ai_task(&id)
    })
    .await
}

/// Deletes tasks in a batch: one write lock, one transaction. Returns the deleted tasks, so the UI can save them back as they were on undo.
#[tauri::command]
pub async fn ai_task_delete_many(
    state: tauri::State<'_, AppState>,
    ids: Vec<String>,
) -> std::result::Result<Vec<DeletedAiTask>, AppError> {
    with_write(
        &state.data_dir,
        &state.db,
        WritePurpose::Metadata,
        move |db| db.delete_ai_tasks(&ids),
    )
    .await
}

/// Template list, built-in templates first.
#[tauri::command]
pub async fn ai_template_list(
    state: tauri::State<'_, AppState>,
) -> std::result::Result<Vec<AiTaskTemplate>, AppError> {
    spawn_independent_read(state.data_dir.clone(), |db| db.list_ai_task_templates()).await
}

/// Saves a user template. Built-in templates are read-only: `err.ai_template.builtin_readonly`.
#[tauri::command]
pub async fn ai_template_save(
    state: tauri::State<'_, AppState>,
    template: AiTaskTemplate,
) -> std::result::Result<AiTaskTemplate, AppError> {
    with_write(&state.data_dir, &state.db, WritePurpose::Metadata, |db| {
        db.save_ai_task_template(&template)
    })
    .await
}

/// Deletes a user template.
#[tauri::command]
pub async fn ai_template_delete(
    state: tauri::State<'_, AppState>,
    id: String,
) -> std::result::Result<(), AppError> {
    with_write(&state.data_dir, &state.db, WritePurpose::Metadata, |db| {
        db.delete_ai_task_template(&id)
    })
    .await
}

/// Previews the coverage facts and warnings of a task. Also accepts an unsaved draft (empty id).
/// If `workout_ids` has a workout that is not on this device: `err.ai_task.workout_not_found`.
#[tauri::command]
pub async fn ai_task_preview(
    state: tauri::State<'_, AppState>,
    task: AiTask,
    token_budget: Option<usize>,
) -> std::result::Result<AiTaskPreview, AppError> {
    let budget = token_budget.map(clamp_budget);
    spawn_independent_read(state.data_dir.clone(), move |db| {
        db.ai_task_preview_with_budget(&task, budget)
    })
    .await
}

/// The budget accepts only sensible values near the two levels: the UI sends only "free plan" or "I subscribe".
fn clamp_budget(budget: usize) -> usize {
    budget.clamp(DEFAULT_TOKEN_BUDGET / 3, SUBSCRIBED_TOKEN_BUDGET * 4)
}

/// Writes the data JSON and the prompt txt (plus copies of the original attachments; the frontend gives file names by the naming rule) to
/// the desktop folder `ZeppBridge AI/<task name>_<time>/`. The user drags them from the desktop into the AI chat.
/// If the desktop is not available, falls back to `data_dir/exports/ai-tasks/`. A `missing` attachment gives `blocked`.
///
/// Two stages: the build (validation, anchors, coverage, bundle, serialization) runs on an independent read-only
/// connection and does not hold `state.db`. The disk write is plain file IO. It writes new files, not
/// zepp.db. The cross-process write lock is for database writers, so it is not needed here.
/// Optional part of `ai_task_prepare`: the task brief that the frontend prepared in the UI language, the final prompt that the user edited,
/// and the stems of the two files. When absent (old frontend / not sent), the behavior is the same as before.
#[derive(Debug, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AiTaskPrepareOptions {
    brief_text: Option<String>,
    prompt_override: Option<String>,
    data_file_stem: Option<String>,
    prompt_file_stem: Option<String>,
    /// When given, deliver one `.md`. `markdown_guide` is the frontend's localized "how to read this file" text.
    token_budget: Option<usize>,
    markdown_guide: Option<String>,
    provider: Option<String>,
}

#[tauri::command]
pub async fn ai_task_prepare(
    state: tauri::State<'_, AppState>,
    task: AiTask,
    coverage_note: String,
    direction_text: Option<String>,
    options: Option<AiTaskPrepareOptions>,
) -> std::result::Result<AiTaskPrepareResult, AppError> {
    let output_root = directories::UserDirs::new()
        .and_then(|dirs| dirs.desktop_dir().map(|path| path.join("ZeppBridge AI")))
        .unwrap_or_else(|| state.data_dir.join("exports").join("ai-tasks"));
    let recorded_task = task.clone();
    let provider = options
        .as_ref()
        .and_then(|o| o.provider.clone())
        .unwrap_or_else(|| "export".into());
    let plan = spawn_independent_read(state.data_dir.clone(), move |db| {
        let options = options.unwrap_or_default();
        let parts = AiTaskPromptParts {
            brief: options.brief_text.as_deref(),
            direction: direction_text.as_deref(),
            override_text: options.prompt_override.as_deref(),
            data_stem: options.data_file_stem.as_deref(),
            prompt_stem: options.prompt_file_stem.as_deref(),
            markdown: options.token_budget.map(|budget| AiTaskMarkdownParts {
                guide: options.markdown_guide.as_deref(),
                token_budget: clamp_budget(budget),
            }),
        };
        db.ai_task_prepare_plan(&task, &coverage_note, &parts, &output_root)
    })
    .await?;
    let result = join_blocking(tokio::task::spawn_blocking(move || plan.finish()).await)
        .and_then(|result| result.map_err(AppError::from))?;
    if let Some(path) = &result.md_path {
        with_write(&state.data_dir, &state.db, WritePurpose::Metadata, |db| {
            db.record_ai_exchange(&recorded_task, &provider, path)
        })
        .await?;
    }
    Ok(result)
}

/// Batch stat of the attachment paths the user picked. For local checks only, so `path` can be returned here.
#[tauri::command]
pub async fn ai_task_attachment_stat(
    paths: Vec<String>,
) -> std::result::Result<Vec<AiTaskAttachmentStat>, AppError> {
    if paths.len() > MAX_STAT_PATHS {
        return Err(AppError::new(
            "err.ai_task.invalid",
            "At most 256 attachment paths per check",
        ));
    }
    join_blocking(tokio::task::spawn_blocking(move || stat_attachment_paths(&paths)).await)
}

#[tauri::command]
pub async fn ai_task_day_strip(
    state: tauri::State<'_, AppState>,
    days_before: i64,
    end: Option<String>,
) -> Result<Vec<zeppbridge_core::storage::ai_strip::DayStripRow>, AppError> {
    let end = match end {
        Some(date) => chrono::NaiveDate::parse_from_str(&date, "%Y-%m-%d")
            .map_err(|_| AppError::new("err.ai_task.invalid", "Invalid date"))?,
        None => chrono::Local::now().date_naive(),
    };
    spawn_independent_read(state.data_dir.clone(), move |db| {
        db.ai_task_day_strip(days_before, end)
    })
    .await
}
#[tauri::command]
pub async fn ai_exchange_list(
    state: tauri::State<'_, AppState>,
    limit: usize,
) -> Result<Vec<zeppbridge_core::storage::ai_exchanges::AiExchange>, AppError> {
    spawn_independent_read(state.data_dir.clone(), move |db| db.ai_exchange_list(limit)).await
}
/// Names one round-trip record (blank = go back to the original question). Returns whether the record still exists.
#[tauri::command]
pub async fn ai_exchange_rename(
    state: tauri::State<'_, AppState>,
    id: String,
    title: String,
) -> Result<bool, AppError> {
    with_write(
        &state.data_dir,
        &state.db,
        WritePurpose::Metadata,
        move |db| db.rename_ai_exchange(&id, &title),
    )
    .await
}

/// Deletes one round-trip record. Returns whether something was really deleted.
#[tauri::command]
pub async fn ai_exchange_delete(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<bool, AppError> {
    with_write(
        &state.data_dir,
        &state.db,
        WritePurpose::Metadata,
        move |db| db.delete_ai_exchange(&id),
    )
    .await
}

#[tauri::command]
pub async fn ai_profile_save(
    state: tauri::State<'_, AppState>,
    note: String,
) -> Result<(), AppError> {
    with_write(&state.data_dir, &state.db, WritePurpose::Metadata, |db| {
        db.set_ai_profile_note(&note)
    })
    .await
}

/// The cards that are in the inbox now.
#[tauri::command]
pub async fn card_collection_get(
    state: tauri::State<'_, AppState>,
) -> std::result::Result<Vec<CardPick>, AppError> {
    spawn_independent_read(state.data_dir.clone(), |db| db.card_collection()).await
}

/// Replaces the full inbox. Returns the deduplicated and sorted copy.
#[tauri::command]
pub async fn card_collection_set(
    state: tauri::State<'_, AppState>,
    picks: Vec<CardPick>,
) -> std::result::Result<Vec<CardPick>, AppError> {
    with_write(
        &state.data_dir,
        &state.db,
        WritePurpose::Metadata,
        move |db| db.set_card_collection(&picks),
    )
    .await
}

/// Prompts that one task used before (the version history of the Prompt library), newest first.
#[tauri::command]
pub async fn ai_prompt_versions(
    state: tauri::State<'_, AppState>,
    id: String,
) -> std::result::Result<Vec<zeppbridge_core::ai_tasks::library::AiPromptVersion>, AppError> {
    spawn_independent_read(state.data_dir.clone(), move |db| db.ai_prompt_versions(&id)).await
}

/// Exports one task as `.zbprompt` to the "ZeppBridge AI" folder on the desktop (the same place as the coach profile). Returns the path.
#[tauri::command]
pub async fn ai_prompt_export(
    state: tauri::State<'_, AppState>,
    id: String,
) -> std::result::Result<String, AppError> {
    let failed = || AppError::new("err.ai_task.prompt_export", "Failed to export .zbprompt");
    let text = spawn_independent_read(state.data_dir.clone(), {
        let id = id.clone();
        move |db| db.export_zbprompt(&id)
    })
    .await?;
    let title = serde_json::from_str::<zeppbridge_core::ai_tasks::library::ZbPrompt>(&text)
        .map(|prompt| prompt.title)
        .unwrap_or_default();
    let stem: String = title
        .chars()
        .filter(|c| !c.is_control() && !r#"\/:*?"<>|"#.contains(*c))
        .take(60)
        .collect();
    let stem = stem.trim().trim_matches('.');
    let stem = if stem.is_empty() { "prompt" } else { stem };
    let root = directories::UserDirs::new()
        .and_then(|dirs| dirs.desktop_dir().map(|path| path.join("ZeppBridge AI")))
        .unwrap_or_else(|| state.data_dir.join("exports").join("ai-tasks"));
    let path = root.join(format!("{stem}.zbprompt"));
    join_blocking(
        tokio::task::spawn_blocking(move || {
            std::fs::create_dir_all(&root).map_err(|_| failed())?;
            std::fs::write(&path, text).map_err(|_| failed())?;
            Ok(path.to_string_lossy().into_owned())
        })
        .await,
    )?
}

/// Imports one `.zbprompt` and saves it as one entry in "Saved tasks" (with no linked workout).
#[tauri::command]
pub async fn ai_prompt_import(
    state: tauri::State<'_, AppState>,
    path: String,
) -> std::result::Result<AiTask, AppError> {
    use zeppbridge_core::ai_tasks::library::ZBPROMPT_MAX_BYTES;
    let unreadable = || AppError::new("err.ai_task.prompt_import", "Cannot read this .zbprompt");
    let text = join_blocking(
        tokio::task::spawn_blocking(move || {
            let size = std::fs::metadata(&path).map_err(|_| unreadable())?.len();
            if size > ZBPROMPT_MAX_BYTES {
                return Err(unreadable());
            }
            std::fs::read_to_string(&path).map_err(|_| unreadable())
        })
        .await,
    )??;
    with_write(
        &state.data_dir,
        &state.db,
        WritePurpose::Metadata,
        move |db| db.import_zbprompt(&text),
    )
    .await
}
