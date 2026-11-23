//! Restore: preflight, queueing, swap-in at next startup, rollback on failure (split out of storage/backup.rs, logic unchanged).

use super::*;

/// Preview before a restore: manifest, verification result, difference from the current database, and whether it can be restored.
pub fn restore_preview(data_dir: &Path, id: &str) -> Result<RestorePreview> {
    validate_backup_id(id)?;
    let manifest = load_manifest(data_dir, id)?;
    let verification = verify_backup(data_dir, id)?;
    let current_schema_version = current_schema_version(data_dir);
    let compatibility = if manifest.schema_version > current_schema_version {
        RestoreCompatibility::FutureSchemaRefused
    } else if manifest.schema_version == current_schema_version {
        RestoreCompatibility::SameSchema
    } else {
        RestoreCompatibility::OlderSchemaWillMigrate
    };
    let mut current_table_counts = BTreeMap::new();
    if let Ok(db) = Database::open_read_only_any_version(database_path(data_dir)) {
        for table in COUNTED_TABLES {
            let count: i64 = db
                .conn
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                    row.get(0)
                })
                .unwrap_or(0);
            current_table_counts.insert(table.to_string(), count);
        }
    }
    let blocker = if !verification.is_usable() {
        verification
            .problem
            .clone()
            .or_else(|| Some("The backup failed verification".into()))
    } else if compatibility == RestoreCompatibility::FutureSchemaRefused {
        Some(format!(
            "This backup comes from a newer ZeppBridge (schema {}, current {}). Opening it with an older version loses fields, so it is not restored and the current database is unchanged. Upgrade ZeppBridge first.",
            manifest.schema_version, current_schema_version
        ))
    } else {
        None
    };
    Ok(RestorePreview {
        manifest,
        verification,
        compatibility,
        current_schema_version,
        current_table_counts,
        can_restore: blocker.is_none(),
        blocker,
    })
}

pub(super) fn current_schema_version(data_dir: &Path) -> i64 {
    Database::open_read_only_any_version(database_path(data_dir))
        .ok()
        .and_then(|db| {
            db.conn
                .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
                .ok()
        })
        .unwrap_or(CURRENT_SCHEMA_VERSION)
}

/// Queues a restore: finish all checks and the rollback snapshot first, then write the pending entry.
///
/// The actual file swap waits until the next startup. At that point the app, the sync thread and the local API have not
/// opened any connection yet, so only then can the swap be truly atomic.
pub fn stage_restore(data_dir: &Path, id: &str, app_version: &str) -> Result<PendingRestore> {
    validate_backup_id(id)?;
    // A sync may be writing: wait for it to release the write lock, instead of hitting "busy" right after the user clicks restore.
    let _guard = write_lock::acquire_with_timeout(
        data_dir,
        WritePurpose::Restore,
        std::time::Duration::from_secs(30),
    )
    .map_err(map_write_lock)?;
    if pending_restore(data_dir).is_none() {
        // No pending entry means the last restore finished (the pending entry is deleted first on completion). What is left is only
        // an old database copy that cleanup did not remove. Keeping it would make a new restore think a swap is half done, and all writes
        // and cancels would be blocked.
        if database_path(data_dir).try_exists()? {
            remove_sqlite_group(&previous_path(data_dir))?;
        }
        remove_if_present(&publication_path(data_dir))?;
    } else if publication_path(data_dir).exists() || previous_path(data_dir).exists() {
        return Err(ZeppBridgeError::Busy(
            "A restore has started. Restart to complete it.".into(),
        ));
    }
    let preview = restore_preview(data_dir, id)?;
    if !preview.can_restore {
        return Err(ZeppBridgeError::DataUnavailable(
            preview
                .blocker
                .unwrap_or_else(|| "This backup cannot be restored right now".into()),
        ));
    }
    // Create the rollback snapshot at queue time. If we waited until startup and the disk was full then, there would be no way back.
    let rollback = create_backup(data_dir, BackupKind::PreRestore, app_version)?;
    let pending = PendingRestore {
        backup_id: id.to_string(),
        staged_at: Utc::now().to_rfc3339(),
        rollback_backup_id: rollback.id,
    };
    let encoded = serde_json::to_vec_pretty(&pending)
        .map_err(|error| ZeppBridgeError::ParseError(format!("Cannot write the restore plan: {error}")))?;
    crate::paths::write_file_atomically(&data_dir.join(PENDING_RESTORE_FILE), &encoded)?;
    Ok(pending)
}

pub fn pending_restore(data_dir: &Path) -> Option<PendingRestore> {
    let text = std::fs::read_to_string(data_dir.join(PENDING_RESTORE_FILE)).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn cancel_pending_restore(data_dir: &Path) -> Result<()> {
    let _guard =
        write_lock::try_acquire(data_dir, WritePurpose::Restore).map_err(map_write_lock)?;
    cancel_pending_restore_unlocked(data_dir)
}

/// Use when the caller holds the write lock.
pub fn cancel_pending_restore_unlocked(data_dir: &Path) -> Result<()> {
    if publication_path(data_dir).exists() || previous_path(data_dir).exists() {
        return Err(ZeppBridgeError::Busy(
            "A restore has started. Restart to complete it.".into(),
        ));
    }
    remove_if_present(&data_dir.join(PENDING_RESTORE_FILE))?;
    Ok(())
}

pub fn apply_pending_restore(data_dir: &Path) -> Option<RestoreOutcome> {
    let pending = pending_restore(data_dir)?;
    let _guard = match write_lock::try_acquire(data_dir, WritePurpose::Restore) {
        Ok(guard) => guard,
        Err(error) => {
            let error = map_write_lock(error);
            let code = if error.is_busy() {
                "err.backup.restore_busy"
            } else {
                "err.storage.write_lock_unavailable"
            };
            return Some(restore_fail(
                &pending,
                format!("The restore has not run yet: {}. It will try again at next startup.", error.user_message()),
                code,
            ));
        }
    };
    // Before the write lock was taken, another process may have completed or cancelled this restore.
    pending_restore(data_dir).map(|pending| run_restore(data_dir, &pending))
}

pub(super) fn restore_fail(
    pending: &PendingRestore,
    message: String,
    code: &str,
) -> RestoreOutcome {
    RestoreOutcome {
        backup_id: pending.backup_id.clone(),
        rollback_backup_id: pending.rollback_backup_id.clone(),
        succeeded: false,
        message,
        message_code: Some(code.to_string()),
    }
}

pub(super) fn restore_ok(pending: &PendingRestore, message: String) -> RestoreOutcome {
    RestoreOutcome {
        backup_id: pending.backup_id.clone(),
        rollback_backup_id: pending.rollback_backup_id.clone(),
        succeeded: true,
        message,
        message_code: None,
    }
}

#[derive(Serialize, Deserialize)]
struct RestorePublication {
    pending: PendingRestore,
    sha256: String,
}

fn publication_path(data_dir: &Path) -> PathBuf {
    data_dir.join("restore-publication.json")
}

fn previous_path(data_dir: &Path) -> PathBuf {
    data_dir.join("zepp.db.restore-previous")
}

/// A writer must not create an empty database while a restore owns the original file.
pub(crate) fn check_incomplete_restore(db_path: &Path) -> Result<()> {
    if let Some(data_dir) = db_path.parent() {
        if db_path == database_path(data_dir)
            && previous_path(data_dir).try_exists()?
            && (!db_path.try_exists()? || data_dir.join(PENDING_RESTORE_FILE).try_exists()?)
        {
            return Err(ZeppBridgeError::DataUnavailable(
                "A database restore is not complete. Restart the desktop app to continue the restore.".into(),
            ));
        }
    }
    Ok(())
}

fn remove_if_present(path: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}

pub(super) fn run_restore(data_dir: &Path, pending: &PendingRestore) -> RestoreOutcome {
    match publish_restore(data_dir, pending) {
        Ok(()) => restore_ok(
            pending,
            "Restored from backup. The database before the restore was saved as a rollback backup, so you can switch back.".into(),
        ),
        Err(error) => {
            let live = database_path(data_dir);
            let displaced = previous_path(data_dir);
            let message = if !live.exists() && displaced.exists() {
                match restore_sqlite_group(&displaced, &live) {
                    Ok(()) => format!("The restore did not complete. The original database was put back: {}", error.user_message()),
                    Err(rollback) => {
                        format!("The restore did not complete and the automatic rollback failed ({rollback}). Keep the restore files.")
                    }
                }
            } else if !publication_path(data_dir).exists() {
                format!("The restore did not run. The current database is unchanged: {}", error.user_message())
            } else {
                format!("The restore did not finish: {}", error.user_message())
            };
            if live.exists()
                && !displaced.exists()
                && data_dir.join("zepp.db.restore-staging").exists()
            {
                let _ = remove_if_present(&publication_path(data_dir));
            }
            let code = if !publication_path(data_dir).exists() && live.exists() {
                "err.backup.restore_failed"
            } else {
                error.code()
            };
            restore_fail(pending, message, code)
        }
    }
}

fn publish_restore(data_dir: &Path, pending: &PendingRestore) -> Result<()> {
    let live = database_path(data_dir);
    let staging = data_dir.join("zepp.db.restore-staging");
    let displaced = previous_path(data_dir);
    let publication = publication_path(data_dir);
    let existing = match std::fs::read(&publication) {
        Ok(bytes) => Some(
            serde_json::from_slice::<RestorePublication>(&bytes)
                .map_err(|error| ZeppBridgeError::ParseError(error.to_string()))?,
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };

    if let Some(existing) = existing {
        if existing.pending != *pending {
            return Err(ZeppBridgeError::DataUnavailable(
                "The restore state does not match the queued backup".into(),
            ));
        }
        if !staging.exists() && live.exists() && file_sha256(&live)? == existing.sha256 {
            if SQLITE_SIDECARS
                .iter()
                .any(|suffix| sidecar(&live, suffix).exists())
            {
                return Err(ZeppBridgeError::DataUnavailable(
                    "The restore target still has SQLite journal files. Keep the current database and the restore copy.".into(),
                ));
            }
            return finish_restore(data_dir, &displaced, &publication);
        }
    }

    let verification = verify_backup(data_dir, &pending.backup_id)?;
    if !verification.is_usable() {
        return Err(ZeppBridgeError::DataUnavailable(
            verification
                .problem
                .unwrap_or_else(|| "The backup failed verification".into()),
        ));
    }
    let manifest = load_manifest(data_dir, &pending.backup_id)?;

    // An older version may have swapped in the snapshot but not cleared the pending entry. With an old WAL present, the move flow below still runs.
    if !publication.exists()
        && displaced.exists()
        && live.exists()
        && SQLITE_SIDECARS
            .iter()
            .all(|suffix| !sidecar(&live, suffix).exists())
        && file_sha256(&live)? == manifest.sha256
        && manifest.schema_version == CURRENT_SCHEMA_VERSION
    {
        return finish_restore(data_dir, &displaced, &publication);
    }

    // Finish the migration in a temporary database. If the migration fails, the current database and WAL stay in place.
    remove_sqlite_group(&staging)?;
    std::fs::copy(snapshot_path(data_dir, &pending.backup_id), &staging)?;
    if file_sha256(&staging)? != manifest.sha256 {
        return Err(ZeppBridgeError::DataUnavailable(
            "The checksum of the restore temporary file does not match".into(),
        ));
    }
    let db = Database::from_connection(rusqlite::Connection::open(&staging)?)?;
    let integrity: String = db
        .conn
        .query_row("PRAGMA integrity_check(1)", [], |row| row.get(0))?;
    if !integrity.eq_ignore_ascii_case("ok") {
        return Err(ZeppBridgeError::DataUnavailable(
            "The restore temporary file failed the integrity check".into(),
        ));
    }
    let journal: String = db
        .conn
        .query_row("PRAGMA journal_mode=DELETE", [], |row| row.get(0))?;
    if !journal.eq_ignore_ascii_case("delete") {
        return Err(ZeppBridgeError::DataUnavailable(
            "Cannot merge the restore temporary database into a single file".into(),
        ));
    }
    drop(db);
    std::fs::OpenOptions::new()
        .write(true)
        .open(&staging)?
        .sync_all()?;
    let state = RestorePublication {
        pending: pending.clone(),
        sha256: file_sha256(&staging)?,
    };
    let bytes = serde_json::to_vec(&state)
        .map_err(|error| ZeppBridgeError::ParseError(error.to_string()))?;
    crate::paths::write_file_atomically(&publication, &bytes)?;

    if live.exists() && displaced.exists() {
        return Err(ZeppBridgeError::DataUnavailable(
            "Cannot confirm the state of the last restore. Keep the current database and the restore copy.".into(),
        ));
    }
    if let Err(error) =
        displace_sqlite_group(&live, &displaced).and_then(|()| std::fs::rename(&staging, &live))
    {
        if let Err(rollback) = restore_sqlite_group(&displaced, &live) {
            return Err(ZeppBridgeError::DataUnavailable(format!(
                "Swapping in the backup failed ({error}), and the automatic rollback also failed ({rollback}). Keep the database files created during the restore."
            )));
        }
        return Err(error.into());
    }
    finish_restore(data_dir, &displaced, &publication)
}

fn finish_restore(data_dir: &Path, displaced: &Path, publication: &Path) -> Result<()> {
    // After the pending entry is consumed, the new database can take writes. A cleanup failure only leaves one extra old database copy.
    remove_if_present(&data_dir.join(PENDING_RESTORE_FILE))?;
    let _ = remove_sqlite_group(displaced);
    let _ = remove_if_present(publication);
    Ok(())
}

pub(super) const SQLITE_SIDECARS: [&str; 2] = ["-wal", "-shm"];

pub(super) fn sidecar(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(suffix);
    PathBuf::from(name)
}

fn move_sidecars(from: &Path, to: &Path) -> std::io::Result<()> {
    for suffix in SQLITE_SIDECARS {
        let from = sidecar(from, suffix);
        let to = sidecar(to, suffix);
        if from.try_exists()? {
            if to.try_exists()? {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    "SQLite sidecar already exists",
                ));
            }
            std::fs::rename(from, to)?;
        }
    }
    Ok(())
}

pub(super) fn displace_sqlite_group(live: &Path, displaced: &Path) -> std::io::Result<()> {
    if live.try_exists()? {
        std::fs::rename(live, displaced)?;
    }
    // If the process stops right after the main database is renamed, restart moves the original journal files from here, then swaps in the new database.
    move_sidecars(live, displaced)
}

pub(super) fn restore_sqlite_group(displaced: &Path, live: &Path) -> std::io::Result<()> {
    if displaced.try_exists()? {
        move_sidecars(displaced, live)?;
        std::fs::rename(displaced, live)?;
    }
    Ok(())
}

pub(super) fn remove_sqlite_group(path: &Path) -> std::io::Result<()> {
    remove_if_present(path)?;
    for suffix in SQLITE_SIDECARS {
        remove_if_present(&sidecar(path, suffix))?;
    }
    Ok(())
}
