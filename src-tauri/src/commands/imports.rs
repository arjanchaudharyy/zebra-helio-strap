//! Import workouts from other sources: GPX, FIT, full Strava archive, Apple Health. Reading files and storing data are local. No network.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::app_state::AppState;
use crate::commands::{join_blocking, spawn_independent_read, spawn_independent_write};
use crate::ipc_error::AppError;
use chrono::Utc;
use zeppbridge_core::import::{read_import_file, ImportReadError, ImportSource, ParsedFile};
use zeppbridge_core::storage::imports::{ImportBatch, ImportReport};
use zeppbridge_core::storage::write_lock::WritePurpose;

/// Imports one or more files from one source as one batch (undo removes them together).
#[tauri::command]
pub async fn import_workouts(
    state: tauri::State<'_, AppState>,
    source: String,
    paths: Vec<String>,
) -> std::result::Result<ImportReport, AppError> {
    let source = ImportSource::parse(&source)
        .ok_or_else(|| AppError::new("err.import.source", "Unknown import source"))?;
    if paths.is_empty() {
        return Err(AppError::new("err.import.not_recognized", "No file selected"));
    }
    let file_name = match paths.as_slice() {
        [one] => PathBuf::from(one)
            .file_name()
            .map(|name| name.to_string_lossy().into_owned()),
        many => Some(format!("{} files", many.len())),
    };
    let parsed = join_blocking(
        tokio::task::spawn_blocking(move || {
            let mut merged = ParsedFile::default();
            for path in &paths {
                let one =
                    read_import_file(source, &PathBuf::from(path)).map_err(
                        |error| match error {
                            ImportReadError::TooLarge => {
                                AppError::new("err.import.too_large", "The file is too large (over 64 MB)")
                            }
                            ImportReadError::NotRecognized => AppError::new(
                                "err.import.not_recognized",
                                "This file is not an export from the selected source",
                            ),
                            ImportReadError::Io(_) => {
                                AppError::new("err.import.read", "Cannot read this file")
                            }
                        },
                    )?;
                merged.unreadable += one.unreadable;
                merged.activities.extend(one.activities);
                if let Some(health) = one.health {
                    let into = merged.health.get_or_insert_with(Default::default);
                    into.samples.extend(health.samples);
                    into.daily.extend(health.daily);
                    into.sleeps.extend(health.sleeps);
                }
            }
            Ok::<_, AppError>(merged)
        })
        .await,
    )??;
    spawn_independent_write(state.data_dir.clone(), WritePurpose::Metadata, move |db| {
        db.import_activities(source, file_name.as_deref(), parsed, Utc::now())
    })
    .await
}

/// All import batches, newest first.
#[tauri::command]
pub async fn import_batches(
    state: tauri::State<'_, AppState>,
) -> std::result::Result<Vec<ImportBatch>, AppError> {
    spawn_independent_read(state.data_dir.clone(), |db| db.import_batches()).await
}

/// Undoes a whole batch. Returns the number of workouts deleted.
#[tauri::command]
pub async fn undo_import_batch(
    state: tauri::State<'_, AppState>,
    batch_id: i64,
) -> std::result::Result<usize, AppError> {
    spawn_independent_write(state.data_dir.clone(), WritePurpose::Metadata, move |db| {
        db.undo_import_batch(batch_id, Utc::now())
    })
    .await
}

/// The source of each imported workout, for the source label in the workout list.
#[tauri::command]
pub async fn workout_import_sources(
    state: tauri::State<'_, AppState>,
) -> std::result::Result<HashMap<String, String>, AppError> {
    spawn_independent_read(state.data_dir.clone(), |db| {
        Ok(db.workout_import_sources()?.into_iter().collect())
    })
    .await
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct UnstrapExportResult {
    /// The written file (under `data\exports\`). The UI uses it for "Show in folder".
    pub path: String,
    pub workouts: usize,
    pub heart_rate_points: usize,
}

/// Beta lab: writes the workouts of the last `days` days (empty = all) as a zip that unstrap can import, saved to `exports\`.
#[tauri::command]
pub async fn export_for_unstrap(
    state: tauri::State<'_, AppState>,
    days: Option<u32>,
) -> std::result::Result<UnstrapExportResult, AppError> {
    let failed = || AppError::new("err.export.unstrap", "Could not write the export package for unstrap");
    let since = days.map(|days| Utc::now() - chrono::Duration::days(i64::from(days)));
    let export = spawn_independent_read(state.data_dir.clone(), move |db| {
        zeppbridge_core::export_unstrap::build_unstrap_export(db, since)
    })
    .await?;
    let dir = state.data_dir.join("exports");
    let path = dir.join(zeppbridge_core::export_unstrap::export_file_name(
        chrono::Local::now().date_naive(),
    ));
    let written = path.clone();
    join_blocking(
        tokio::task::spawn_blocking(move || {
            std::fs::create_dir_all(&dir).map_err(|_| failed())?;
            std::fs::write(&written, &export.bytes).map_err(|_| failed())
        })
        .await,
    )??;
    Ok(UnstrapExportResult {
        path: path.to_string_lossy().into_owned(),
        workouts: export.workouts,
        heart_rate_points: export.heart_rate_points,
    })
}
