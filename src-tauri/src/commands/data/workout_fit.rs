//! Replaces a workout with a corrected `.fit` file. This works only on this device: the Zepp open API has no
//! endpoint to import workouts. The cloud copy stays as is, and an undo brings the original back.

use super::{spawn_independent_read, with_write};
use crate::app_state::AppState;
use crate::export_fit::{decode_fit_file, FitImportError, MAX_FIT_IMPORT_BYTES};
use crate::ipc_error::AppError;
use crate::storage::{FitOverrideOutcome, WorkoutFitOverride, FIT_START_TOLERANCE_SECONDS};
use std::path::Path;
use zeppbridge_core::storage::write_lock::WritePurpose;

#[tauri::command]
pub async fn get_workout_fit_override(
    state: tauri::State<'_, AppState>,
    workout_id: String,
) -> std::result::Result<Option<WorkoutFitOverride>, AppError> {
    spawn_independent_read(state.data_dir.clone(), move |db| {
        db.workout_fit_override(&workout_id)
    })
    .await
}

#[tauri::command]
pub async fn import_workout_fit(
    state: tauri::State<'_, AppState>,
    workout_id: String,
    path: String,
) -> std::result::Result<WorkoutFitOverride, AppError> {
    let file = Path::new(&path);
    let is_fit = file
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("fit"));
    if !is_fit {
        return Err(not_fit());
    }
    let size = std::fs::metadata(file)
        .map_err(|_| AppError::new("err.workout_fit.read_failed", "Cannot read this file"))?
        .len();
    if size > MAX_FIT_IMPORT_BYTES as u64 {
        return Err(too_large());
    }
    let bytes = std::fs::read(file)
        .map_err(|_| AppError::new("err.workout_fit.read_failed", "Cannot read this file"))?;
    let imported = decode_fit_file(&bytes).map_err(|error| match error {
        FitImportError::TooLarge => too_large(),
        FitImportError::NotFit => not_fit(),
        FitImportError::MultipleActivities => AppError::new(
            "err.workout_fit.multiple_activities",
            "Use a FIT file that contains only one workout",
        ),
        FitImportError::NoActivity => AppError::new(
            "err.workout_fit.no_activity",
            "This .fit file has no workout data (no records and no laps)",
        ),
    })?;
    let file_name = file
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_owned);
    let outcome = with_write(&state.data_dir, &state.db, WritePurpose::Metadata, |db| {
        db.import_workout_fit(&workout_id, file_name.as_deref(), &bytes, &imported)
    })
    .await?;
    match outcome {
        FitOverrideOutcome::Applied(info) => Ok(info),
        FitOverrideOutcome::WorkoutMissing => {
            Err(AppError::new("err.workout.not_found", "Workout not found"))
        }
        FitOverrideOutcome::StartMismatch {
            workout_start,
            fit_start,
        } => Err(AppError::new(
            "err.workout_fit.start_mismatch",
            "The start time of this .fit file does not match this workout. It is probably a different workout",
        )
        .with_params(serde_json::json!({
            "workoutStart": workout_start.to_rfc3339(),
            "fitStart": fit_start.to_rfc3339(),
            "toleranceMinutes": FIT_START_TOLERANCE_SECONDS / 60,
        }))),
    }
}

#[tauri::command]
pub async fn remove_workout_fit(
    state: tauri::State<'_, AppState>,
    workout_id: String,
) -> std::result::Result<bool, AppError> {
    with_write(&state.data_dir, &state.db, WritePurpose::Metadata, |db| {
        db.remove_workout_fit(&workout_id)
    })
    .await
}

fn not_fit() -> AppError {
    AppError::new("err.workout_fit.not_fit", "This is not a readable .fit file")
}

fn too_large() -> AppError {
    AppError::new(
        "err.workout_fit.too_large",
        "The file is too large to be a .fit file for one workout",
    )
    .with_params(serde_json::json!({ "maxMb": MAX_FIT_IMPORT_BYTES / 1024 / 1024 }))
}
