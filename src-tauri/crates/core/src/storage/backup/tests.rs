use super::*;
use crate::models::{MetricSample, SourceScope};
use crate::storage::write_lock::{self, WritePurpose};
use chrono::TimeZone;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("zeppbridge-backup-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn seed(data_dir: &Path, samples: i64) -> Database {
    let db = Database::new(database_path(data_dir)).unwrap();
    for index in 0..samples {
        db.insert_metric_sample(&MetricSample {
            metric: "heart_rate".into(),
            timestamp: Utc.with_ymd_and_hms(2026, 8, 1, 0, 0, 0).unwrap()
                + chrono::Duration::minutes(index),
            value: 60.0 + index as f64,
            unit: "bpm".into(),
            source_scope: SourceScope::Device,
            device_id: Some("device-a".into()),
        })
        .unwrap();
    }
    db
}

#[test]
fn backup_restore_keeps_hidden_state_type_corrections_and_undo_history() {
    let dir = temp_dir("local-workout-edits");
    let db = seed(&dir, 1);
    db.insert_workout(&crate::models::Workout {
        workout_id: "hidden-run".into(),
        workout_type: "running".into(),
        normalized_type: "running".into(),
        type_source: "numeric_mapped".into(),
        start_time: Utc.with_ymd_and_hms(2026, 8, 1, 0, 0, 0).unwrap(),
        end_time: Utc.with_ymd_and_hms(2026, 8, 1, 1, 0, 0).unwrap(),
        source_scope: SourceScope::Device,
        ..Default::default()
    })
    .unwrap();
    let edit = db
        .edit_workouts(&["hidden-run".into()], Some(Some("hiking")), Some(true))
        .unwrap();
    drop(db);
    let backup = create_backup(&dir, BackupKind::Manual, "1.0.0").unwrap();
    let db = Database::new(database_path(&dir)).unwrap();
    db.undo_workout_edit(&edit.id).unwrap();
    drop(db);
    stage_restore(&dir, &backup.id, "1.0.0").unwrap();
    let outcome = apply_pending_restore(&dir).unwrap();
    assert!(outcome.succeeded, "{outcome:?}");
    let db = Database::new(database_path(&dir)).unwrap();
    assert_eq!(db.count_workouts().unwrap(), 0);
    assert_eq!(
        db.hidden_workouts_page(10, 0).unwrap()[0]
            .user_override
            .as_deref(),
        Some("hiking")
    );
    db.undo_workout_edit(&edit.id).unwrap();
    assert_eq!(db.count_workouts().unwrap(), 1);
    assert!(db
        .get_workout_detail("hidden-run")
        .unwrap()
        .unwrap()
        .user_override
        .is_none());
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn regression_markdown_manifest_ids_cannot_redirect_pruning_or_pinning() {
    let dir = temp_dir("manifest-id-boundary");
    drop(seed(&dir, 1));
    let mut manifest = create_backup(&dir, BackupKind::PreMigration, "1.0.0").unwrap();
    let original_id = manifest.id.clone();
    let original_path = manifest_path(&dir, &original_id);
    std::fs::write(dir.join("outside.db"), b"keep database").unwrap();
    std::fs::write(dir.join("outside.json"), b"keep manifest").unwrap();
    for index in 0..MIGRATION_BACKUP_KEEP {
        manifest.id = format!("valid-{index}");
        manifest.created_at = format!("9999-{index}");
        write_manifest(&dir, &manifest).unwrap();
    }
    for invalid in [
        "../outside",
        "..\\outside",
        "/outside",
        "C:\\outside",
        "",
        "valid-0",
    ] {
        manifest.id = invalid.into();
        manifest.created_at = "0000".into();
        std::fs::write(&original_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        assert_eq!(list_backups(&dir).unwrap().len(), MIGRATION_BACKUP_KEEP);
        assert!(load_manifest(&dir, &original_id).is_err());
        assert!(set_pinned(&dir, &original_id, true).is_err());
        assert!(prune_migration_backups(&dir).unwrap().is_empty());
        assert_eq!(
            std::fs::read(dir.join("outside.db")).unwrap(),
            b"keep database"
        );
        assert_eq!(
            std::fs::read(dir.join("outside.json")).unwrap(),
            b"keep manifest"
        );
        assert!(!load_manifest(&dir, "valid-0").unwrap().pinned);
    }
    manifest.id = original_id;
    write_manifest(&dir, &manifest).unwrap();
    assert_eq!(prune_migration_backups(&dir).unwrap(), vec![manifest.id]);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_snapshot_is_verified_before_it_is_ever_called_a_backup() {
    let dir = temp_dir("verified");
    let db = seed(&dir, 5);
    drop(db);

    let manifest = create_backup(&dir, BackupKind::Manual, "1.0.0").unwrap();
    assert!(manifest.integrity_ok);
    assert_eq!(manifest.sha256.len(), 64);
    assert!(manifest.bytes > 0);
    assert_eq!(manifest.schema_version, CURRENT_SCHEMA_VERSION);
    assert_eq!(manifest.table_counts.get("metric_samples"), Some(&5));

    let verification = verify_backup(&dir, &manifest.id).unwrap();
    assert!(verification.is_usable(), "{verification:?}");
    assert!(verification.problem.is_none());
}

#[test]
fn the_snapshot_captures_writes_that_are_still_only_in_the_wal() {
    // A plain copy of zepp.db misses these rows. The Backup API does not.
    let dir = temp_dir("wal");
    let db = seed(&dir, 40);
    // Do not close the connection on purpose, and do not checkpoint.
    let manifest = create_backup(&dir, BackupKind::Manual, "1.0.0").unwrap();
    assert_eq!(manifest.table_counts.get("metric_samples"), Some(&40));
    drop(db);
}

/// The reason a verification failed must also be translatable.
///
/// `problem` is the Chinese source text, and the UI uses `problem_code` to get the text in its own language. Without the code,
/// an English user sees a line of Chinese under "Snapshots", the same bug the backfill ledger once had.
#[test]
fn a_failed_verification_carries_a_code_for_the_interface() {
    let dir = temp_dir("problem-code");
    drop(seed(&dir, 3));
    let manifest = create_backup(&dir, BackupKind::Manual, "1.0.0").unwrap();

    // Delete the snapshot file: the easiest failure to build, and the most common one.
    std::fs::remove_file(snapshot_path(&dir, &manifest.id)).unwrap();
    let verification = verify_backup(&dir, &manifest.id).unwrap();

    assert!(!verification.is_usable());
    let problem = verification.problem.as_deref().unwrap_or_default();
    assert!(
        !problem.is_empty(),
        "A failed verification must carry a problem text"
    );
    assert_eq!(
        verification.problem_code.as_deref(),
        Some("ui.backup.file_missing"),
        "The source text must come with a code, so the UI can show localized text"
    );
}

#[test]
fn a_corrupted_snapshot_never_looks_restorable() {
    let dir = temp_dir("corrupt");
    drop(seed(&dir, 3));
    let manifest = create_backup(&dir, BackupKind::Manual, "1.0.0").unwrap();

    // Change the file contents but keep the length, so only the hash can detect it.
    let path = snapshot_path(&dir, &manifest.id);
    let mut bytes = std::fs::read(&path).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 0xFF;
    std::fs::write(&path, &bytes).unwrap();

    let verification = verify_backup(&dir, &manifest.id).unwrap();
    assert!(!verification.sha256_match);
    assert!(!verification.is_usable());
    let preview = restore_preview(&dir, &manifest.id).unwrap();
    assert!(!preview.can_restore);
    assert!(preview.blocker.unwrap().contains("SHA-256"));
}

#[test]
fn a_backup_from_a_newer_schema_is_refused_without_touching_the_current_library() {
    let dir = temp_dir("future");
    drop(seed(&dir, 2));
    let manifest = create_backup(&dir, BackupKind::Manual, "9.9.9").unwrap();

    // Make the manifest claim a newer schema, and change the snapshot itself to match.
    let snapshot = rusqlite::Connection::open(snapshot_path(&dir, &manifest.id)).unwrap();
    snapshot
        .execute_batch(&format!(
            "PRAGMA user_version = {};",
            CURRENT_SCHEMA_VERSION + 1
        ))
        .unwrap();
    drop(snapshot);
    let mut updated = manifest.clone();
    updated.schema_version = CURRENT_SCHEMA_VERSION + 1;
    updated.bytes = std::fs::metadata(snapshot_path(&dir, &manifest.id))
        .unwrap()
        .len();
    updated.sha256 = file_sha256(&snapshot_path(&dir, &manifest.id)).unwrap();
    write_manifest(&dir, &updated).unwrap();

    let preview = restore_preview(&dir, &manifest.id).unwrap();
    assert_eq!(
        preview.compatibility,
        RestoreCompatibility::FutureSchemaRefused
    );
    assert!(!preview.can_restore);
    assert!(stage_restore(&dir, &manifest.id, "1.0.0").is_err());
    assert!(pending_restore(&dir).is_none(), "A refused restore must not leave a pending item");
}

#[test]
fn restore_rechecks_actual_schema_even_when_manifest_claims_compatibility() {
    let dir = temp_dir("future-pending");
    drop(seed(&dir, 2));
    let mut manifest = create_backup(&dir, BackupKind::Manual, "1.0.0").unwrap();
    let pending = stage_restore(&dir, &manifest.id, "1.0.0").unwrap();
    let snapshot = snapshot_path(&dir, &manifest.id);
    let conn = rusqlite::Connection::open(&snapshot).unwrap();
    conn.execute_batch(&format!(
        "PRAGMA user_version = {};",
        CURRENT_SCHEMA_VERSION + 1
    ))
    .unwrap();
    drop(conn);
    manifest.bytes = std::fs::metadata(&snapshot).unwrap().len();
    manifest.sha256 = file_sha256(&snapshot).unwrap();
    write_manifest(&dir, &manifest).unwrap();
    let before = std::fs::read(database_path(&dir)).unwrap();
    let outcome = run_restore(&dir, &pending);
    assert!(!outcome.succeeded);
    assert!(outcome.message.contains("The restore did not run"));
    assert_eq!(std::fs::read(database_path(&dir)).unwrap(), before);
}

#[test]
fn restore_swaps_the_library_and_keeps_a_rollback_snapshot() {
    let dir = temp_dir("restore");
    drop(seed(&dir, 3));
    let backup = create_backup(&dir, BackupKind::Manual, "1.0.0").unwrap();

    // Then more data is written.
    {
        let db = Database::new(database_path(&dir)).unwrap();
        for index in 100..110 {
            db.insert_metric_sample(&MetricSample {
                metric: "heart_rate".into(),
                timestamp: Utc.with_ymd_and_hms(2026, 9, 1, 0, 0, 0).unwrap()
                    + chrono::Duration::minutes(index),
                value: 70.0,
                unit: "bpm".into(),
                source_scope: SourceScope::Device,
                device_id: Some("device-a".into()),
            })
            .unwrap();
        }
    }

    stage_restore(&dir, &backup.id, "1.0.0").unwrap();
    assert!(pending_restore(&dir).is_some());

    let outcome = apply_pending_restore(&dir).expect("a restore is queued");
    assert!(outcome.succeeded, "{outcome:?}");
    assert!(pending_restore(&dir).is_none(), "The pending item must be cleared after the restore runs");

    let db = Database::open_read_only_any_version(database_path(&dir)).unwrap();
    let count: i64 = db
        .conn
        .query_row("SELECT COUNT(*) FROM metric_samples", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 3, "The library must return to its state at backup time");

    // The rollback snapshot exists and holds the 13 rows from before the restore.
    let rollback = load_manifest(&dir, &outcome.rollback_backup_id).unwrap();
    assert_eq!(rollback.kind, BackupKind::PreRestore);
    assert_eq!(rollback.table_counts.get("metric_samples"), Some(&13));
}

/// A restore must move the old library's `-wal` / `-shm` away too, and before the new library goes in.
///
/// A WAL left in place belongs to **the library that was replaced**. When the new library opens, SQLite
/// replays it as its own log, the old library's dirty pages overwrite the new library, and the B-tree is corrupted.
/// The old code put the new library in first, then deleted the WAL with `let _ =`. If the delete failed (a Windows handle not released)
/// or power failed between the two steps, it fell into exactly this case.
#[test]
fn restore_moves_the_wal_away_before_the_new_library_takes_its_place() {
    let dir = temp_dir("restore-wal");
    drop(seed(&dir, 3));
    let backup = create_backup(&dir, BackupKind::Manual, "1.0.0").unwrap();

    // Make a WAL / SHM that "belongs to the old library", with a recognizable marker. A real WAL
    // comes from SQLite. Here we only need to prove that these two files do not stay next to the new library.
    let stale = b"stale-wal-belonging-to-the-replaced-database";
    std::fs::write(dir.join("zepp.db-wal"), stale).unwrap();
    std::fs::write(dir.join("zepp.db-shm"), stale).unwrap();

    stage_restore(&dir, &backup.id, "1.0.0").unwrap();
    let outcome = apply_pending_restore(&dir).expect("a restore is queued");
    assert!(outcome.succeeded, "{outcome:?}");

    for sidecar in ["zepp.db-wal", "zepp.db-shm"] {
        let path = dir.join(sidecar);
        if let Ok(bytes) = std::fs::read(&path) {
            assert_ne!(
                bytes.as_slice(),
                stale,
                "{sidecar} is still the copy from before the restore, and it would be replayed as the new library's log"
            );
        }
    }

    // The restored library still reads, and it really holds the three rows from backup time.
    let db = Database::open_read_only_any_version(database_path(&dir)).unwrap();
    let count: i64 = db
        .conn
        .query_row("SELECT COUNT(*) FROM metric_samples", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 3);
}

/// The three files move away as a group, or come back as a group.
#[test]
fn the_sqlite_group_moves_and_comes_back_together() {
    let dir = temp_dir("sqlite-group");
    let live = dir.join("zepp.db");
    let displaced = dir.join("zepp.db.restore-previous");
    std::fs::write(&live, b"main").unwrap();
    std::fs::write(dir.join("zepp.db-wal"), b"wal").unwrap();
    std::fs::write(dir.join("zepp.db-shm"), b"shm").unwrap();

    displace_sqlite_group(&live, &displaced).unwrap();
    assert!(!live.exists(), "The main library must already be moved away");
    assert!(!dir.join("zepp.db-wal").exists(), "The WAL must move with the main library");
    assert!(!dir.join("zepp.db-shm").exists());
    assert_eq!(
        std::fs::read(dir.join("zepp.db.restore-previous-wal")).unwrap(),
        b"wal"
    );

    restore_sqlite_group(&displaced, &live).unwrap();
    assert_eq!(std::fs::read(&live).unwrap(), b"main");
    assert_eq!(std::fs::read(dir.join("zepp.db-wal")).unwrap(), b"wal");
    assert_eq!(std::fs::read(dir.join("zepp.db-shm")).unwrap(), b"shm");

    remove_sqlite_group(&live).unwrap();
    assert!(!live.exists());
    assert!(!dir.join("zepp.db-wal").exists());
    assert!(!dir.join("zepp.db-shm").exists());
}

#[test]
fn a_busy_write_lock_leaves_pending_restore_in_place() {
    let dir = temp_dir("restore-busy");
    drop(seed(&dir, 3));
    let backup = create_backup(&dir, BackupKind::Manual, "1.0.0").unwrap();
    stage_restore(&dir, &backup.id, "1.0.0").unwrap();
    let _held = write_lock::try_acquire(&dir, WritePurpose::Sync).unwrap();

    let outcome = apply_pending_restore(&dir).expect("a restore is queued");
    assert!(!outcome.succeeded, "{outcome:?}");
    assert_eq!(
        outcome.message_code.as_deref(),
        Some("err.backup.restore_busy")
    );
    assert!(
        pending_restore(&dir).is_some(),
        "The queue file must stay when the write lock is not available"
    );
    assert!(outcome.message.contains("try again at next startup"), "{outcome:?}");
}

#[test]
fn a_missing_snapshot_leaves_the_current_library_untouched() {
    let dir = temp_dir("missing");
    drop(seed(&dir, 6));
    let backup = create_backup(&dir, BackupKind::Manual, "1.0.0").unwrap();
    let pending = stage_restore(&dir, &backup.id, "1.0.0").unwrap();

    // The snapshot is deleted after it is queued and before it runs.
    std::fs::remove_file(snapshot_path(&dir, &pending.backup_id)).unwrap();

    let outcome = apply_pending_restore(&dir).expect("a restore is queued");
    assert!(!outcome.succeeded);
    assert!(outcome.message.contains("The current database is unchanged"), "{outcome:?}");
    assert!(
        pending_restore(&dir).is_some(),
        "A failed restore must leave the queue file, to try again at the next start"
    );

    let db = Database::open_read_only_any_version(database_path(&dir)).unwrap();
    let count: i64 = db
        .conn
        .query_row("SELECT COUNT(*) FROM metric_samples", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 6, "A failed restore must not touch the original library");
}

#[test]
fn pinning_is_refused_while_another_writer_holds_the_lock() {
    let dir = temp_dir("pin-busy");
    drop(seed(&dir, 1));
    let backup = create_backup(&dir, BackupKind::Manual, "1.0.0").unwrap();
    let _held = write_lock::try_acquire(&dir, WritePurpose::Sync).unwrap();
    let error = set_pinned(&dir, &backup.id, true).unwrap_err();
    assert!(error.is_busy(), "{error:?}");
    assert!(!load_manifest(&dir, &backup.id).unwrap().pinned);
    drop(_held);
    assert!(set_pinned(&dir, &backup.id, true).unwrap().pinned);
}

#[test]
fn cancelling_a_restore_is_refused_while_another_writer_holds_the_lock() {
    let dir = temp_dir("cancel-busy");
    drop(seed(&dir, 1));
    let backup = create_backup(&dir, BackupKind::Manual, "1.0.0").unwrap();
    stage_restore(&dir, &backup.id, "1.0.0").unwrap();
    let _held = write_lock::try_acquire(&dir, WritePurpose::Sync).unwrap();
    let error = cancel_pending_restore(&dir).unwrap_err();
    assert!(error.is_busy(), "{error:?}");
    assert!(
        pending_restore(&dir).is_some(),
        "The queue file must stay when the write lock is not available"
    );
    drop(_held);
}

#[test]
fn a_failed_staging_copy_does_not_delete_the_displaced_library() {
    let dir = temp_dir("displaced-kept");
    drop(seed(&dir, 6));
    let backup = create_backup(&dir, BackupKind::Manual, "1.0.0").unwrap();
    {
        let db = Database::new(database_path(&dir)).unwrap();
        db.insert_metric_sample(&MetricSample {
            metric: "heart_rate".into(),
            timestamp: Utc.with_ymd_and_hms(2026, 8, 2, 0, 0, 0).unwrap(),
            value: 70.0,
            unit: "bpm".into(),
            source_scope: SourceScope::Device,
            device_id: Some("device-a".into()),
        })
        .unwrap();
    }
    stage_restore(&dir, &backup.id, "1.0.0").unwrap();

    let displaced = dir.join("zepp.db.restore-previous");
    std::fs::write(&displaced, b"ORIGINAL-LIBRARY-BYTES").unwrap();
    // Make the copy to staging fail: the target is already a directory. The old code deleted
    // displaced before the copy, which left only this half-done copy of the original library.
    let staging = dir.join("zepp.db.restore-staging");
    std::fs::create_dir_all(&staging).unwrap();
    std::fs::write(staging.join("blocker"), b"x").unwrap();

    let outcome = apply_pending_restore(&dir).expect("a restore is queued");
    assert!(!outcome.succeeded, "{outcome:?}");
    assert_eq!(
        std::fs::read(&displaced).unwrap(),
        b"ORIGINAL-LIBRARY-BYTES",
        "A verify / copy failure must not delete the original library in displaced"
    );
    assert!(pending_restore(&dir).is_some());
    let db = Database::open_read_only_any_version(database_path(&dir)).unwrap();
    let count: i64 = db
        .conn
        .query_row("SELECT COUNT(*) FROM metric_samples", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 7, "A failed restore must not touch the original library");
    drop(db);
}

#[test]
fn a_completed_swap_is_finished_without_recopying() {
    let dir = temp_dir("already-swapped");
    drop(seed(&dir, 6));
    let backup = create_backup(&dir, BackupKind::Manual, "1.0.0").unwrap();
    {
        let db = Database::new(database_path(&dir)).unwrap();
        db.insert_metric_sample(&MetricSample {
            metric: "heart_rate".into(),
            timestamp: Utc.with_ymd_and_hms(2026, 8, 2, 0, 0, 0).unwrap(),
            value: 70.0,
            unit: "bpm".into(),
            source_scope: SourceScope::Device,
            device_id: Some("device-a".into()),
        })
        .unwrap();
    }
    stage_restore(&dir, &backup.id, "1.0.0").unwrap();

    let live = database_path(&dir);
    let displaced = dir.join("zepp.db.restore-previous");
    std::fs::copy(&live, &displaced).unwrap();
    std::fs::copy(snapshot_path(&dir, &backup.id), &live).unwrap();

    let outcome = apply_pending_restore(&dir).expect("a restore is queued");
    assert!(outcome.succeeded, "{outcome:?}");
    let db = Database::open_read_only_any_version(database_path(&dir)).unwrap();
    let count: i64 = db
        .conn
        .query_row("SELECT COUNT(*) FROM metric_samples", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 6, "It must finish the backup that is already in place, not copy it again");
    drop(db);
}

#[test]
fn pruning_keeps_five_migration_backups_and_never_deletes_pinned_or_manual_ones() {
    let dir = temp_dir("prune");
    drop(seed(&dir, 1));
    let manual = create_backup(&dir, BackupKind::Manual, "1.0.0").unwrap();

    let mut migration_ids = Vec::new();
    for _ in 0..(MIGRATION_BACKUP_KEEP + 3) {
        let manifest = create_backup(&dir, BackupKind::PreMigration, "1.0.0").unwrap();
        migration_ids.push(manifest.id);
        // The manifest id has millisecond precision. Keep the sort order stable.
        std::thread::sleep(std::time::Duration::from_millis(3));
    }
    // Mark the oldest one as kept.
    set_pinned(&dir, &migration_ids[0], true).unwrap();

    let removed = prune_migration_backups(&dir).unwrap();
    let remaining = list_backups(&dir).unwrap();
    let remaining_ids: Vec<&str> = remaining.iter().map(|m| m.id.as_str()).collect();

    assert!(
        remaining_ids.contains(&manual.id.as_str()),
        "Manual backups must not be cleaned up"
    );
    assert!(
        remaining_ids.contains(&migration_ids[0].as_str()),
        "Backups marked as kept must not be cleaned up"
    );
    assert!(!removed.is_empty(), "Backups over the limit must be cleaned up");
    let kept_migrations = remaining
        .iter()
        .filter(|m| m.kind == BackupKind::PreMigration && !m.pinned)
        .count();
    assert_eq!(kept_migrations, MIGRATION_BACKUP_KEEP);
}

/// A writer in another process: it works only when the parent test calls it through an environment variable (run alone with `--ignored`, it does nothing).
/// After it gets the write lock it prints a marker line, then holds the lock until the parent process kills it.
const LOCK_CHILD_DIR: &str = "ZEPPBRIDGE_TEST_LOCK_CHILD_DIR";
const LOCK_CHILD_HELD: &str = "ZB-LOCK-HELD";

#[test]
#[ignore = "started only by a_writer_killed_in_another_process_frees_the_lock_and_the_queued_restore_runs"]
fn cross_process_lock_holder() {
    let Some(dir) = std::env::var_os(LOCK_CHILD_DIR) else {
        return;
    };
    let _held = write_lock::try_acquire(Path::new(&dir), WritePurpose::Sync).unwrap();
    println!("{LOCK_CHILD_HELD}");
    use std::io::Write as _;
    std::io::stdout().flush().unwrap();
    std::thread::sleep(std::time::Duration::from_secs(120));
}

/// Two real processes: while another process holds the write lock, the queued restore does not touch the library, keeps the queue file, and can say who is writing.
/// After that process is force-ended (crash, Task Manager), the lock is available at once, with no lock file to delete by hand,
/// and the queued restore succeeds the next time. An in-process test cannot prove "a different process" and "the process died".
#[test]
fn a_writer_killed_in_another_process_frees_the_lock_and_the_queued_restore_runs() {
    use std::io::BufRead as _;
    let dir = temp_dir("cross-process");
    drop(seed(&dir, 3));
    let backup = create_backup(&dir, BackupKind::Manual, "1.0.0").unwrap();
    stage_restore(&dir, &backup.id, "1.0.0").unwrap();

    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "storage::backup::tests::cross_process_lock_holder",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(LOCK_CHILD_DIR, &dir)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let mut lines = std::io::BufReader::new(child.stdout.take().unwrap()).lines();
    assert!(
        lines.any(|line| line.is_ok_and(|line| line.contains(LOCK_CHILD_HELD))),
        "The child process exited without getting the write lock"
    );

    match write_lock::try_acquire(&dir, WritePurpose::Metadata) {
        Err(write_lock::WriteLockError::Busy { holder }) => {
            assert!(holder.is_some(), "The waiting side must be able to say what the other process is doing")
        }
        other => panic!("Must be Busy while another process holds the lock: {:?}", other.map(|_| ())),
    }
    let outcome = apply_pending_restore(&dir).expect("a restore is queued");
    assert!(!outcome.succeeded, "{outcome:?}");
    assert_eq!(
        outcome.message_code.as_deref(),
        Some("err.backup.restore_busy")
    );
    assert!(pending_restore(&dir).is_some());

    child.kill().unwrap();
    child.wait().unwrap();
    drop(
        write_lock::acquire_with_timeout(
            &dir,
            WritePurpose::Metadata,
            std::time::Duration::from_secs(2),
        )
        .expect("A process that was ended must not leave a lock"),
    );
    let outcome = apply_pending_restore(&dir).expect("the queue file is still there");
    assert!(outcome.succeeded, "{outcome:?}");
    assert!(pending_restore(&dir).is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn restore_resumes_at_each_boundary_while_moving_the_original_library() {
    use rusqlite::config::DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE;

    for moved_sidecars in 0..=SQLITE_SIDECARS.len() {
        let dir = temp_dir("restore-interrupted");
        let db = seed(&dir, 3);
        db.conn
            .execute_batch(
                "CREATE TABLE restore_marker(value TEXT);
                               INSERT INTO restore_marker VALUES ('snapshot');",
            )
            .unwrap();
        drop(db);
        let backup = create_backup(&dir, BackupKind::Manual, "1.0.0").unwrap();
        let pending = stage_restore(&dir, &backup.id, "1.0.0").unwrap();

        let live = database_path(&dir);
        let previous = dir.join("zepp.db.restore-previous");
        let conn = rusqlite::Connection::open(&live).unwrap();
        conn.set_db_config(SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE, true)
            .unwrap();
        conn.execute_batch(
            "PRAGMA wal_autocheckpoint=0;
                            UPDATE restore_marker SET value='original WAL';",
        )
        .unwrap();
        drop(conn);
        assert!(sidecar(&live, "-wal").exists());

        std::fs::rename(&live, &previous).unwrap();
        for suffix in SQLITE_SIDECARS.into_iter().take(moved_sidecars) {
            if sidecar(&live, suffix).exists() {
                std::fs::rename(sidecar(&live, suffix), sidecar(&previous, suffix)).unwrap();
            }
        }

        assert!(Database::open_migrated(&live).is_err());
        let outcome = apply_pending_restore(&dir).unwrap();
        assert!(outcome.succeeded, "{outcome:?}");
        assert!(pending_restore(&dir).is_none());
        assert!(!sidecar(&live, "-wal").exists());
        for path in [&live, &snapshot_path(&dir, &pending.rollback_backup_id)] {
            let db = Database::open_read_only_any_version(path.clone()).unwrap();
            let value: String = db
                .conn
                .query_row("SELECT value FROM restore_marker", [], |row| row.get(0))
                .unwrap();
            assert_eq!(value, "snapshot");
        }
    }
}

#[test]
fn a_leftover_original_copy_does_not_block_the_next_restore() {
    let dir = temp_dir("restore-leftover-previous");
    drop(seed(&dir, 3));
    let backup = create_backup(&dir, BackupKind::Manual, "1.0.0").unwrap();
    // The last restore already finished; only the old library copy was not deleted (this happens on Windows when a handle is not released).
    let previous = dir.join("zepp.db.restore-previous");
    std::fs::copy(database_path(&dir), &previous).unwrap();

    stage_restore(&dir, &backup.id, "1.0.0").unwrap();
    assert!(!previous.exists());
    assert!(Database::open_migrated(&database_path(&dir)).is_ok());
    let outcome = apply_pending_restore(&dir).unwrap();
    assert!(outcome.succeeded, "{outcome:?}");
    assert!(pending_restore(&dir).is_none());
    let _ = std::fs::remove_dir_all(&dir);
}
