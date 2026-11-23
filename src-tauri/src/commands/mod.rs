mod ai_clients;
mod ai_handoff;
mod ai_tasks;
mod auth;
mod backup;
mod cloud_companion;
mod coach_ai;
mod strap_live;
pub(crate) mod credential_recovery;
mod data;
pub use data::{edit_workouts, get_hidden_workout_page, undo_workout_edit};
mod data_letters;
mod imports;
mod login;
mod mcp_writes;
mod my_records;
mod official;
mod push;
mod status;
mod sync;
mod system;
mod training_plan;

use crate::ipc_error::AppError;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::sync::Mutex;
use zeppbridge_core::storage::write_lock::{self, WritePurpose};
use zeppbridge_core::storage::Database;

pub(crate) fn join_blocking<T>(result: Result<T, tokio::task::JoinError>) -> Result<T, AppError> {
    result.map_err(|_| AppError::new("err.storage.worker_failed", "The background database task was interrupted"))
}

/// The longest time a short write waits for another writer.
///
/// Sync and backfill now take the write lock only while they store data (milliseconds to seconds), so a short wait is enough.
/// Before, this failed on the first conflict: auto sync held the write lock for over ten seconds every 15 minutes, and during that time
/// changing a setting or a workout type was refused with "another write is in progress".
#[cfg(not(test))]
const QUICK_WRITE_WAIT: Duration = Duration::from_secs(5);
/// Tests only need to prove "it waits, and reports busy if the wait runs out". They do not need to wait five seconds.
#[cfg(test)]
const QUICK_WRITE_WAIT: Duration = Duration::from_millis(600);

/// Takes the write lock for a short write: if the lock is taken, retry a moment later in async code, without blocking a runtime worker thread.
/// If the lock is still taken after [`QUICK_WRITE_WAIT`] (long maintenance such as backup or replay), report the retryable busy error as before.
pub(crate) async fn acquire_quick_write(
    data_dir: &Path,
    purpose: WritePurpose,
) -> Result<write_lock::ExclusiveWriteGuard, write_lock::WriteLockError> {
    let deadline = std::time::Instant::now() + QUICK_WRITE_WAIT;
    loop {
        match write_lock::try_acquire(data_dir, purpose) {
            Err(write_lock::WriteLockError::Busy { .. })
                if std::time::Instant::now() < deadline =>
            {
                tokio::time::sleep(Duration::from_millis(120)).await;
            }
            other => return other,
        }
    }
}

pub(crate) async fn with_write<T>(
    data_dir: &Path,
    database: &Mutex<Database>,
    purpose: WritePurpose,
    mutation: impl FnOnce(&Database) -> zeppbridge_core::models::error::Result<T>,
) -> Result<T, AppError> {
    let _write_guard = acquire_quick_write(data_dir, purpose).await?;
    let mut db = database.lock().await;
    // If startup hit another writer's lock and got only a read-only connection, switch to a writable one now that the lock is free.
    db.reopen_writable_if_query_only(data_dir.join("zepp.db"))?;
    Ok(mutation(&db)?)
}

/// Before saving new credentials, check who owns the library: if it belongs to another account, refuse
/// and write nothing. An empty library or the same account is accepted. The legacy channel and official authorization share this entry point.
/// Whether two Zepp account IDs belong to different people. An empty value on either side is not a mismatch: it means not configured yet.
///
/// Official authorization and advanced data use the same user_id numbering (checked against a real library:
/// the same account has equal values on both sides), so they can be compared directly.
pub(crate) fn accounts_differ(a: &str, b: &str) -> bool {
    let (a, b) = (a.trim(), b.trim());
    !a.is_empty() && !b.is_empty() && a != b
}

pub(crate) async fn claim_library_for_login(
    data_dir: &Path,
    user_id: &str,
) -> Result<(), AppError> {
    let known = zeppbridge_core::storage::configured_accounts(data_dir);
    let user_id = user_id.to_string();
    spawn_independent_write(data_dir.to_path_buf(), WritePurpose::Metadata, move |db| {
        db.claim_library_for_login(&user_id, &known)
    })
    .await
}

pub(crate) async fn spawn_independent_write<T, F>(
    data_dir: PathBuf,
    purpose: WritePurpose,
    work: F,
) -> Result<T, AppError>
where
    T: Send + 'static,
    F: FnOnce(&Database) -> zeppbridge_core::models::error::Result<T> + Send + 'static,
{
    join_blocking(
        tokio::task::spawn_blocking(move || -> Result<T, AppError> {
            let _replay = matches!(purpose, WritePurpose::Reprocess)
                .then(zeppbridge_core::storage::ReplayGuard::enter);
            let _compaction = matches!(purpose, WritePurpose::Compaction)
                .then(zeppbridge_core::storage::CompactionGuard::enter);
            let _write_guard =
                write_lock::acquire_with_timeout(&data_dir, purpose, Duration::from_secs(20))?;
            let db = Database::open_without_migration(data_dir.join("zepp.db"))?;
            work(&db).map_err(AppError::from)
        })
        .await,
    )?
}

/// Idle read-only connections, returned and reused by library path.
///
/// Read-only commands used to queue behind the single `AppState::db` lock. The dozen or so overview queries at startup
/// ran one by one, and ran blocking SQLite on async runtime threads. In WAL mode reads
/// can run in parallel: now each read-only command takes its own read-only connection in the blocking thread pool. A few connections
/// are kept for reuse. This saves about 2.5 ms of open time and a cold page cache each time. Idle connections hold no read
/// snapshot, so they do not block checkpoints or writes.
static READ_POOL: std::sync::Mutex<Vec<(PathBuf, Database)>> = std::sync::Mutex::new(Vec::new());
const READ_POOL_IDLE_MAX: usize = 4;

fn take_read_connection(path: &Path) -> zeppbridge_core::models::error::Result<Database> {
    let pooled = READ_POOL.lock().ok().and_then(|mut pool| {
        let index = pool.iter().position(|(p, _)| p == path)?;
        Some(pool.swap_remove(index).1)
    });
    match pooled {
        Some(db) => Ok(db),
        None => Database::open_read_only(path.to_path_buf()),
    }
}

fn return_read_connection(path: PathBuf, db: Database) {
    if let Ok(mut pool) = READ_POOL.lock() {
        if pool.len() < READ_POOL_IDLE_MAX {
            pool.push((path, db));
        }
    }
}

pub(crate) async fn spawn_independent_read<T, F>(data_dir: PathBuf, work: F) -> Result<T, AppError>
where
    T: Send + 'static,
    F: FnOnce(&Database) -> zeppbridge_core::models::error::Result<T> + Send + 'static,
{
    join_blocking(
        tokio::task::spawn_blocking(move || {
            let path = data_dir.join("zepp.db");
            let db = take_read_connection(&path)?;
            let result = work(&db);
            // A connection that failed does not go back to the pool: after the library is restored or replaced, an old connection keeps reporting the same error.
            if result.is_ok() {
                return_read_connection(path, db);
            }
            result
        })
        .await,
    )?
    .map_err(read_failure)
}

/// An independent read failed: during replay or compaction, report the "local maintenance, retrying automatically" code,
/// so the UI does not say "the local database is temporarily unavailable" (users saw this when handing data to AI).
/// Read connections already have a 30-second busy_timeout, so there is no extra retry here.
fn read_failure(error: zeppbridge_core::models::error::ZeppBridgeError) -> AppError {
    use zeppbridge_core::models::error::ZeppBridgeError;
    if let ZeppBridgeError::DatabaseError(inner) = &error {
        // Log only the SQLite error code. No path, no data.
        eprintln!("Read-only query failed: {:?}", inner.sqlite_error_code());
        if let Some(deferred) = local_maintenance_error() {
            return deferred;
        }
    }
    AppError::from(error)
}

/// The code a read command reports while local maintenance (replay or compaction) runs.
pub(crate) fn local_maintenance_error() -> Option<AppError> {
    if zeppbridge_core::storage::compaction_in_progress() {
        Some(AppError::new(
            "err.local.maintenance_compaction",
            "Compacting stored raw payloads. Retrying automatically when done",
        ))
    } else if zeppbridge_core::storage::replay_in_progress() {
        Some(AppError::new(
            "err.local.maintenance_replay",
            "Rebuilding data from local raw payloads. Retrying automatically when done",
        ))
    } else {
        None
    }
}

pub(crate) use ai_clients::{get_mcp_sidecar, save_mcp_bundle};
pub(crate) use coach_ai::{ai_coach_ask, ai_coach_configure, ai_coach_key_status, ai_coach_set_key, ai_coach_status};
pub(crate) use strap_live::{strap_alerts, strap_daily, strap_key_fetch, daily_goals, strap_detail, strap_live_recent, strap_live_start, strap_set, strap_buzz, wake_alarm_get, wake_alarm_set, focus_timer_start, focus_timer_status, strap_settings, strap_live_stop, strap_sync_now, zebra_start_route};
pub(crate) use ai_handoff::{ai_handoff_autopaste, ai_handoff_cancel, ai_handoff_copy_file};
pub(crate) use ai_tasks::{
    ai_exchange_delete, ai_exchange_list, ai_exchange_rename, ai_profile_save, ai_prompt_export,
    ai_prompt_import, ai_prompt_versions, ai_task_attachment_stat, ai_task_day_strip,
    ai_task_delete, ai_task_delete_many, ai_task_get, ai_task_list, ai_task_prepare,
    ai_task_preview, ai_task_save, ai_task_set_pinned, ai_template_delete, ai_template_list,
    ai_template_save, card_collection_get, card_collection_set,
};
pub(crate) use auth::{clear_auth, manual_auth, start_fresh_library, verify_auth};
pub(crate) use backup::{
    cancel_pending_restore, create_manual_backup, get_pending_restore, get_restore_preview,
    list_backups, set_backup_pinned, stage_restore, verify_backup,
};
pub(crate) use cloud_companion::{
    cloud_companion_open, cloud_companion_save_config, cloud_companion_setup,
    cloud_companion_state, cloud_companion_upload,
};
pub(crate) use data::{
    cleanup_old_data, compact_raw_payloads, copy_workout_gpx, get_capability_overview,
    get_daily_heart_rate_extremes, get_data_health, get_device_profile, get_device_profiles,
    get_health_overview, get_heart_rate_series, get_heart_rate_zones, get_hourly_steps,
    get_metric_baselines, get_metric_series, get_recent_sleep, get_recent_workouts,
    get_sleep_detail, get_sleep_nap_stages, get_sleep_page, get_storage_estimate,
    get_strength_catalog, get_stress_series, get_training_balance, get_unknown_workout_codes,
    get_user_prefs, get_weekly_report, get_workout_detail, get_workout_fit_override,
    get_workout_insight, get_workout_page, get_workout_series, get_workout_type_options,
    import_workout_fit, metric_exclusions, open_data_folder, open_external_url, prepare_ai_handoff,
    remove_workout_fit, reprocess_local_data, run_database_integrity_check, save_fit_export,
    set_device_hidden, set_device_model_override, set_heart_rate_zone_preference,
    set_metric_excluded, set_user_prefs, set_workout_code_label, set_workout_type_override,
};
pub(crate) use data_letters::{
    data_letters_mail_status, data_letters_open_capsule, data_letters_seal_capsule,
    data_letters_state,
};
pub(crate) use imports::{
    export_for_unstrap, import_batches, import_workouts, undo_import_batch, workout_import_sources,
};
pub(crate) use login::{
    cancel_web_login, get_login_status, recover_advanced_credentials, start_web_login,
};
pub(crate) use mcp_writes::{mcp_write_undo, mcp_writes_list, set_mcp_task_only};
pub(crate) use my_records::export_my_records;
pub(crate) use official::{
    cancel_official_login, check_official_login_now, disconnect_official, get_official_status,
    recover_official_credentials, start_official_login,
};
pub(crate) use push::{
    push_import_companion, push_remove, push_run, push_save, push_set_enabled, push_state,
};
pub(crate) use status::get_app_status;
pub(crate) use sync::{
    background_quick_sync, cancel_sync, get_coverage_ledger, get_workout_detail_coverage,
    probe_data_capabilities, reset_coverage_ledger, retry_failed_backfill_chunks,
    start_history_backfill, start_history_sync, start_incremental_sync,
    start_workout_detail_backfill,
};
pub(crate) use system::system_region_locale;

pub(crate) use training_plan::{
    training_plan_adherence, training_plan_discard, training_plan_history, training_plan_preview,
    training_plan_publish, training_plan_save_draft, training_plan_set_ai_publish,
    training_plan_state, training_plan_update_draft,
};

mod coach_notes;
pub(crate) use coach_notes::{
    coach_note_add, coach_note_decide, coach_note_delete, coach_note_edit, coach_notes_export,
    coach_notes_list,
};

mod feedback;
pub(crate) use feedback::{
    workout_feedback_adjusted, workout_feedback_dismiss, workout_feedback_get,
    workout_feedback_prompt, workout_feedback_save, workout_feedback_snooze,
    workout_feedback_toast_due, workout_feedback_toast_show,
};

mod logbook;
pub(crate) use logbook::{
    injuries_list, injury_delete, injury_save, personal_records, race_delete, race_save,
    races_list, shoe_delete, shoe_save, shoes_overview, workout_feel_get, workout_feel_save,
};

mod rings;
pub(crate) use rings::{annual_rings, rings_save_image};

mod life_events;
pub(crate) use life_events::{delete_life_event, list_life_events, save_life_event};
