use chrono::{DateTime, Utc};
use tauri::{AppHandle, Emitter};

use crate::app_state::AppState;
use crate::ipc_error::AppError;
use crate::ipc_types::{ui_sync_report, UiSyncReport};
use crate::models::{CapabilityProbe, UserPrefs};
use crate::storage::coverage::CoverageLedger;
use crate::sync::{
    probe_since, CloudProbe, OfficialMode, OfficialSync, StreamStatus, SyncManager, SyncProgress,
    SyncReport,
};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use zeppbridge_core::official::OfficialStore;
use zeppbridge_core::storage::write_lock::{self, WritePurpose};
use zeppbridge_core::storage::Database;

use super::with_write;

/// Run the first 30-day sync and return per-stream progress to the UI.
///
/// The manager is taken after `sync_command_lock` so a concurrent save/clear
/// cannot leave this command writing with a credential that was just replaced.
/// A report with failed streams remains a successful IPC response so the UI
/// can render each stream's actual state; only an underlying
/// transport/database error is returned as `Err`.
pub async fn start_initial_sync(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    days: Option<i64>,
) -> std::result::Result<UiSyncReport, AppError> {
    let days = match days {
        Some(value) => UserPrefs::clamp_days(value)
            .map_err(|message| AppError::new("err.sync.history_days_out_of_range", message))?,
        None => {
            let database = state.db.lock().await;
            database
                .user_prefs()
                .map(|prefs| prefs.history_sync_days)
                .unwrap_or(UserPrefs::DEFAULT_HISTORY_SYNC_DAYS)
        }
    };
    run_sync(&app, &state, SyncWindow::History(days), false).await
}

#[tauri::command]
pub async fn start_history_sync(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    days: i64,
) -> std::result::Result<UiSyncReport, AppError> {
    start_initial_sync(app, state, Some(days)).await
}

/// Run the overlap-window incremental sync and return per-stream progress.
#[tauri::command]
/// `quick`: a silent scheduled sync. It fetches only the last few days, but uses the full window when a full refresh is due
/// (see `SyncManager::quick_sync_report_with_progress`). Syncs that the user starts or that run at startup
/// do not set it and always use the full window.
///
/// `skip_probe`: do not first probe the cloud for new data. "Try again" in the "no new cloud data yet" notice sets it:
/// the user just pulled to refresh on the phone and wants a real sync, so the probe must not block it again.
pub async fn start_incremental_sync(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    quick: Option<bool>,
    skip_probe: Option<bool>,
) -> std::result::Result<UiSyncReport, AppError> {
    if state.auth_state.read().await.as_str() != "verified" && !official_connected(&state.data_dir)
    {
        return Err(AppError::new(
            "err.sync.not_verified",
            "Complete the connection check before you sync recent data",
        ));
    }
    let window = if quick.unwrap_or(false) {
        SyncWindow::Quick
    } else {
        SyncWindow::Incremental
    };
    run_sync(&app, &state, window, !skip_probe.unwrap_or(false)).await
}

/// A scheduled sync from the backend timer (`auto_sync.rs`, when the window is not visible). It is the same path as the frontend calling
/// `start_incremental_sync` with `quick`: it probes the cloud first and sends progress events the same way.
pub(crate) async fn background_quick_sync(
    app: &AppHandle,
    state: &AppState,
) -> std::result::Result<UiSyncReport, AppError> {
    if state.auth_state.read().await.as_str() != "verified" && !official_connected(&state.data_dir)
    {
        return Err(AppError::new(
            "err.sync.not_verified",
            "Complete the connection check before you sync recent data",
        ));
    }
    run_sync(app, state, SyncWindow::Quick, true).await
}

/// How far back one sync fetches.
enum SyncWindow {
    /// The normal full window (`INCREMENTAL_SYNC_DAYS`).
    Incremental,
    /// Scheduled sync: the last few days, or the full window when a full refresh is due.
    Quick,
    /// History sync: the number of days the user sets.
    History(i64),
}

/// Probe the optional Zepp event streams and report what answers.
///
/// This exists because "another tool can read HRV, so ZeppBridge should too"
/// is not a fact about *this* account: stream availability varies by device
/// and region, and the endpoint offers no discovery call. The probe makes a
/// handful of requests and reports status plus field names. It saves only
/// probe metadata so the capability board can reflect the same result.
#[tauri::command]
pub async fn probe_data_capabilities(
    state: tauri::State<'_, AppState>,
) -> std::result::Result<Vec<CapabilityProbe>, AppError> {
    let _command_guard = state.sync_command_lock.lock().await;
    if state.auth_state.read().await.as_str() != "verified" {
        return Err(AppError::new(
            "err.sync.not_verified_probe",
            "Complete the connection check before you probe data capabilities",
        ));
    }
    let manager = require_manager(&state).await?;
    manager.probe_capabilities().await.map_err(AppError::from)
}

/// Full history backfill.
///
/// This is different from a normal sync: it splits work into calendar months, records each block in a ledger, can stop and resume, and **does no cleanup**.
/// Each call processes a limited number of blocks and returns the ledger. The UI uses it to show progress and decide whether to continue,
/// so a backfill of several years does not become one long task that cannot be cancelled.
#[tauri::command]
pub async fn start_history_backfill(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    from_date: String,
    max_chunks: Option<usize>,
) -> std::result::Result<CoverageLedger, AppError> {
    if state.auth_state.read().await.as_str() != "verified" {
        return Err(AppError::new(
            "err.sync.not_verified_backfill",
            "Complete the connection check before you backfill history",
        ));
    }
    let from = chrono::NaiveDate::parse_from_str(from_date.trim(), "%Y-%m-%d").map_err(|_| {
        AppError::new(
            "err.backfill.bad_start_date",
            "The backfill start date is not valid. Use YYYY-MM-DD",
        )
    })?;
    let to = Utc::now().date_naive();
    if from > to {
        return Err(AppError::new(
            "err.backfill.start_in_future",
            "The backfill start date cannot be later than today",
        ));
    }
    // Without a lower limit, a very early start (even a typo) makes `plan_backfill`
    // expand six streams by calendar month into hundreds of thousands of sync blocks, and the whole app hangs (while it holds the write lock).
    // As with all other time windows, the limit is `MAX_HISTORY_SYNC_DAYS`: Zepp has no
    // records from before that.
    // This is a different case from "wrong date format" and has its own code: once "All" went back 3652 days
    // by calendar year, hit this check, and showed "Use YYYY-MM-DD", which users could not understand.
    if (to - from).num_days() > UserPrefs::MAX_HISTORY_SYNC_DAYS {
        return Err(AppError::new(
            "err.backfill.start_too_early",
            format!(
                "The backfill start is too early: you can backfill only the last {} days",
                UserPrefs::MAX_HISTORY_SYNC_DAYS
            ),
        ));
    }
    let _command_guard = state.sync_command_lock.lock().await;
    if let Some(kind) = local_maintenance_deferred() {
        return Err(deferred_app_error(kind));
    }
    let manager = require_manager(&state).await?;
    begin_progress_epoch();
    match manager
        .history_backfill(from, to, max_chunks.unwrap_or(24), |progress| {
            emit_sync_progress(&app, progress)
        })
        .await
    {
        Ok(ledger) => Ok(ledger),
        Err(error) if error.is_busy() => Err(deferred_app_error(busy_deferred_kind())),
        Err(error) => {
            if error.needs_reauth() {
                *state.auth_state.write().await = "needs_reauth".to_string();
            }
            Err(error.into())
        }
    }
}

/// The current history coverage ledger.
#[tauri::command]
pub async fn get_coverage_ledger(
    state: tauri::State<'_, AppState>,
) -> std::result::Result<CoverageLedger, AppError> {
    // The UI reads this again and again during a backfill. Use the separate read-only connection, so it does not compete with sync finalization for the command-side lock.
    super::spawn_independent_read(state.data_dir.clone(), |db| db.coverage_ledger()).await
}

#[tauri::command]
pub async fn get_workout_detail_coverage(
    state: tauri::State<'_, AppState>,
    channel: Option<String>,
) -> std::result::Result<zeppbridge_core::storage::detail_queue::DetailCoverage, AppError> {
    let official = detail_channel(channel.as_deref())?;
    super::spawn_independent_read(state.data_dir.clone(), move |db| {
        db.workout_detail_coverage_for_channel(official)
    })
    .await
}

fn detail_channel(channel: Option<&str>) -> std::result::Result<Option<bool>, AppError> {
    match channel {
        None => Ok(None),
        Some("advanced") => Ok(Some(false)),
        Some("official") => Ok(Some(true)),
        Some(_) => Err(AppError::new(
            "err.core.config",
            "The detail channel must be advanced or official",
        )),
    }
}

#[tauri::command]
pub async fn start_workout_detail_backfill(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    max_records: Option<usize>,
    retry_failed: Option<bool>,
    channel: Option<String>,
) -> std::result::Result<zeppbridge_core::storage::detail_queue::DetailCoverage, AppError> {
    let _command = state
        .sync_command_lock
        .try_lock()
        .map_err(|_| deferred_app_error(busy_deferred_kind()))?;
    let requested = detail_channel(channel.as_deref())?;
    if let Some(kind) = local_maintenance_deferred() {
        return Err(deferred_app_error(kind));
    }
    let limit = max_records.unwrap_or(20).clamp(1, 40);
    let legacy = if state.auth_state.read().await.as_str() == "verified" {
        state.sync.read().await.clone()
    } else {
        None
    };
    let official = official_connected(&state.data_dir);
    let use_official = requested.unwrap_or(legacy.is_none());
    if (use_official && !official) || (!use_official && legacy.is_none()) {
        return Err(AppError::new(
            "err.sync.not_verified_backfill",
            "Complete the connection check before you backfill history",
        ));
    }
    begin_progress_epoch();
    if !use_official {
        return legacy
            .expect("available advanced manager checked above")
            .history_detail_backfill(limit, retry_failed.unwrap_or(false), |p| {
                emit_sync_progress(&app, p)
            })
            .await
            .map_err(AppError::from);
    } else {
        let db = Database::open_without_migration(state.data_dir.join("zepp.db"))?;
        let zone = db
            .device_time_zone()?
            .unwrap_or_else(zeppbridge_core::official::fetch::system_time_zone);
        let cancel = Arc::new(AtomicBool::new(false));
        let sync = OfficialSync::new(&state.data_dir, db, cancel.clone(), zone)?;
        if let Ok(mut slot) = OFFICIAL_CANCEL.lock() {
            *slot = Some(cancel);
        }
        let result = sync
            .history_detail_backfill(limit, retry_failed.unwrap_or(false))
            .await;
        if let Ok(mut slot) = OFFICIAL_CANCEL.lock() {
            *slot = None;
        }
        match result {
            Ok(_) => {}
            Err(error) if error.is_cancelled() => {}
            Err(error) => return Err(error.into()),
        }
    }
    super::spawn_independent_read(state.data_dir.clone(), |db| {
        db.workout_detail_coverage_for_channel(Some(true))
    })
    .await
}

/// Clears the ledger and plans a new backfill.
///
/// Clears only the ledger. It deletes no data already written to the local database.
#[tauri::command]
pub async fn reset_coverage_ledger(
    state: tauri::State<'_, AppState>,
) -> std::result::Result<CoverageLedger, AppError> {
    let _command_guard = state.sync_command_lock.lock().await;
    with_write(&state.data_dir, &state.db, WritePurpose::Metadata, |db| {
        db.reset_coverage_ledger()?;
        db.coverage_ledger()
    })
    .await
}

/// Puts failed blocks back into the automatic backfill queue.
///
/// The difference from "Clear ledger" is important: this action touches only `failed`. Blocks that are written, or that the cloud
/// confirmed as empty, stay as they are. The previous version forced users to clear the whole ledger and fetch years of
/// history again just to retry one failed month. That must not happen again.
#[tauri::command]
pub async fn retry_failed_backfill_chunks(
    state: tauri::State<'_, AppState>,
) -> std::result::Result<CoverageLedger, AppError> {
    let _command_guard = state.sync_command_lock.lock().await;
    with_write(&state.data_dir, &state.db, WritePurpose::Metadata, |db| {
        db.reset_failed_backfill_chunks()?;
        db.coverage_ledger()
    })
    .await
}

#[tauri::command]
pub async fn cancel_sync(state: tauri::State<'_, AppState>) -> std::result::Result<(), AppError> {
    if let Some(manager) = state.sync.read().await.clone() {
        manager.request_cancel();
    }
    if let Ok(current) = OFFICIAL_CANCEL.lock() {
        if let Some(flag) = current.as_ref() {
            flag.store(true, Ordering::SeqCst);
        }
    }
    Ok(())
}

/// The cancel flag of the official sync in progress (sync commands run one after another, so only one runs at a time).
static OFFICIAL_CANCEL: std::sync::Mutex<Option<Arc<AtomicBool>>> = std::sync::Mutex::new(None);

/// Whether official sign-in is active (the token exists and is not marked as needing a new authorization).
pub(crate) fn official_connected(data_dir: &std::path::Path) -> bool {
    OfficialStore::new(data_dir)
        .meta()
        .ok()
        .flatten()
        .is_some_and(|meta| !meta.needs_reauth)
}

/// Runs one official sync. The window days follow the same rules as the legacy channel.
async fn run_official(
    state: &AppState,
    window: &SyncWindow,
    mode: OfficialMode,
    on_progress: &(dyn Fn(SyncProgress) + Send + Sync),
) -> zeppbridge_core::models::error::Result<SyncReport> {
    let db = Database::open_without_migration(state.data_dir.join("zepp.db"))?;
    let days = match window {
        SyncWindow::History(days) => *days,
        SyncWindow::Incremental => zeppbridge_core::contract::INCREMENTAL_SYNC_DAYS,
        SyncWindow::Quick => {
            zeppbridge_core::sync::quick_window_days(db.full_window_refresh_due(Utc::now())?)
        }
    };
    // Use the time zone the watch reported. If there is none (official sign-in only), use this computer's time zone.
    let time_zone = db
        .device_time_zone()
        .ok()
        .flatten()
        .unwrap_or_else(zeppbridge_core::official::fetch::system_time_zone);
    let cancel = Arc::new(AtomicBool::new(false));
    if let Ok(mut current) = OFFICIAL_CANCEL.lock() {
        *current = Some(cancel.clone());
    }
    let sync = OfficialSync::new(&state.data_dir, db, cancel, time_zone)?;
    let result = sync.run(days, mode, on_progress).await;
    if let Ok(mut current) = OFFICIAL_CANCEL.lock() {
        *current = None;
    }
    result
}

async fn require_manager(state: &AppState) -> std::result::Result<Arc<SyncManager>, AppError> {
    state
        .sync
        .read()
        .await
        .clone()
        .ok_or_else(|| AppError::new("err.sync.not_connected", "Zepp is not connected. Connect it first"))
}

/// Each sync writes two lines to the diagnostics log: the start (which window, probe or not) and the result (deferred / cloud not new / records written / error code).
/// Before, there were none: when a user said "sync does nothing", the log did not show whether sync started or why it stopped.
/// It logs only the result and counts, never health data, tokens or request content.
async fn run_sync(
    app: &AppHandle,
    state: &AppState,
    window: SyncWindow,
    probe: bool,
) -> std::result::Result<UiSyncReport, AppError> {
    let label = match &window {
        SyncWindow::Incremental => "incremental".to_string(),
        SyncWindow::Quick => "quick".to_string(),
        SyncWindow::History(days) => format!("history {days}d"),
    };
    crate::diagnostics::log(&format!("Sync started: {label}, probe={probe}"));
    let started = std::time::Instant::now();
    let result = run_sync_locked(app, state, window, probe).await;
    let took = started.elapsed().as_secs_f64();
    match &result {
        Ok(report) => crate::diagnostics::log(&format!(
            "Sync finished: {label}, outcome={}{}, records={}, in {took:.1}s",
            report.outcome,
            report
                .message_code
                .as_deref()
                .map(|code| format!(" ({code})"))
                .unwrap_or_default(),
            report.total_records,
        )),
        Err(error) => crate::diagnostics::log(&format!(
            "Sync failed: {label}, code={}, in {took:.1}s",
            error.code
        )),
    }
    result
}

async fn run_sync_locked(
    app: &AppHandle,
    state: &AppState,
    window: SyncWindow,
    probe: bool,
) -> std::result::Result<UiSyncReport, AppError> {
    let _command_guard = state.sync_command_lock.lock().await;
    // Re-read after the lock: save/clear may have swapped the manager while
    // this command waited, and the handle cloned beforehand would keep writing
    // with the old credential.
    //
    // The legacy channel (advanced data) and Zepp official sign-in can be linked one at a time. With only official, the whole round uses official.
    // With both, the legacy channel runs first, then the items only official has (core::sync::official).
    let manager = state.sync.read().await.clone();
    let official = official_connected(&state.data_dir);
    if manager.is_none() && !official {
        return Err(AppError::new(
            "err.sync.not_connected",
            "Zepp is not connected. Connect it first",
        ));
    }
    // A `NORMALIZER_REVISION` bump makes the next launch replay every stored
    // raw payload, which writes in bulk for as long as a quarter of an hour on
    // a large library. A sync starting in the middle of that used to lose the
    // race for SQLite's write lock and surface as "workouts failed: local database
    // temporarily unavailable". That is alarming wording for a library that is busy healing
    // itself and has lost nothing. Standing aside and coming back is both
    // truthful and what the user would want.
    // The first launch after an update compresses the stored responses in the background, and the app also syncs automatically
    // at startup. Both start at once, the sync cannot get the write lock, and the user sees a red line
    // "Another write is in progress". Compression is normal one-time maintenance that we schedule,
    // and it must not alarm the user. As with replay, stand aside and retry.
    if let Some(kind) = local_maintenance_deferred() {
        return Ok(deferred_ui_report(kind));
    }
    let (before, full_refresh_due) = {
        let database = state.db.lock().await;
        // If it cannot be read, treat it as due: an extra full window is better than a sync the probe blocks.
        let due = database.full_window_refresh_due(Utc::now()).unwrap_or(true);
        (database.newest_samples()?, due)
    };
    // Before sync, probe the cloud for new data. If the phone has not uploaded the watch's new data yet, do not run all streams (10 to 60 seconds).
    // Tell the user to pull to refresh on the phone. Probe only for quick / incremental syncs. If the probe gives no result, sync as usual.
    // When nothing is new, do not write the database or record this round's cloud sync time: nothing was fetched.
    if let Some(since) = probe
        .then(|| probe_window(&window, &before, full_refresh_due))
        .flatten()
    {
        let probe = match &manager {
            Some(manager) => manager.probe_cloud(since).await,
            None => official_probe(state, since).await,
        };
        if let CloudProbe::Stale { cloud_latest_at } = probe {
            return Ok(cloud_stale_ui_report(cloud_latest_at));
        }
    }
    let started_at = Utc::now().to_rfc3339();
    let epoch_at_start = crate::storage::content_epoch();
    begin_progress_epoch();
    let on_progress = |progress| emit_sync_progress(app, progress);
    let report_result = match &manager {
        Some(manager) => match window {
            SyncWindow::History(days) => {
                manager
                    .history_sync_report_with_progress(days, on_progress)
                    .await
            }
            SyncWindow::Quick => manager.quick_sync_report_with_progress(on_progress).await,
            SyncWindow::Incremental => {
                manager
                    .incremental_sync_report_with_progress(on_progress)
                    .await
            }
        },
        None => run_official(state, &window, OfficialMode::Only, &on_progress).await,
    };
    // Both are linked: if the legacy channel had no major error this round, add official sleep and hourly steps. A failure here is only logged
    // and does not change this round's result, because the legacy data is already stored.
    if manager.is_some() && official && report_result.is_ok() {
        if let Err(error) =
            run_official(state, &window, OfficialMode::Supplement, &on_progress).await
        {
            eprintln!("Official supplementary sync did not complete: {error}");
        }
    }
    let finished_at = Utc::now().to_rfc3339();
    let report = match report_result {
        Ok(report) => report,
        Err(error) if error.is_busy() => {
            return Ok(deferred_ui_report(busy_deferred_kind()));
        }
        Err(error) if error.is_cancelled() => {
            // A user-initiated cancellation is a deliberate terminal outcome,
            // not a failure: report it as `cancelled` so the UI can show a
            // neutral banner instead of a red error.
            record_cloud_sync_locked(state, &finished_at, "cancelled", 0).await;
            return Ok(ui_sync_report(
                SyncReport {
                    cleanup_failed: false,
                    success: false,
                    core_ok: false,
                    streams: Vec::new(),
                    records_written: 0,
                    message: Some("Sync cancelled".into()),
                },
                started_at,
                finished_at,
                "cancelled".to_string(),
                &BTreeMap::new(),
            ));
        }
        Err(error) => {
            record_cloud_sync_locked(state, &finished_at, "failed", 0).await;
            // With only official sign-in, the token failure is already recorded in official.json. The legacy channel state does not change.
            if error.needs_reauth() && manager.is_some() {
                *state.auth_state.write().await = "needs_reauth".to_string();
            }
            return Err(error.into());
        }
    };
    let (freshness, after) = {
        let database = state.db.lock().await;
        // The data is already stored. A failure to read freshness only makes the "any new samples" check fall back to the safe side.
        // It must not turn this successful sync into an error.
        let freshness = database.stream_freshness().unwrap_or_else(|error| {
            eprintln!("Could not read freshness after sync: {error}");
            Default::default()
        });
        let after = freshness
            .iter()
            .map(|(stream, value)| (stream.clone(), value.newest_sample_at.clone()))
            .collect::<BTreeMap<_, _>>();
        (freshness, after)
    };
    let outcome = classify_outcome(&report, &before, &after);
    record_cloud_sync_locked(state, &finished_at, outcome, report.records_written).await;

    if manager.is_none() {
        // Official only: the two lines below change the legacy channel auth state, which is not related to this round.
    } else if report.streams.iter().any(|stream| stream.needs_reauth) {
        *state.auth_state.write().await = "needs_reauth".to_string();
    } else if report.core_ok {
        // If the core data streams work, the credential is good. A failure of a side stream (sleep / hrv ...)
        // does not mean the login state is bad, and must not send the user back to "needs reauthentication".
        *state.auth_state.write().await = "verified".to_string();
        // A successful sync proves the credential works: clear the transient
        // verify/auth warning so the UI never shows "Connected" next to a stale
        // red error banner (startup migration notices are intentionally kept).
        *state.auth_warning.write().await = None;
    }

    // The window moves forward one day each day: after sync, also send the training plans that entered the window (nothing is sent without official sign-in,
    // with an empty window, or when the content did not change).
    if official {
        super::training_plan::roll_after_sync(state).await;
    }

    // Push to the user's own server: if the whole sync did not fail, push one round in the background without blocking the sync result.
    // With no enabled target this does nothing (no database open, no connection). Objects with no change are skipped
    // by the hash in the manifest, so a call each round does not resend. Items that failed last round are retried this round.
    if outcome != "failed" {
        zeppbridge_core::push::spawn_after_sync(state.data_dir.clone());
    }

    let mut ui_report = ui_sync_report(
        report,
        started_at,
        finished_at,
        outcome.to_string(),
        &freshness,
    );
    ui_report.content_changed = Some(crate::storage::content_epoch() != epoch_at_start);
    Ok(ui_report)
}

/// Maps one sync to one word in the UI.
///
/// Before, this and `SyncManager::sync_report` each had their own "only the three core streams count" rule,
/// so when sleep / hrv / wellness / workout_detail really failed, both agreed on
/// "Updated". Now there is only one rule: **no real `Failed` can be green.**
///
/// `Unavailable` / `Unverified` stay neutral: the watch does not have that capability, which is not an error.
fn classify_outcome(
    report: &SyncReport,
    before: &BTreeMap<String, Option<String>>,
    after: &BTreeMap<String, Option<String>>,
) -> &'static str {
    let any_failed = report
        .streams
        .iter()
        .any(|stream| stream.status == StreamStatus::Failed);
    let has_success = report
        .streams
        .iter()
        .any(|stream| stream.status == StreamStatus::Success);
    // It is failed only when no stream worked (or there are no streams at all, for example when the whole upstream is down).
    if (any_failed || !report.success) && !has_success {
        return "failed";
    }
    // Some worked and some failed: partial. The user must see that a stream is missing.
    if any_failed {
        return "partial";
    }
    if samples_advanced(before, after) {
        "updated"
    } else {
        "no_new_data"
    }
}

fn samples_advanced(
    before: &BTreeMap<String, Option<String>>,
    after: &BTreeMap<String, Option<String>>,
) -> bool {
    after
        .iter()
        .any(|(stream, newest)| match (before.get(stream), newest) {
            (Some(Some(previous)), Some(current)) => current > previous,
            (_, Some(_)) => true,
            _ => false,
        })
}

/// The content version at the last "stream done" event. The initial value is impossible, so before the first sync it always reports "changed".
static PROGRESS_EPOCH: AtomicU64 = AtomicU64::new(u64::MAX);

/// A sync / backfill round starts: the next done event counts only changes from this round.
fn begin_progress_epoch() {
    PROGRESS_EPOCH.store(crate::storage::content_epoch(), Ordering::SeqCst);
}

/// Progress sent to the UI: a done event also carries `changed`, which tells whether this step changed the database.
/// A stream fetched again with no change still sends a done event, and the UI must not invalidate page caches and query again for it.
#[derive(Clone, serde::Serialize)]
struct UiSyncProgress {
    #[serde(flatten)]
    progress: SyncProgress,
    #[serde(skip_serializing_if = "Option::is_none")]
    changed: Option<bool>,
}

fn emit_sync_progress(app: &AppHandle, progress: SyncProgress) {
    let changed = progress.completed.then(|| {
        let now = crate::storage::content_epoch();
        PROGRESS_EPOCH.swap(now, Ordering::SeqCst) != now
    });
    let _ = app.emit("sync://progress", UiSyncProgress { progress, changed });
}

/// Records this round's sync time and result.
///
/// Two changes. The wait for the write lock runs in the blocking thread pool (before, it polled with `std::thread::sleep` in async,
/// which could freeze a Tokio worker for up to 10 seconds). A failure to record is only logged, not returned as an error: at this point all data
/// is stored, and if backup / compression / a CLI reparse holds the lock, a successful sync must not become
/// an IPC error that also loses the result.
async fn record_cloud_sync_locked(
    state: &AppState,
    finished_at: &str,
    outcome: &str,
    records_written: i64,
) {
    let data_dir = state.data_dir.clone();
    let guard = tokio::task::spawn_blocking(move || {
        write_lock::acquire_with_timeout(&data_dir, WritePurpose::Metadata, Duration::from_secs(10))
    })
    .await;
    let _write_guard = match guard {
        Ok(Ok(guard)) => guard,
        Ok(Err(error)) => {
            eprintln!("Could not record this round's sync time (write lock busy): {error}");
            return;
        }
        Err(error) => {
            eprintln!("Could not record this round's sync time: {error}");
            return;
        }
    };
    let database = state.db.lock().await;
    if let Err(error) = database.record_cloud_sync(finished_at, outcome, records_written) {
        eprintln!("Could not record this round's sync time: {error}");
    }
}

/// Whether this round probes the cloud first, and from which moment. An explicit history backfill (`History`) does not probe.
fn probe_window(
    window: &SyncWindow,
    before: &BTreeMap<String, Option<String>>,
    full_refresh_due: bool,
) -> Option<DateTime<Utc>> {
    if matches!(window, SyncWindow::History(_)) {
        return None;
    }
    let newest = before.get("heart_rate").and_then(|value| value.as_deref());
    probe_since(newest, Utc::now(), full_refresh_due)
}

/// The probe for official sign-in only: it uses the same token store and time zone as the real sync.
async fn official_probe(state: &AppState, since: DateTime<Utc>) -> CloudProbe {
    let Ok(db) = Database::open_without_migration(state.data_dir.join("zepp.db")) else {
        return CloudProbe::Unknown;
    };
    let time_zone = db
        .device_time_zone()
        .ok()
        .flatten()
        .unwrap_or_else(zeppbridge_core::official::fetch::system_time_zone);
    match OfficialSync::new(
        &state.data_dir,
        db,
        Arc::new(AtomicBool::new(false)),
        time_zone,
    ) {
        Ok(sync) => sync.probe_cloud(since).await,
        Err(_) => CloudProbe::Unknown,
    }
}

/// The cloud has no new data yet: nothing was fetched, and the UI says "The watch's new data is not in the cloud yet".
fn cloud_stale_ui_report(cloud_latest_at: DateTime<Utc>) -> UiSyncReport {
    let now = Utc::now().to_rfc3339();
    let mut report = ui_sync_report(
        SyncReport {
            cleanup_failed: false,
            success: true,
            core_ok: true,
            streams: Vec::new(),
            records_written: 0,
            message: Some(
                "The watch's new data is not in the cloud yet. Open Zepp on your phone, pull down to sync, then click sync again".into(),
            ),
        },
        now.clone(),
        now,
        "cloud_stale".to_string(),
        &BTreeMap::new(),
    );
    report.message_code = Some("ui.sync.cloud_stale".to_string());
    // Nothing was fetched this round: "last cloud sync" in the report must not be now (ui_sync_report falls back to the finish time when there are no streams).
    report.last_cloud_sync_at = String::new();
    report.cloud_latest_at = Some(cloud_latest_at.to_rfc3339());
    report
}

fn local_maintenance_deferred() -> Option<(&'static str, &'static str)> {
    if crate::storage::compaction_in_progress() {
        Some((
            "err.sync.deferred_compaction",
            "Compressing old responses to save disk space. This cloud sync retries automatically later",
        ))
    } else if crate::storage::replay_in_progress() {
        Some((
            "err.sync.deferred_replay",
            "Rebuilding derived data from local raw responses. This cloud sync retries automatically later",
        ))
    } else {
        None
    }
}

fn busy_deferred_kind() -> (&'static str, &'static str) {
    local_maintenance_deferred().unwrap_or((
        "err.sync.deferred_busy",
        "Another write is in progress. This cloud sync retries automatically later",
    ))
}

fn deferred_app_error(kind: (&'static str, &'static str)) -> AppError {
    AppError::new(kind.0, kind.1)
}

fn deferred_ui_report(kind: (&'static str, &'static str)) -> UiSyncReport {
    let (code, message) = kind;
    let now = Utc::now().to_rfc3339();
    let mut deferred = ui_sync_report(
        SyncReport {
            cleanup_failed: false,
            success: false,
            core_ok: false,
            streams: Vec::new(),
            records_written: 0,
            message: Some(message.into()),
        },
        now.clone(),
        now,
        "deferred".to_string(),
        &BTreeMap::new(),
    );
    deferred.message_code = Some(code.to_string());
    deferred
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::CapabilityStatus;
    use crate::sync::{is_core_stream, StreamReport};

    fn report(statuses: &[StreamStatus], success: bool) -> SyncReport {
        SyncReport {
            cleanup_failed: false,
            success,
            core_ok: success,
            streams: statuses
                .iter()
                .enumerate()
                .map(|(index, status)| StreamReport {
                    stream: format!("stream-{index}"),
                    status: *status,
                    records_written: 0,
                    raw_records: 0,
                    capability: CapabilityStatus::Verified,
                    needs_reauth: false,
                    message: None,
                })
                .collect(),
            records_written: 0,
            message: None,
        }
    }

    #[test]
    fn classifies_new_samples_and_successful_empty_cloud_response() {
        let before = BTreeMap::from([("heart_rate".into(), Some("2026-08-12T10:00:00Z".into()))]);
        let unchanged = before.clone();
        let advanced = BTreeMap::from([("heart_rate".into(), Some("2026-08-12T10:01:00Z".into()))]);
        let success = report(&[StreamStatus::Success], true);

        assert_eq!(
            classify_outcome(&success, &before, &unchanged),
            "no_new_data"
        );
        assert_eq!(classify_outcome(&success, &before, &advanced), "updated");
    }

    /// A report builder that only names the streams. `report()` makes `stream-0` / `stream-1`,
    /// but this regression test is about exactly "which stream failed".
    fn named_report(streams: &[(&str, StreamStatus)]) -> SyncReport {
        let any_failed = streams
            .iter()
            .any(|(_, status)| *status == StreamStatus::Failed);
        let core_failed = streams
            .iter()
            .any(|(name, status)| is_core_stream(name) && *status == StreamStatus::Failed);
        SyncReport {
            cleanup_failed: false,
            success: !any_failed,
            core_ok: !core_failed,
            streams: streams
                .iter()
                .map(|(name, status)| StreamReport {
                    stream: (*name).to_string(),
                    status: *status,
                    records_written: 0,
                    raw_records: 0,
                    capability: CapabilityStatus::Verified,
                    needs_reauth: false,
                    message: None,
                })
                .collect(),
            records_written: 0,
            message: None,
        }
    }

    /// This test locks in the "false green" case: heart rate worked, sleep **really failed**,
    /// and the UI must not say "Updated".
    #[test]
    fn an_optional_stream_failure_is_partial_not_success() {
        let before = BTreeMap::from([("heart_rate".into(), Some("2026-08-12T10:00:00Z".into()))]);
        let advanced = BTreeMap::from([("heart_rate".into(), Some("2026-08-12T10:01:00Z".into()))]);
        let mixed = named_report(&[
            ("heart_rate", StreamStatus::Success),
            ("sleep", StreamStatus::Failed),
        ]);

        assert!(!mixed.success, "When a side stream fails, the whole sync must not count as success");
        assert!(mixed.core_ok, "The core streams did not fail, so the credential must not be judged bad");
        assert_eq!(classify_outcome(&mixed, &before, &advanced), "partial");
        // The same when samples did not advance: do not fall back to `no_new_data` and hide the failure.
        assert_eq!(classify_outcome(&mixed, &before, &before), "partial");
    }

    /// The reverse: a missing device capability is not a failure and must not make the whole sync partial.
    #[test]
    fn an_unavailable_stream_stays_neutral() {
        let samples = BTreeMap::new();
        let report = named_report(&[
            ("heart_rate", StreamStatus::Success),
            ("hrv", StreamStatus::Unavailable),
            ("wellness", StreamStatus::Unverified),
        ]);
        assert!(report.success);
        assert_eq!(classify_outcome(&report, &samples, &samples), "no_new_data");
    }

    #[test]
    fn classifies_partial_and_failed_reports() {
        let samples = BTreeMap::new();
        assert_eq!(
            classify_outcome(
                &report(&[StreamStatus::Success, StreamStatus::Unavailable], true),
                &samples,
                &samples,
            ),
            "no_new_data"
        );
        assert_eq!(
            classify_outcome(
                &report(&[StreamStatus::Failed, StreamStatus::Unavailable], false),
                &samples,
                &samples,
            ),
            "failed"
        );
    }

    #[test]
    fn a_stale_cloud_reports_nothing_written_and_no_sync_time() {
        let latest = DateTime::parse_from_rfc3339("2026-10-07T08:12:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let report = cloud_stale_ui_report(latest);
        assert_eq!(report.outcome, "cloud_stale");
        assert_eq!(report.total_records, 0);
        assert!(report.streams.is_empty());
        // Nothing was fetched this round: "last cloud sync" must not be now.
        assert!(report.last_cloud_sync_at.is_empty());
        assert_eq!(report.message_code.as_deref(), Some("ui.sync.cloud_stale"));
        assert_eq!(
            report.cloud_latest_at.as_deref(),
            Some("2026-10-07T08:12:00+00:00")
        );
    }

    #[test]
    fn history_backfills_never_probe_the_cloud_first() {
        let recent = (Utc::now() - chrono::Duration::minutes(30)).to_rfc3339();
        let before = BTreeMap::from([("heart_rate".to_string(), Some(recent))]);
        assert!(probe_window(&SyncWindow::History(180), &before, false).is_none());
        assert!(probe_window(&SyncWindow::Incremental, &before, false).is_some());
        assert!(probe_window(&SyncWindow::Quick, &before, false).is_some());
        // A full refresh is due: sync as usual.
        assert!(probe_window(&SyncWindow::Quick, &before, true).is_none());
    }

    #[test]
    fn a_busy_writer_is_deferred_not_a_failed_red_bar() {
        let report = deferred_ui_report((
            "err.sync.deferred_busy",
            "Another write is in progress. This cloud sync retries automatically later",
        ));
        assert_eq!(report.outcome, "deferred");
        assert_eq!(
            report.message_code.as_deref(),
            Some("err.sync.deferred_busy")
        );
        assert!(!report.success);
        assert_eq!(report.total_records, 0);
    }
}
