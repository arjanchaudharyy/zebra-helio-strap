use super::{spawn_independent_read, spawn_independent_write, with_write};

use crate::app_state::AppState;

use crate::connectors::ZeppConnector;

use crate::device_catalog::{match_catalog, CatalogMatchInput, CatalogMatchStatus};

use crate::export_fit;

use crate::insight::{WeeklyReport, WorkoutInsight};

use crate::ipc_error::AppError;

use crate::ipc_types::CleanupResult;

use crate::models::{
    AiHandoffMetadata, AiHandoffResult, CapabilityOverview, DailyHeartRateExtreme,
    DeviceCacheMetadata, DeviceMatchStatus, DeviceProfile, DeviceProfilesResult, ExportDetail,
    ExportResult, ExportSelection, HealthOverview, HeartRatePoint, HeartRateZoneOptions,
    HeartRateZonePreference, MetricSeries, RawPayloadCompaction, SleepSession, SleepStageSlice,
    StorageEstimate, StressPoint, TrainingBalancePoint, UserPrefs, Workout, WorkoutSeries,
};

use crate::storage::corrections::WorkoutCodeLabel;
use crate::storage::device_local_state::is_hidden_device;

use crate::storage::provenance::{DataHealth, IntegrityCheckResult};

use crate::storage::looks_like_firmware_version;

use chrono::{DateTime, Local, Utc};

use serde::{Deserialize, Serialize};

use serde_json::{Map, Value};

use std::path::{Path, PathBuf};

use std::time::Duration;

use zeppbridge_core::storage::write_lock::WritePurpose;

mod device_parse;
mod device_profiles;
mod export;
mod redact;
mod strength_catalog;
#[cfg(test)]
mod tests;
mod workout_fit;

pub(crate) use device_parse::*;
pub(crate) use device_profiles::*;
pub(crate) use export::*;
pub(crate) use redact::*;
pub(crate) use strength_catalog::*;
pub(crate) use workout_fit::*;

const DEVICE_CACHE_MAX_AGE_SECONDS: i64 = 24 * 60 * 60;

pub(crate) const AI_HANDOFF_INLINE_LIMIT_BYTES: usize = 2 * 1024 * 1024;

/// Return the latest health metrics persisted in the local database.
#[tauri::command]
pub async fn get_health_overview(
    state: tauri::State<'_, AppState>,
) -> std::result::Result<HealthOverview, AppError> {
    spawn_independent_read(state.data_dir.clone(), |db| db.get_health_overview()).await
}

/// What this account can actually give an AI, and what it cannot.
///
/// Read from stored data wherever the library already proves the answer, which
/// is most of it; the rest comes from the last silent capability check that ran
/// during a sync. Nothing here costs a request.
#[tauri::command]
pub async fn get_capability_overview(
    state: tauri::State<'_, AppState>,
) -> std::result::Result<CapabilityOverview, AppError> {
    let today = Local::now().date_naive();
    spawn_independent_read(state.data_dir.clone(), move |db| {
        db.capability_overview(today)
    })
    .await
}

/// One page of records, plus the total count on this device.
///
/// The total is the other half of paging. Without it, the UI can say "showing 500" but not "2317 in
/// total", and users asked exactly "where is the rest?".
/// These are two concrete types, not one generic `Page<T>`: the code that `#[tauri::command]` generates
/// infers the return type, and a generic parameter there falls back to the never type. The resulting
/// error (`!: Deserialize` / never type fallback) does not point here at all.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SleepPage {
    pub items: Vec<SleepSession>,
    /// The total count in the local database. Paging does not change it.
    pub total: i64,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkoutPage {
    pub items: Vec<Workout>,
    pub total: i64,
}

/// The maximum number of records on one page.
///
/// It is the **page size limit**, not "the most records this app lets you see". Before,
/// the `clamp(1, 500)` in `get_recent_*` did both jobs: the SQL had no
/// `OFFSET`, so the app had no way to reach records after number 500.
const MAX_PAGE_SIZE: usize = 500;

/// Return the most recent persisted sleep sessions.
#[tauri::command]
pub async fn get_recent_sleep(
    state: tauri::State<'_, AppState>,
    limit: usize,
) -> std::result::Result<Vec<SleepSession>, AppError> {
    let limit = limit.clamp(1, 500);
    spawn_independent_read(state.data_dir.clone(), move |db| {
        db.get_recent_sleep_sessions(limit)
    })
    .await
}

/// Gets sleep records by page, newest first.
#[tauri::command]
pub async fn get_sleep_page(
    state: tauri::State<'_, AppState>,
    limit: usize,
    offset: usize,
) -> std::result::Result<SleepPage, AppError> {
    let limit = limit.clamp(1, MAX_PAGE_SIZE);
    spawn_independent_read(state.data_dir.clone(), move |db| {
        Ok(SleepPage {
            items: db.sleep_sessions_page(limit, offset)?,
            total: db.count_sleep_sessions()?,
        })
    })
    .await
}

/// Return one persisted sleep session by its stable source identifier.
#[tauri::command]
pub async fn get_sleep_detail(
    state: tauri::State<'_, AppState>,
    sleep_id: String,
) -> std::result::Result<Option<SleepSession>, AppError> {
    spawn_independent_read(state.data_dir.clone(), move |db| {
        db.get_sleep_detail(&sleep_id)
    })
    .await
}

/// The nap stage slices recorded on the same day as this night (official channel only).
#[tauri::command]
pub async fn get_sleep_nap_stages(
    state: tauri::State<'_, AppState>,
    sleep_id: String,
) -> std::result::Result<Vec<SleepStageSlice>, AppError> {
    spawn_independent_read(state.data_dir.clone(), move |db| {
        db.get_sleep_nap_stages(&sleep_id)
    })
    .await
}

/// Return the most recent persisted workouts.
#[tauri::command]
pub async fn get_recent_workouts(
    state: tauri::State<'_, AppState>,
    limit: usize,
) -> std::result::Result<Vec<Workout>, AppError> {
    let limit = limit.clamp(1, 500);
    spawn_independent_read(state.data_dir.clone(), move |db| {
        db.get_recent_workouts(limit)
    })
    .await
}

/// Gets workout records by page, newest first.
#[tauri::command]
pub async fn get_workout_page(
    state: tauri::State<'_, AppState>,
    limit: usize,
    offset: usize,
) -> std::result::Result<WorkoutPage, AppError> {
    let limit = limit.clamp(1, MAX_PAGE_SIZE);
    spawn_independent_read(state.data_dir.clone(), move |db| {
        Ok(WorkoutPage {
            items: db.workouts_page(limit, offset)?,
            total: db.count_workouts()?,
        })
    })
    .await
}

/// Return one persisted workout by its stable source identifier.
#[tauri::command]
pub async fn get_workout_detail(
    state: tauri::State<'_, AppState>,
    workout_id: String,
) -> std::result::Result<Option<Workout>, AppError> {
    spawn_independent_read(state.data_dir.clone(), move |db| {
        db.get_workout_detail(&workout_id)
    })
    .await
}

#[tauri::command]
pub async fn get_workout_series(
    state: tauri::State<'_, AppState>,
    workout_id: String,
) -> std::result::Result<WorkoutSeries, AppError> {
    spawn_independent_read(state.data_dir.clone(), move |db| {
        db.get_workout_series(&workout_id)
    })
    .await
}

#[tauri::command]
pub async fn get_heart_rate_series(
    state: tauri::State<'_, AppState>,
    hours: i64,
) -> std::result::Result<Vec<HeartRatePoint>, AppError> {
    spawn_independent_read(state.data_dir.clone(), move |db| {
        db.heart_rate_series(hours)
    })
    .await
}

#[tauri::command]
pub async fn get_stress_series(
    state: tauri::State<'_, AppState>,
    hours: i64,
) -> std::result::Result<Vec<StressPoint>, AppError> {
    spawn_independent_read(state.data_dir.clone(), move |db| db.stress_series(hours)).await
}

/// Daily series for the body and training screens.
///
/// One round trip fills a whole screen: the caller names the metrics it wants
/// and gets each one back with its unit, its source table and how many days of
/// the window actually carry data.
#[tauri::command]
pub async fn get_metric_series(
    state: tauri::State<'_, AppState>,
    metrics: Vec<String>,
    days: i64,
) -> std::result::Result<Vec<MetricSeries>, AppError> {
    spawn_independent_read(state.data_dir.clone(), move |db| {
        db.metric_series(&metrics, days)
    })
    .await
}

/// The metrics that are "higher / lower than usual" (compared only with the user's own last 28 days; metrics without enough data are not returned).
#[tauri::command]
pub async fn get_metric_baselines(
    state: tauri::State<'_, AppState>,
    metrics: Vec<String>,
) -> std::result::Result<Vec<zeppbridge_core::insight::anomaly::MetricBaseline>, AppError> {
    let today = chrono::Local::now().date_naive();
    // Metrics the user excludes (metric_prefs) are not in the "than usual" comparison.
    spawn_independent_read(state.data_dir.clone(), move |db| {
        let excluded = db.excluded_metrics()?;
        let metrics: Vec<String> = metrics
            .into_iter()
            .filter(|metric| !excluded.contains(metric))
            .collect();
        db.metric_baselines(&metrics, today)
    })
    .await
}

/// Metrics the user excludes (not compared, not sent to AI / MCP; the data is not deleted).
#[tauri::command]
pub async fn metric_exclusions(
    state: tauri::State<'_, AppState>,
) -> std::result::Result<Vec<String>, AppError> {
    spawn_independent_read(state.data_dir.clone(), move |db| db.excluded_metrics()).await
}

/// Excludes or restores a metric. Returns the new list.
#[tauri::command]
pub async fn set_metric_excluded(
    state: tauri::State<'_, AppState>,
    metric: String,
    excluded: bool,
) -> std::result::Result<Vec<String>, AppError> {
    crate::commands::with_write(
        &state.data_dir,
        &state.db,
        zeppbridge_core::storage::write_lock::WritePurpose::Metadata,
        move |db| db.set_metric_excluded(&metric, excluded),
    )
    .await
}

/// Hourly steps for each local day in a range, both ends included (only with Zepp official sign-in). An empty array when there is no official data.
#[tauri::command]
pub async fn get_hourly_steps(
    state: tauri::State<'_, AppState>,
    start: String,
    end: String,
) -> std::result::Result<Vec<zeppbridge_core::models::HourlySteps>, AppError> {
    spawn_independent_read(state.data_dir.clone(), move |db| {
        db.hourly_steps(&start, &end)
    })
    .await
}

/// Raw daily heart rate extremes.
///
/// The daily maximum heart rate in the Zepp App is **filtered** (a user reported that the App showed 104, but the raw
/// data peaked above 120). This command gives the unfiltered daily max of the local raw samples,
/// with the sample count for each day. On a day with few samples, that "maximum" is only the highest
/// of those few points.
#[tauri::command]
pub async fn get_daily_heart_rate_extremes(
    state: tauri::State<'_, AppState>,
    days: i64,
) -> std::result::Result<Vec<DailyHeartRateExtreme>, AppError> {
    spawn_independent_read(state.data_dir.clone(), move |db| {
        db.daily_heart_rate_extremes(days)
    })
    .await
}

/// Acute (7 day) versus chronic (28 day) training load, day by day.
#[tauri::command]
pub async fn get_training_balance(
    state: tauri::State<'_, AppState>,
    days: i64,
) -> std::result::Result<Vec<TrainingBalancePoint>, AppError> {
    let window = days.clamp(1, 1825);
    let end = chrono::Local::now().date_naive();
    let start = end - chrono::Duration::days(window - 1);
    spawn_independent_read(state.data_dir.clone(), move |db| {
        db.training_load_balance(start, end)
    })
    .await
}

/// The heart-rate zone picker's state: measured bases, the models they
/// support, the user's choice and the zones that choice produces.
#[tauri::command]
pub async fn get_heart_rate_zones(
    state: tauri::State<'_, AppState>,
    days: i64,
) -> std::result::Result<HeartRateZoneOptions, AppError> {
    spawn_independent_read(state.data_dir.clone(), move |db| {
        db.heart_rate_zone_options(days)
    })
    .await
}

/// Record which zone model and which measured bases the user picked.
///
/// Every field is optional and clearing them all is a valid state: nothing
/// here is chosen on the user's behalf, so "not decided yet" has to survive a
/// round trip.
#[tauri::command]
pub async fn set_heart_rate_zone_preference(
    state: tauri::State<'_, AppState>,
    model: Option<String>,
    max_basis: Option<String>,
    resting_basis: Option<String>,
    threshold_basis: Option<String>,
    days: i64,
) -> std::result::Result<HeartRateZoneOptions, AppError> {
    with_write(&state.data_dir, &state.db, WritePurpose::Metadata, |db| {
        db.set_heart_rate_zone_preference(&HeartRateZonePreference {
            model,
            max_basis,
            resting_basis,
            threshold_basis,
        })?;
        db.heart_rate_zone_options(days)
    })
    .await
}

#[tauri::command]
pub async fn get_storage_estimate(
    state: tauri::State<'_, AppState>,
    days: i64,
) -> std::result::Result<StorageEstimate, AppError> {
    let data_dir = state.data_dir.clone();
    spawn_independent_read(state.data_dir.clone(), move |db| {
        db.storage_estimate(days, &data_dir)
    })
    .await
}

/// The current retention / backfill / archive preferences.
///
/// `AppStatus` has only the retention period and the backfill window, not the archive switch. The UI needs a separate
/// way to read the full preferences, or the archive panel can only guess.
#[tauri::command]
pub async fn get_user_prefs(
    state: tauri::State<'_, AppState>,
) -> std::result::Result<UserPrefs, AppError> {
    spawn_independent_read(state.data_dir.clone(), |db| db.user_prefs()).await
}

#[tauri::command]
pub async fn set_user_prefs(
    state: tauri::State<'_, AppState>,
    retention_days: i64,
    history_sync_days: i64,
    archive_enabled: Option<bool>,
) -> std::result::Result<UserPrefs, AppError> {
    with_write(&state.data_dir, &state.db, WritePurpose::Metadata, |db| {
        // Old callers that do not pass the archive switch keep the current value. Archiving is not turned off silently.
        let archive_enabled = match archive_enabled {
            Some(value) => value,
            None => db
                .user_prefs()
                .map(|prefs| prefs.archive_enabled)
                .unwrap_or(false),
        };
        db.set_user_prefs(&UserPrefs {
            demo_mode: db.is_demo_library()?,
            ai_profile_note: db.ai_profile_note()?,
            retention_days,
            history_sync_days,
            archive_enabled,
        })
    })
    .await
}

/// Remove records older than the requested retention window.
/// Compresses the stored raw responses.
///
/// This is a separate command that the user starts with one click, not part of sync: an old database can have thousands of responses
/// and more than 1 GB of text, and compression must read and write all of it. In sync, a quick "is there new
/// data?" check would become a wait of several minutes.
#[tauri::command]
pub async fn compact_raw_payloads(
    state: tauri::State<'_, AppState>,
) -> std::result::Result<RawPayloadCompaction, AppError> {
    spawn_independent_write(state.data_dir.clone(), WritePurpose::Compaction, |db| {
        db.compact_raw_payloads()
    })
    .await
}

#[tauri::command]
pub async fn cleanup_old_data(
    state: tauri::State<'_, AppState>,
    days: i64,
) -> std::result::Result<CleanupResult, AppError> {
    if !(1..=365).contains(&days) {
        return Err(AppError::new(
            "err.prefs.retention_out_of_range",
            "Retention must be between 1 and 365 days",
        ));
    }

    spawn_independent_write(state.data_dir.clone(), WritePurpose::Cleanup, move |db| {
        db.cleanup_old_data(days)
    })
    .await?;

    Ok(CleanupResult {
        days,
        message: Some(format!("Cleared data older than {} days", days)),
    })
}

#[tauri::command]
pub async fn reprocess_local_data(
    state: tauri::State<'_, AppState>,
) -> std::result::Result<serde_json::Value, AppError> {
    let streams = spawn_independent_write(state.data_dir.clone(), WritePurpose::Reprocess, |db| {
        let streams = db.reprocess_raw_records()?;
        // A manual reparse is recorded on its own timeline. The cloud sync time does not change.
        db.record_local_replay(true)?;
        Ok(streams)
    })
    .await?;
    let total_records: i64 = streams.values().sum();
    Ok(serde_json::json!({
        "total_records": total_records,
        "streams": streams,
        "message": "Reprocessed the local raw responses with the new parser"
    }))
}

/// Deterministic insights for one workout.
///
/// The backend gives only traceable facts, comparisons and evidence, and no natural language text. The UI owns the wording.
/// AI can only explain these facts, not rewrite them.
#[tauri::command]
pub async fn get_workout_insight(
    state: tauri::State<'_, AppState>,
    workout_id: String,
) -> std::result::Result<WorkoutInsight, AppError> {
    spawn_independent_read(state.data_dir.clone(), move |db| {
        db.workout_insight(&workout_id)
    })
    .await
}

/// Local weekly report: the last 7 days compared with your own previous 28 days.
///
/// No comparison with any population baseline: the project has no population data and has no plan to get it.
#[tauri::command]
pub async fn get_weekly_report(
    state: tauri::State<'_, AppState>,
) -> std::result::Result<WeeklyReport, AppError> {
    let now = Local::now();
    spawn_independent_read(state.data_dir.clone(), move |db| db.weekly_report(now)).await
}

/// The backend contract for the data health center.
///
/// This call does not use the network or run `integrity_check`: opening the page must be cheap. The integrity
/// check is an explicit action, see `run_database_integrity_check`.
#[tauri::command]
pub async fn get_data_health(
    state: tauri::State<'_, AppState>,
    window_days: Option<i64>,
) -> std::result::Result<DataHealth, AppError> {
    let database_bytes = std::fs::metadata(state.data_dir.join("zepp.db"))
        .map(|meta| meta.len())
        .unwrap_or(0);
    let window_days = window_days.unwrap_or(90);
    spawn_independent_read(state.data_dir.clone(), move |db| {
        db.data_health(window_days, database_bytes)
    })
    .await
}

/// Runs SQLite `integrity_check` on the whole database and records the result.
///
/// On a large database this is a full scan, so it runs only when the user clicks. The page usually shows the last
/// result and its time.
#[tauri::command]
pub async fn run_database_integrity_check(
    state: tauri::State<'_, AppState>,
) -> std::result::Result<IntegrityCheckResult, AppError> {
    spawn_independent_write(state.data_dir.clone(), WritePurpose::Metadata, |db| {
        db.run_integrity_check()
    })
    .await
}

/// All options in the bundled workout catalog, for the correction dropdown.
///
/// `include_str!` compiles the catalog into the binary, so this list always matches the values the backend allows.
/// The UI does not need its own copy that can drift.
#[tauri::command]
pub fn get_workout_type_options() -> Vec<zeppbridge_core::sport_catalog::SportOption> {
    zeppbridge_core::sport_catalog::options().to_vec()
}

/// All Zepp workout type codes on this device that have no name yet.
///
/// Zepp custom workout templates give codes that are not in the catalog (12 and 226 in real feedback).
/// We have no evidence of which workout these codes are, so we do not guess. We give them to the user
/// with the number of affected records, and the user names each one once.
#[tauri::command]
pub async fn get_unknown_workout_codes(
    state: tauri::State<'_, AppState>,
) -> std::result::Result<Vec<WorkoutCodeLabel>, AppError> {
    spawn_independent_read(state.data_dir.clone(), |db| {
        db.unknown_workout_code_labels()
    })
    .await
}

/// Names an unknown code (pass `null` to undo). It applies to all records with the same code.
#[tauri::command]
pub async fn set_workout_code_label(
    state: tauri::State<'_, AppState>,
    zepp_type: i32,
    label: Option<String>,
) -> std::result::Result<Vec<WorkoutCodeLabel>, AppError> {
    with_write(&state.data_dir, &state.db, WritePurpose::Metadata, |db| {
        db.set_workout_code_label(zepp_type, label.as_deref())?;
        db.unknown_workout_code_labels()
    })
    .await
}

/// The user picks the model of a device (pass `null` to undo).
///
/// This is not an identification result. It is a user correction: `match_status` is `user_assigned`, and the UI must
/// label it honestly, not show it as automatic identification.
#[tauri::command]
pub async fn set_device_model_override(
    state: tauri::State<'_, AppState>,
    device_key: String,
    catalog_id: Option<String>,
) -> std::result::Result<(), AppError> {
    with_write(&state.data_dir, &state.db, WritePurpose::Metadata, |db| {
        db.set_device_model_override(&device_key, catalog_id.as_deref())
    })
    .await
}

#[tauri::command]
pub async fn set_workout_type_override(
    state: tauri::State<'_, AppState>,
    workout_id: String,
    user_override: Option<String>,
) -> std::result::Result<Workout, AppError> {
    let workout = with_write(&state.data_dir, &state.db, WritePurpose::Metadata, |db| {
        db.set_workout_type_override(&workout_id, user_override.as_deref())?;
        db.get_workout_detail(&workout_id)
    })
    .await?;
    workout.ok_or_else(|| AppError::new("err.workout.not_found", "Workout not found"))
}

#[tauri::command]
pub async fn edit_workouts(
    state: tauri::State<'_, AppState>,
    workout_ids: Vec<String>,
    change_type: bool,
    workout_type: Option<String>,
    hidden: Option<bool>,
) -> std::result::Result<zeppbridge_core::storage::workout_edits::WorkoutEdit, AppError> {
    with_write(&state.data_dir, &state.db, WritePurpose::Metadata, |db| {
        db.edit_workouts(
            &workout_ids,
            change_type.then_some(workout_type.as_deref()),
            hidden,
        )
    })
    .await
}

#[tauri::command]
pub async fn undo_workout_edit(
    state: tauri::State<'_, AppState>,
    id: String,
) -> std::result::Result<usize, AppError> {
    with_write(&state.data_dir, &state.db, WritePurpose::Metadata, |db| {
        db.undo_workout_edit(&id)
    })
    .await
}

#[tauri::command]
pub async fn get_hidden_workout_page(
    state: tauri::State<'_, AppState>,
    limit: usize,
    offset: usize,
) -> std::result::Result<WorkoutPage, AppError> {
    spawn_independent_read(state.data_dir.clone(), move |db| {
        Ok(WorkoutPage {
            items: db.hidden_workouts_page(limit.clamp(1, MAX_PAGE_SIZE), offset)?,
            total: db.count_hidden_workouts()?,
        })
    })
    .await
}

/// Opens an https link in the system browser (opens the AI website during an AI handoff).
///
/// The frontend does not call the opener plugin directly: in an AppImage the browser inherits the injected library path and does not start,
/// so that path must be removed first (external_open.rs). Only https is accepted, and the frontend has a fixed allowlist of AI sites.
/// On Linux it waits for the opener's exit code, so it does not run on the main thread (`async`).
#[tauri::command(async)]
pub fn open_external_url(app: tauri::AppHandle, url: String) -> std::result::Result<(), AppError> {
    let parsed = tauri::Url::parse(&url)
        .map_err(|_| AppError::new("err.open_url.not_allowed", "Only https links can be opened"))?;
    if parsed.scheme() != "https" || parsed.host_str().is_none() || !parsed.username().is_empty() {
        return Err(AppError::new(
            "err.open_url.not_allowed",
            "Only https links can be opened",
        ));
    }
    crate::external_open::open_url(&app, parsed.as_str()).map_err(|_| {
        AppError::new(
            "err.open_url.failed",
            "Could not open the system browser. Check the default browser setting and try again",
        )
    })
}

/// Open the application's local data directory in the platform file manager.
/// Off the main thread: on Linux it waits for the opener's exit code.
#[tauri::command(async)]
pub fn open_data_folder(state: tauri::State<'_, AppState>) -> std::result::Result<(), AppError> {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(&state.data_dir)
            .spawn()
            .map(|_| ())
            .map_err(|error| {
                AppError::new(
                    "err.data_folder.open_failed",
                    format!("Could not open the data folder: {error}"),
                )
            })
    }

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&state.data_dir)
            .spawn()
            .map(|_| ())
            .map_err(|error| {
                AppError::new(
                    "err.data_folder.open_failed",
                    format!("Could not open the data folder: {error}"),
                )
            })
    }

    // On Linux the project already has a lot of support for WebKitGTK, Flatpak, Secret Service and AppImage,
    // but this one settings button returned "not supported". `xdg-open` is the standard desktop
    // entry point, and all four channels (deb / rpm / Flatpak / AppImage) have it.
    #[cfg(target_os = "linux")]
    {
        crate::external_open::open_folder(&state.data_dir).map_err(|error| {
            AppError::new(
                "err.data_folder.open_failed",
                format!("Could not open the data folder: {error}"),
            )
        })
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        let _ = state;
        Err(AppError::new(
            "err.data_folder.unsupported_os",
            "Opening the data folder is supported only on Windows/macOS/Linux",
        ))
    }
}
