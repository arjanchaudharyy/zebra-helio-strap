//! Logbook (subjective layer): shoes, races and medal wall, PB wall, injury log, and how each workout felt.
//! Everything here is written by the user. Writes take only a short write lock. PBs are computed on read.

use super::{spawn_independent_read, with_write};
use crate::app_state::AppState;
use crate::ipc_error::AppError;
use serde::Serialize;
use zeppbridge_core::storage::logbook::annotations::{AnnotationInput, WorkoutFeel};
use zeppbridge_core::storage::logbook::injuries::{Injury, InjuryInput};
use zeppbridge_core::storage::logbook::races::{Race, RaceInput};
use zeppbridge_core::storage::logbook::records::PersonalRecords;
use zeppbridge_core::storage::logbook::shoes::{ShoeInput, ShoeSummary, UnknownGear, GEAR_TAKEN};
use zeppbridge_core::storage::write_lock::WritePurpose;
use zeppbridge_core::storage::Database;

#[derive(Debug, Serialize)]
pub struct ShoeOverview {
    pub shoes: Vec<ShoeSummary>,
    /// Shoe IDs recorded in Zepp that are not registered yet.
    pub unknown_gear: Vec<UnknownGear>,
}

/// A core validation failure uses the generic code `err.core.config`. This maps it to codes the UI knows.
fn relabel(error: AppError) -> AppError {
    if error.code != "err.core.config" {
        return error;
    }
    if error.message == GEAR_TAKEN {
        return AppError::new("err.logbook.gear_taken", error.message);
    }
    AppError::new("err.logbook.invalid", error.message)
}

async fn write<T>(
    state: &tauri::State<'_, AppState>,
    mutation: impl FnOnce(&Database) -> zeppbridge_core::models::error::Result<T>,
) -> Result<T, AppError> {
    with_write(&state.data_dir, &state.db, WritePurpose::Metadata, mutation)
        .await
        .map_err(relabel)
}

#[tauri::command]
pub async fn shoes_overview(state: tauri::State<'_, AppState>) -> Result<ShoeOverview, AppError> {
    spawn_independent_read(state.data_dir.clone(), |db| {
        let (shoes, unknown_gear) = db.shoe_overview()?;
        Ok(ShoeOverview {
            shoes,
            unknown_gear,
        })
    })
    .await
}

#[tauri::command]
pub async fn shoe_save(
    state: tauri::State<'_, AppState>,
    input: ShoeInput,
) -> Result<i64, AppError> {
    write(&state, |db| db.save_shoe(&input)).await
}

#[tauri::command]
pub async fn shoe_delete(state: tauri::State<'_, AppState>, id: i64) -> Result<(), AppError> {
    write(&state, |db| db.delete_shoe(id)).await
}

#[tauri::command]
pub async fn workout_feel_get(
    state: tauri::State<'_, AppState>,
    workout_id: String,
) -> Result<Option<WorkoutFeel>, AppError> {
    spawn_independent_read(state.data_dir.clone(), move |db| {
        db.workout_feel(&workout_id)
    })
    .await
}

#[tauri::command]
pub async fn workout_feel_save(
    state: tauri::State<'_, AppState>,
    input: AnnotationInput,
) -> Result<(), AppError> {
    write(&state, |db| db.save_workout_feel(&input)).await
}

#[tauri::command]
pub async fn injuries_list(state: tauri::State<'_, AppState>) -> Result<Vec<Injury>, AppError> {
    spawn_independent_read(state.data_dir.clone(), |db| db.list_injuries()).await
}

#[tauri::command]
pub async fn injury_save(
    state: tauri::State<'_, AppState>,
    input: InjuryInput,
) -> Result<i64, AppError> {
    write(&state, |db| db.save_injury(&input)).await
}

#[tauri::command]
pub async fn injury_delete(state: tauri::State<'_, AppState>, id: i64) -> Result<(), AppError> {
    write(&state, |db| db.delete_injury(id)).await
}

#[tauri::command]
pub async fn races_list(state: tauri::State<'_, AppState>) -> Result<Vec<Race>, AppError> {
    spawn_independent_read(state.data_dir.clone(), |db| db.list_races()).await
}

#[tauri::command]
pub async fn race_save(
    state: tauri::State<'_, AppState>,
    input: RaceInput,
) -> Result<i64, AppError> {
    write(&state, |db| db.save_race(&input)).await
}

#[tauri::command]
pub async fn race_delete(state: tauri::State<'_, AppState>, id: i64) -> Result<(), AppError> {
    write(&state, |db| db.delete_race(id)).await
}

#[tauri::command]
pub async fn personal_records(
    state: tauri::State<'_, AppState>,
) -> Result<PersonalRecords, AppError> {
    spawn_independent_read(state.data_dir.clone(), |db| db.personal_records()).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validation_failures_reach_the_interface_as_logbook_codes() {
        let gear = relabel(AppError::new("err.core.config", GEAR_TAKEN));
        assert_eq!(gear.code, "err.logbook.gear_taken");
        let other = relabel(AppError::new("err.core.config", "Race details are not valid"));
        assert_eq!(other.code, "err.logbook.invalid");
        let busy = relabel(AppError::new("err.storage.write_busy", "x"));
        assert_eq!(busy.code, "err.storage.write_busy");
    }
}
