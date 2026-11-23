//! Data letters in the letter box: weekly data letters, time capsules and "on this day". All local, no network.

use crate::app_state::AppState;
use crate::commands::{spawn_independent_read, with_write};
use crate::ipc_error::AppError;
use chrono::{Local, NaiveDate, Utc};
use serde::Serialize;
use zeppbridge_core::insight::letters::{DataLetter, MailStatus, OnThisDayYear};
use zeppbridge_core::storage::write_lock::WritePurpose;

#[derive(Debug, Clone, Serialize)]
pub struct DataLettersView {
    pub letters: Vec<DataLetter>,
    pub on_this_day: Vec<OnThisDayYear>,
}

/// Opens the letter box: if last week's letter is not delivered yet, deliver it first (a short write; if a sync holds the write lock, skip it this time),
/// then read all letters and "on this day".
#[tauri::command]
pub async fn data_letters_state(
    state: tauri::State<'_, AppState>,
) -> std::result::Result<DataLettersView, AppError> {
    let today = Local::now().date_naive();
    let _ = with_write(
        &state.data_dir,
        &state.db,
        WritePurpose::Metadata,
        move |db| db.ensure_weekly_letter(today, Utc::now()),
    )
    .await;
    spawn_independent_read(state.data_dir.clone(), move |db| {
        Ok(DataLettersView {
            letters: db.data_letters()?,
            on_this_day: db.on_this_day(today)?,
        })
    })
    .await
}

/// State for the letter alert (the top-left badge turns into an envelope). Like opening the letter box, it first delivers last week's letter
/// (skipped if the write lock is taken), then reads the newest weekly letter id and the capsules that are due but still sealed.
#[tauri::command]
pub async fn data_letters_mail_status(
    state: tauri::State<'_, AppState>,
) -> std::result::Result<MailStatus, AppError> {
    let today = Local::now().date_naive();
    let _ = with_write(
        &state.data_dir,
        &state.db,
        WritePurpose::Metadata,
        move |db| db.ensure_weekly_letter(today, Utc::now()),
    )
    .await;
    spawn_independent_read(state.data_dir.clone(), move |db| {
        db.data_letter_mail_status(today)
    })
    .await
}

/// Seals a time capsule. `open_on` is a local day (YYYY-MM-DD).
#[tauri::command]
pub async fn data_letters_seal_capsule(
    state: tauri::State<'_, AppState>,
    note: String,
    open_on: String,
) -> std::result::Result<i64, AppError> {
    let open_on = NaiveDate::parse_from_str(&open_on, "%Y-%m-%d")
        .map_err(|_| AppError::new("err.letters.capsule_date", "The open date format is not valid"))?;
    let today = Local::now().date_naive();
    with_write(
        &state.data_dir,
        &state.db,
        WritePurpose::Metadata,
        move |db| db.seal_capsule(&note, open_on, today, Utc::now()),
    )
    .await
}

/// Opens a capsule that is due.
#[tauri::command]
pub async fn data_letters_open_capsule(
    state: tauri::State<'_, AppState>,
    id: i64,
) -> std::result::Result<DataLetter, AppError> {
    let today = Local::now().date_naive();
    with_write(
        &state.data_dir,
        &state.db,
        WritePurpose::Metadata,
        move |db| db.open_capsule(id, today, Utc::now()),
    )
    .await
}
