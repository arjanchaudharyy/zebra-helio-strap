//! Training plan: draft, preview, send to the watch, undo, clear.
//!
//! Publishing has three stages (see `zeppbridge_core::training_plan::publish`): prepare and record inside the write lock ->
//! release the write lock and go online -> record the result inside the write lock. The online stage holds no write lock, so sync and other short writes do not wait for it.

use super::{spawn_independent_read, with_write};
use crate::app_state::AppState;
use crate::ipc_error::AppError;
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use std::path::Path;
use zeppbridge_core::models::error::Result as CoreResult;
use zeppbridge_core::official::{OfficialClient, OfficialStore};
use zeppbridge_core::storage::training_plan::{
    DraftOrigin, DraftPreview, PlanDraft, PlanOverview, Prepared, PublishRecord, PublishRequest,
    SendOutcome,
};
use zeppbridge_core::storage::write_lock::WritePurpose;
use zeppbridge_core::storage::Database;
use zeppbridge_core::training_plan::publish::{ensure_connected, send_window};
use zeppbridge_core::training_plan::PlanDocument;

/// Plan dates use this computer's calendar: "tomorrow" in the UI is this computer's tomorrow.
fn today() -> NaiveDate {
    chrono::Local::now().date_naive()
}

#[derive(Debug, Serialize)]
pub struct TrainingPlanState {
    #[serde(flatten)]
    pub overview: PlanOverview,
    pub drafts: Vec<PlanDraft>,
    pub ai_may_publish: bool,
}

#[tauri::command]
pub async fn training_plan_state(
    state: tauri::State<'_, AppState>,
) -> Result<TrainingPlanState, AppError> {
    spawn_independent_read(state.data_dir.clone(), |db| {
        Ok(TrainingPlanState {
            overview: db.plan_overview(today())?,
            drafts: db.open_plan_drafts()?,
            ai_may_publish: db.ai_may_publish_plans()?,
        })
    })
    .await
}

/// Saves a draft. The UI can save only "what you wrote" and "a pasted AI reply". MCP uses its own process.
#[tauri::command]
pub async fn training_plan_save_draft(
    state: tauri::State<'_, AppState>,
    document: PlanDocument,
    pasted: bool,
) -> Result<String, AppError> {
    let origin = if pasted {
        DraftOrigin::AiPaste
    } else {
        DraftOrigin::User
    };
    with_write(
        &state.data_dir,
        &state.db,
        WritePurpose::Metadata,
        move |db| db.save_plan_draft(origin, &document),
    )
    .await
}

#[tauri::command]
pub async fn training_plan_preview(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<DraftPreview, AppError> {
    spawn_independent_read(state.data_dir.clone(), move |db| {
        db.preview_plan_draft(&id, today())
    })
    .await
}

#[tauri::command]
pub async fn training_plan_discard(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<bool, AppError> {
    with_write(
        &state.data_dir,
        &state.db,
        WritePurpose::Metadata,
        move |db| db.discard_plan_draft(&id),
    )
    .await
}

#[tauri::command]
pub async fn training_plan_set_ai_publish(
    state: tauri::State<'_, AppState>,
    allowed: bool,
) -> Result<bool, AppError> {
    with_write(
        &state.data_dir,
        &state.db,
        WritePurpose::Metadata,
        move |db| {
            db.set_ai_may_publish_plans(allowed)?;
            db.ai_may_publish_plans()
        },
    )
    .await
}

/// Pushes the UI can start. The rolling push normally runs after each sync. `Resend` is a manual retry by the user
/// (a week stays "unconfirmed" while offline, and the user must be able to resend without waiting for the next sync).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum PublishAction {
    Draft { id: String },
    Undo,
    Clear,
    Resend,
}

/// The push outcome. `outcome` matches the core `Prepared`. `send` adds the ledger result.
#[derive(Debug, Serialize)]
pub struct PublishResult {
    pub outcome: Prepared,
    /// When something was really sent, the ledger summary for this batch (`sent` / `partial` / `rejected` / `unknown`).
    pub record: Option<PublishRecord>,
    /// Per-window results for this batch: which weeks arrived and which did not.
    pub records: Vec<PublishRecord>,
}

#[tauri::command]
pub async fn training_plan_publish(
    state: tauri::State<'_, AppState>,
    action: PublishAction,
    confirm_clear: bool,
    locale: Option<String>,
) -> Result<PublishResult, AppError> {
    let request = match action {
        PublishAction::Draft { id } => PublishRequest::Draft { id },
        PublishAction::Undo => PublishRequest::UndoLast,
        PublishAction::Clear => PublishRequest::Clear,
        PublishAction::Resend => PublishRequest::Roll,
    };
    publish(&state.data_dir, &state.db, request, confirm_clear, locale).await
}

fn batch_result(db: &Database, outcome: Prepared) -> CoreResult<PublishResult> {
    let (record, records) = match &outcome {
        Prepared::Send { batch_id, .. } => {
            (db.plan_batch_summary(*batch_id)?, db.plan_batch(*batch_id)?)
        }
        _ => (None, Vec::new()),
    };
    Ok(PublishResult {
        outcome,
        record,
        records,
    })
}

async fn publish(
    data_dir: &Path,
    database: &tokio::sync::Mutex<Database>,
    request: PublishRequest,
    confirm_clear: bool,
    locale: Option<String>,
) -> Result<PublishResult, AppError> {
    // An isolated, explicitly seeded demo has no credentials and cannot send HTTP.
    let demo = database.lock().await.is_demo_library()? && data_dir.join(".demo-library").is_file();
    if demo {
        return with_write(data_dir, database, WritePurpose::Metadata, |db| {
            let outcome = db.prepare_plan_publish(&request, today(), confirm_clear)?;
            if let Prepared::Send { sends, .. } = &outcome {
                for send in sends {
                    db.finish_plan_publish(send.publish_id, &SendOutcome::Delivered)?;
                }
            }
            batch_result(db, outcome)
        })
        .await;
    }
    let store = OfficialStore::new(data_dir);
    let client = OfficialClient::new()?;
    // With no usable token, do not touch the ledger.
    ensure_connected(&store, &client).await?;
    let day = today();
    let prepared = with_write(data_dir, database, WritePurpose::Metadata, |db| {
        if let Some(tag) = locale.as_deref() {
            db.set_plan_watch_locale(tag)?;
        }
        db.prepare_plan_publish(&request, day, confirm_clear)
    })
    .await?;
    let Prepared::Send { batch_id, sends } = &prepared else {
        return Ok(PublishResult {
            outcome: prepared,
            record: None,
            records: Vec::new(),
        });
    };
    // Send window by window in ledger order (farthest first, a placeholder right after the window before it), and record the result
    // after each one. No write lock while online. If authorization fails, stop sending and record the remaining windows as the same rejection.
    let mut abort: Option<SendOutcome> = None;
    for send in sends {
        let outcome = match &abort {
            Some(outcome) => outcome.clone(),
            None => send_window(&store, &client, &send.body).await,
        };
        if matches!(&outcome, SendOutcome::Rejected { error_code, .. }
            if error_code == "err.core.needs_reauth")
        {
            abort = Some(outcome.clone());
        }
        let publish_id = send.publish_id;
        with_write(data_dir, database, WritePurpose::Metadata, move |db| {
            db.finish_plan_publish(publish_id, &outcome)
        })
        .await?;
    }
    if abort.is_some() {
        return Err(AppError::new(
            "err.core.needs_reauth",
            "Authentication expired. Reconnect Zepp",
        ));
    }
    let batch_id = *batch_id;
    let (record, records) = with_write(data_dir, database, WritePurpose::Metadata, move |db| {
        Ok((db.plan_batch_summary(batch_id)?, db.plan_batch(batch_id)?))
    })
    .await?;
    Ok(PublishResult {
        outcome: prepared,
        record,
        records,
    })
}

/// Rolling push after a sync: the window moved forward one day, so resend the windows where the ledger and the active plan differ
/// (including the week that did not arrive last time). Best effort, only logs. Sends nothing without an official link or when the content is unchanged,
/// and never clears a window.
pub(crate) async fn roll_after_sync(state: &AppState) {
    let store = OfficialStore::new(&state.data_dir);
    if !store
        .meta()
        .ok()
        .flatten()
        .is_some_and(|meta| !meta.needs_reauth)
    {
        return;
    }
    match publish(
        &state.data_dir,
        &state.db,
        PublishRequest::Roll,
        false,
        None,
    )
    .await
    {
        Ok(PublishResult {
            record: Some(record),
            ..
        }) if record.state != "sent" => {
            eprintln!("Training plan rolling push not confirmed: {}", record.state);
        }
        Ok(_) => {}
        Err(error) => eprintln!("Training plan rolling push failed: {}", error.code),
    }
}

/// After official authorization is disconnected: the official side deletes the synced plans, so the local ledger resets too.
pub(crate) async fn forget_after_revoke(state: &AppState) {
    let result = with_write(&state.data_dir, &state.db, WritePurpose::Metadata, |db| {
        db.forget_plans_after_revoke(today())
    })
    .await;
    if let Err(error) = result {
        eprintln!("Could not clear the training plan ledger after disconnect: {}", error.code);
    }
}

/// Every plan that was pushed (kept as a record), newest first.
#[tauri::command]
pub async fn training_plan_history(
    state: tauri::State<'_, AppState>,
    limit: Option<usize>,
) -> Result<Vec<zeppbridge_core::storage::training_plan::PlanHistoryEntry>, AppError> {
    spawn_independent_read(state.data_dir.clone(), move |db| {
        db.plan_history(limit.unwrap_or(60))
    })
    .await
}

#[tauri::command]
pub async fn training_plan_adherence(
    state: tauri::State<'_, AppState>,
    from: String,
    to: String,
) -> Result<Vec<zeppbridge_core::storage::plan_adherence::AdherenceDay>, AppError> {
    let from = NaiveDate::parse_from_str(&from, "%Y-%m-%d")
        .map_err(|_| AppError::new("err.ai_task.invalid", "Invalid date"))?;
    let to = NaiveDate::parse_from_str(&to, "%Y-%m-%d")
        .map_err(|_| AppError::new("err.ai_task.invalid", "Invalid date"))?;
    spawn_independent_read(state.data_dir.clone(), move |db| {
        db.plan_adherence(from, to)
    })
    .await
}
#[tauri::command]
pub async fn training_plan_update_draft(
    state: tauri::State<'_, AppState>,
    id: String,
    document: PlanDocument,
) -> Result<bool, AppError> {
    with_write(&state.data_dir, &state.db, WritePurpose::Metadata, |db| {
        db.update_plan_draft(&id, &document)
    })
    .await
}
