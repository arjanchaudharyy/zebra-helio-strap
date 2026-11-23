//! Tauri adapter layer for backup and restore.
//!
//! All rules for snapshot creation, validation, manifest and restore are in
//! `zeppbridge_core::storage::backup`. This module only takes the write lock, gives results to the UI,
//! and tells the user the truth: a restore runs at the next start.

use crate::app_state::AppState;
use crate::ipc_error::AppError;
use std::time::Duration;
use zeppbridge_core::storage::backup::{
    self, BackupKind, BackupManifest, BackupVerification, PendingRestore, RestorePreview,
};
use zeppbridge_core::storage::write_lock::{acquire_with_timeout, try_acquire, WritePurpose};

const LOCK_TIMEOUT: Duration = Duration::from_secs(30);

/// All snapshots, newest first.
#[tauri::command]
pub async fn list_backups(
    state: tauri::State<'_, AppState>,
) -> std::result::Result<Vec<BackupManifest>, AppError> {
    backup::list_backups(&state.data_dir).map_err(AppError::from)
}

/// The user creates a snapshot manually.
///
/// Use the SQLite Backup API, then run `integrity_check` and compute SHA-256 at once.
/// If validation fails, report an error and delete the partial file. Do not leave a bad backup that looks good.
///
/// Taking the lock and the copy/hash work are blocking operations. They can wait the full `LOCK_TIMEOUT`
/// if another writer holds the lock. So run them in `spawn_blocking`, or they hold a Tokio worker thread and slow
/// all other IPC commands during that time (the same as `crate::commands::spawn_independent_write`
/// already does for normal writes).
#[tauri::command]
pub async fn create_manual_backup(
    state: tauri::State<'_, AppState>,
) -> std::result::Result<BackupManifest, AppError> {
    let data_dir = state.data_dir.clone();
    crate::commands::join_blocking(
        tokio::task::spawn_blocking(move || -> std::result::Result<BackupManifest, AppError> {
            let _guard = acquire_with_timeout(&data_dir, WritePurpose::Backup, LOCK_TIMEOUT)?;
            backup::create_backup(&data_dir, BackupKind::Manual, env!("CARGO_PKG_VERSION"))
                .map_err(AppError::from)
        })
        .await,
    )?
}

/// Validate an existing snapshot again: file, size, SHA-256 and integrity.
#[tauri::command]
pub async fn verify_backup(
    state: tauri::State<'_, AppState>,
    backup_id: String,
) -> std::result::Result<BackupVerification, AppError> {
    backup::verify_backup(&state.data_dir, &backup_id).map_err(AppError::from)
}

/// Set / clear the "do not clean up this backup automatically" mark.
#[tauri::command]
pub async fn set_backup_pinned(
    state: tauri::State<'_, AppState>,
    backup_id: String,
    pinned: bool,
) -> std::result::Result<BackupManifest, AppError> {
    let _guard = try_acquire(&state.data_dir, WritePurpose::Backup)?;
    backup::set_pinned_unlocked(&state.data_dir, &backup_id, pinned).map_err(AppError::from)
}

/// Preview before restore: manifest, coverage, record count difference from the current database, compatibility.
#[tauri::command]
pub async fn get_restore_preview(
    state: tauri::State<'_, AppState>,
    backup_id: String,
) -> std::result::Result<RestorePreview, AppError> {
    backup::restore_preview(&state.data_dir, &backup_id).map_err(AppError::from)
}

/// Queue a restore.
///
/// Queueing runs all validation and creates a rollback snapshot. The real file replace runs at the next start,
/// before any connection opens. That is the only time when an atomic replace is possible.
///
/// Same as `create_manual_backup`: taking the lock and validating the snapshot are blocking operations, so run
/// them in `spawn_blocking` so that they do not hold a Tokio worker thread.
#[tauri::command]
pub async fn stage_restore(
    state: tauri::State<'_, AppState>,
    backup_id: String,
) -> std::result::Result<PendingRestore, AppError> {
    let data_dir = state.data_dir.clone();
    crate::commands::join_blocking(
        tokio::task::spawn_blocking(move || -> std::result::Result<PendingRestore, AppError> {
            backup::stage_restore(&data_dir, &backup_id, env!("CARGO_PKG_VERSION"))
                .map_err(AppError::from)
        })
        .await,
    )?
}

/// Is a restore queued now?
#[tauri::command]
pub async fn get_pending_restore(
    state: tauri::State<'_, AppState>,
) -> std::result::Result<Option<PendingRestore>, AppError> {
    Ok(backup::pending_restore(&state.data_dir))
}

/// Cancel the queued restore. The rollback snapshot stays. We do not delete the user's backups.
#[tauri::command]
pub async fn cancel_pending_restore(
    state: tauri::State<'_, AppState>,
) -> std::result::Result<(), AppError> {
    let _guard = try_acquire(&state.data_dir, WritePurpose::Restore)?;
    backup::cancel_pending_restore_unlocked(&state.data_dir).map_err(AppError::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use zeppbridge_core::storage::write_lock;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("zeppbridge-cmd-backup-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn pin_and_cancel_refuse_to_run_without_the_write_lock() {
        let dir = temp_dir("lock");
        let held = write_lock::try_acquire(&dir, WritePurpose::Sync).unwrap();
        match try_acquire(&dir, WritePurpose::Backup) {
            Err(write_lock::WriteLockError::Busy { .. }) => {}
            other => panic!("pin must take the write lock: {other:?}"),
        }
        match try_acquire(&dir, WritePurpose::Restore) {
            Err(write_lock::WriteLockError::Busy { .. }) => {}
            other => panic!("cancel must take the write lock: {other:?}"),
        }
        drop(held);
        assert!(try_acquire(&dir, WritePurpose::Backup).is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
