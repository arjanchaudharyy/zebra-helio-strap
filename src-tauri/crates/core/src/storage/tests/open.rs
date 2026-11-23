//! Open, read-only connections, upgrading a real old library, and rescue and quarantine of a corrupt library.

use super::*;

/// Upgrade rehearsal on a real old library. Skipped by default: it needs a real old database.
///
/// A small synthetic library cannot prove the upgrade is safe. What really breaks is a library of hundreds of MB that crosses several
/// schema versions and holds all kinds of legacy rows. This rehearsal stays in the repo so that
/// each new migration step can run against a real library, instead of a last-minute fix on release day.
///
/// ```powershell
/// $env:ZEPPBRIDGE_UPGRADE_FIXTURE = "D:/somewhere/a-copy-of/zepp.db"
/// cargo test --manifest-path src-tauri/Cargo.toml -p zeppbridge-core --jobs 1 `
///   -- --ignored upgrade_a_real_old_database
/// ```
///
/// **Pass a copy.** This test really migrates the file you point it at.
#[test]
#[ignore = "needs a real old library; set ZEPPBRIDGE_UPGRADE_FIXTURE to a copy"]
fn upgrade_a_real_old_database_without_losing_rows() {
    let Ok(source) = std::env::var("ZEPPBRIDGE_UPGRADE_FIXTURE") else {
        panic!("ZEPPBRIDGE_UPGRADE_FIXTURE is not set");
    };
    let source = PathBuf::from(source);
    let dir = std::env::temp_dir().join("zeppbridge-upgrade-drill");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("zepp.db");
    std::fs::copy(&source, &path).expect("copy the old library");

    // Record the row counts of a few key tables before the upgrade. An upgrade only adds structure and must not remove facts.
    // daily_metrics is the exception: the v4 canonical key dedup and the v30 cleanup of readiness sentinel rows are
    // deliberate deletions, so for it we compare "distinct fact keys" and not raw row counts.
    let before = {
        let conn = Connection::open(&path).unwrap();
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        let counts: Vec<(String, i64)> = ["raw_records", "workouts", "metric_samples"]
            .iter()
            .filter_map(|table| {
                conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                    row.get::<_, i64>(0)
                })
                .ok()
                .map(|count| ((*table).to_string(), count))
            })
            .collect();
        let daily_keys: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM (
                        SELECT DISTINCT date, metric, unit, source_scope,
                            COALESCE(device_id, '')
                        FROM daily_metrics
                        WHERE NOT (value = 255 AND metric IN (
                            'readiness', 'physical_readiness', 'mental_readiness',
                            'hrv_readiness', 'rhr_readiness', 'skin_temp_readiness',
                            'afib_readiness', 'ahi_readiness'))
                    )",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        (version, counts, daily_keys)
    };
    assert!(
        before.0 < CURRENT_SCHEMA_VERSION,
        "This fixture is already v{}, so it cannot rehearse an upgrade",
        before.0
    );

    let db = Database::open_migrated(&path).expect("the upgrade must succeed");
    assert_eq!(
        db.diagnostic_schema_version().unwrap(),
        CURRENT_SCHEMA_VERSION
    );

    for (table, count) in &before.1 {
        let after: i64 = db
            .conn
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .unwrap();
        assert!(
            after >= *count,
            "{table} dropped from {count} to {after}: an upgrade must not lose facts"
        );
    }
    let daily_keys_after: i64 = db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM (
                    SELECT DISTINCT date, metric, unit, source_scope,
                        COALESCE(device_id, '')
                    FROM daily_metrics
                    WHERE NOT (value = 255 AND metric IN (
                        'readiness', 'physical_readiness', 'mental_readiness',
                        'hrv_readiness', 'rhr_readiness', 'skin_temp_readiness',
                        'afib_readiness', 'ahi_readiness'))
                )",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);
    assert!(
        daily_keys_after >= before.2,
        "daily_metrics distinct fact keys dropped from {} to {}: an upgrade must not lose facts",
        before.2,
        daily_keys_after
    );

    // After the upgrade, when the library passes the integrity check, the pre-upgrade snapshot is silently deleted to save disk space.
    // Creating and verifying the snapshot itself is covered by the backup tests.
    let backups = backup::list_backups(&dir).expect("read the backup list");
    assert!(
        !backups
            .iter()
            .any(|item| item.kind == backup::BackupKind::PreMigration),
        "A healthy library must not keep the pre-upgrade snapshot"
    );
    assert!(
        !std::fs::read_dir(backup::backup_dir(&dir))
            .map(|entries| entries.count() > 0)
            .unwrap_or(false),
        "The snapshot and its -wal / -shm must all be removed"
    );
}

#[test]
fn a_read_only_connection_refuses_writes_and_ignores_the_write_lock() {
    // The MCP and CLI query paths depend on these two properties: the connection layer guarantees that writes fail,
    // not the caller's discipline; and a long sync that holds the write lock must not block read-only queries.
    let dir = std::env::temp_dir().join("zeppbridge-readonly-contract");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("zepp.db");
    drop(Database::open_migrated(&path).unwrap());

    let _writer =
        write_lock::try_acquire(&dir, write_lock::WritePurpose::Sync).expect("let a writer hold the lock first");

    let reader = Database::open_read_only(path).expect("a read-only connection must not be blocked by the write lock");
    assert!(reader.get_recent_workouts(1).is_ok(), "Queries must work while a sync runs");
    let write_attempt = reader
        .conn
        .execute("DELETE FROM raw_records", [])
        .map_err(|error| error.to_string());
    assert!(write_attempt.is_err(), "A read-only connection must refuse writes at the SQLite layer");
}

#[test]
fn the_pre_migration_backup_can_still_read_a_database_that_is_out_of_date() {
    // "A read-only connection must match the version" is a policy for user queries, not a file-open mechanism.
    // If it were in the mechanism, the pre-upgrade auto backup could not open the old library, so the backup would fail,
    // and the migration would refuse to start: no existing user could upgrade.
    let dir = std::env::temp_dir().join("zeppbridge-premigration-open");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("zepp.db");
    drop(Database::open_migrated(&path).unwrap());

    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(&format!(
        "PRAGMA user_version = {};",
        CURRENT_SCHEMA_VERSION - 1
    ))
    .unwrap();
    drop(conn);

    assert!(
        Database::open_read_only(path.clone()).is_err(),
        "The entry point for user queries must still block a library with a different version"
    );
    assert!(
        Database::open_read_only_any_version(path.clone()).is_ok(),
        "Backup and restore must read old library versions: that is why they exist"
    );
    // The migration path must run all the way (the auto backup in the middle is deleted once the upgrade is done and the library is healthy).
    drop(Database::open_migrated(&path).expect("the old library must upgrade"));
}

#[test]
fn a_read_only_open_says_which_side_is_out_of_date_instead_of_failing_later() {
    // An old library with a new program keeps hitting "no such table", and the user sees only
    // "database temporarily unavailable", with no reason and no next step.
    let dir = std::env::temp_dir().join("zeppbridge-readonly-schema");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("zepp.db");
    drop(Database::open_migrated(&path).unwrap());

    // When the library is older than the program, both paths must appear. Saying only "start the desktop app once"
    // means nothing for a library in a container: there is no desktop app there.
    for (version, expected) in [
        (
            CURRENT_SCHEMA_VERSION - 1,
            vec!["zeppbridge-cli reprocess", "desktop app"],
        ),
        (
            CURRENT_SCHEMA_VERSION + 1,
            vec!["ZeppBridge.mcpb", "Reconnect", env!("CARGO_PKG_VERSION")],
        ),
    ] {
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(&format!("PRAGMA user_version = {version};"))
            .unwrap();
        drop(conn);
        let message = match Database::open_read_only(path.clone()) {
            Ok(_) => panic!("A version mismatch must be reported at open time"),
            Err(error) => error.user_message(),
        };
        for needle in expected {
            assert!(message.contains(needle), "{message}");
        }
    }
}

#[test]
fn current_schema_marker_survives_repeated_idempotent_migrations() {
    let db = Database::in_memory().unwrap();
    assert_eq!(
        db.diagnostic_schema_version().unwrap(),
        CURRENT_SCHEMA_VERSION
    );
    db.migrate().unwrap();
    assert_eq!(
        db.diagnostic_schema_version().unwrap(),
        CURRENT_SCHEMA_VERSION
    );
}

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "zeppbridge-storage-{}-{}-{}",
        label,
        std::process::id(),
        crate::unique_suffix()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn inflate_page_count(path: &Path, extra_pages: u32) {
    use std::fs::OpenOptions;
    use std::io::{Read, Seek, SeekFrom, Write};
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .unwrap();
    let mut header = [0u8; 32];
    file.read_exact(&mut header).unwrap();
    let claimed = u32::from_be_bytes(header[28..32].try_into().unwrap());
    file.seek(SeekFrom::Start(28)).unwrap();
    file.write_all(&(claimed + extra_pages).to_be_bytes())
        .unwrap();
}

#[test]
fn salvage_aligns_truncated_sqlite_page_count() {
    let dir = temp_dir("salvage");
    let path = dir.join("zepp.db");
    {
        let db = Database::new(path.clone()).unwrap();
        db.insert_metric_sample(&MetricSample {
            metric: "heart_rate".into(),
            timestamp: ts(),
            value: 70.0,
            unit: "bpm".into(),
            source_scope: SourceScope::Unknown,
            device_id: None,
        })
        .unwrap();
    }
    let _ = std::fs::remove_file(dir.join("zepp.db-wal"));
    let _ = std::fs::remove_file(dir.join("zepp.db-shm"));
    inflate_page_count(&path, 24);
    assert!(Database::new(path.clone()).is_err());
    let (db, warning) = Database::open_resilient(path.clone()).unwrap();
    assert!(warning.unwrap().contains("truncated"));
    assert_eq!(db.count_metric_samples().unwrap(), 1);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn corrupt_library_is_quarantined_and_app_still_starts() {
    let dir = temp_dir("quarantine");
    let path = dir.join("zepp.db");
    std::fs::write(&path, b"this is not a sqlite database").unwrap();
    let (db, warning) = Database::open_resilient(path.clone()).unwrap();
    assert!(warning.unwrap().contains("corrupt"));
    assert!(path.exists());
    assert_eq!(db.count_metric_samples().unwrap(), 0);
    let quarantined = std::fs::read_dir(dir.join("backups"))
        .unwrap()
        .filter_map(|entry| entry.ok())
        .any(|entry| entry.file_name().to_string_lossy().starts_with("corrupt-"));
    assert!(quarantined);
    let _ = std::fs::remove_dir_all(dir);
}

/* ---------- Recovery keeps the original first, handles the file group together, and changes nothing on failure ---------- */

fn corrupt_dirs(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut dirs: Vec<_> = std::fs::read_dir(dir.join("backups"))
        .map(|entries| {
            entries
                .filter_map(|entry| entry.ok())
                .map(|entry| entry.path())
                .filter(|path| {
                    path.file_name()
                        .is_some_and(|name| name.to_string_lossy().starts_with("corrupt-"))
                })
                .collect()
        })
        .unwrap_or_default();
    dirs.sort();
    dirs
}

#[test]
fn salvage_keeps_the_untouched_original_before_patching_the_header() {
    let dir = temp_dir("salvage-original");
    let path = dir.join("zepp.db");
    {
        let db = Database::new(path.clone()).unwrap();
        db.insert_metric_sample(&MetricSample {
            metric: "heart_rate".into(),
            timestamp: ts(),
            value: 70.0,
            unit: "bpm".into(),
            source_scope: SourceScope::Unknown,
            device_id: None,
        })
        .unwrap();
    }
    let _ = std::fs::remove_file(dir.join("zepp.db-wal"));
    let _ = std::fs::remove_file(dir.join("zepp.db-shm"));
    inflate_page_count(&path, 24);
    let before = std::fs::read(&path).unwrap();

    let (db, warning) = Database::open_resilient(path.clone()).unwrap();
    assert!(warning.unwrap().contains("original"));
    assert_eq!(db.count_metric_samples().unwrap(), 1);
    let kept = corrupt_dirs(&dir);
    assert_eq!(kept.len(), 1);
    assert_eq!(std::fs::read(kept[0].join("zepp.db")).unwrap(), before);
    drop(db);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn two_recoveries_in_the_same_second_never_share_a_quarantine_directory() {
    let dir = temp_dir("same-second");
    let path = dir.join("zepp.db");
    std::fs::write(&path, b"first broken file").unwrap();
    let first = crate::paths::preserve_sqlite_group(&path).unwrap();
    std::fs::write(&path, b"second broken file").unwrap();
    let second = crate::paths::preserve_sqlite_group(&path).unwrap();
    assert_ne!(first, second);
    assert_eq!(
        std::fs::read(first.join("zepp.db")).unwrap(),
        b"first broken file"
    );
    assert_eq!(
        std::fs::read(second.join("zepp.db")).unwrap(),
        b"second broken file"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_corrupt_library_that_cannot_be_preserved_is_left_exactly_as_it_was() {
    let dir = temp_dir("cannot-preserve");
    let path = dir.join("zepp.db");
    std::fs::write(&path, b"this is not a sqlite database").unwrap();
    // backups is a file: the quarantine directory cannot be created, so the original cannot be saved.
    std::fs::write(dir.join("backups"), b"").unwrap();
    let (db, warning) = Database::open_resilient(path.clone()).unwrap();
    assert!(warning.unwrap().contains("left untouched"));
    assert_eq!(db.count_metric_samples().unwrap(), 0, "The temporary empty library still works");
    assert_eq!(
        std::fs::read(&path).unwrap(),
        b"this is not a sqlite database"
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// The page cache must not be shared between connections (`LIBSQLITE3_FLAGS` in `.cargo/config.toml`).
///
/// When shared, one connection that scans a large range fills the whole pool, the other connections' hit rate drops to zero, and large reads get about 10 times slower.
/// A build that skips this config (for example a separate cargo call elsewhere) fails here.
#[test]
fn sqlite_page_caches_are_not_shared_between_connections() {
    let db = Database::in_memory().unwrap();
    let shared: i64 = db
        .conn
        .query_row(
            "SELECT sqlite_compileoption_used('ENABLE_MEMORY_MANAGEMENT')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        shared, 0,
        "SQLite was compiled with SQLITE_ENABLE_MEMORY_MANAGEMENT"
    );
}

/* ---------- 3.1.1 feedback: read-only open failed every time (CLI exit 6, MCP fully unavailable) ---------- */

fn seeded_library(dir: &Path) -> PathBuf {
    let path = dir.join("zepp.db");
    let db = Database::new(path.clone()).unwrap();
    db.insert_metric_sample(&MetricSample {
        metric: "heart_rate".into(),
        timestamp: ts(),
        value: 70.0,
        unit: "bpm".into(),
        source_scope: SourceScope::Unknown,
        device_id: None,
    })
    .unwrap();
    path
}

fn set_read_only(path: &Path, read_only: bool) {
    let mut permissions = std::fs::metadata(path).unwrap().permissions();
    #[allow(clippy::permissions_set_readonly_false)]
    permissions.set_readonly(read_only);
    std::fs::set_permissions(path, permissions).unwrap();
}

#[test]
fn read_only_open_reads_a_library_with_a_truncated_header_without_writing() {
    let dir = temp_dir("ro-truncated");
    let path = seeded_library(&dir);
    let _ = std::fs::remove_file(dir.join("zepp.db-wal"));
    let _ = std::fs::remove_file(dir.join("zepp.db-shm"));
    inflate_page_count(&path, 24);
    let before = std::fs::read(&path).unwrap();
    // Without the fallback, this step is the 3.1.1 "database temporarily unavailable": a read-only connection cannot fix the page header.
    let plain = Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .unwrap()
        .query_row("SELECT count(*) FROM sqlite_master", [], |row| {
            row.get::<_, i64>(0)
        });
    assert!(plain.is_err(), "A plain read-only open must fail after the page header is inflated");

    let db = Database::open_read_only(path.clone()).expect("a truncated library must still open read-only");
    assert_eq!(db.read_fallback(), Some(ReadFallback::TruncatedHeader));
    assert_eq!(db.count_metric_samples().unwrap(), 1);
    assert!(db.is_query_only());
    drop(db);
    assert_eq!(std::fs::read(&path).unwrap(), before, "The read-only fallback must not change the file");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn read_only_open_survives_a_leftover_shm_that_cannot_be_written() {
    let dir = temp_dir("ro-stale-shm");
    let source = temp_dir("ro-stale-shm-source");
    let source_path = seeded_library(&source);
    // Copy the whole file group while the write connection is still open: the same as the -wal / -shm left after the process is killed.
    // Checkpoint and truncate the WAL first, so what remains is "the app exited, the WAL is checkpointed, the -shm is still there".
    let writer = Connection::open(&source_path).unwrap();
    writer
        .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))
        .unwrap();
    for name in ["zepp.db", "zepp.db-wal", "zepp.db-shm"] {
        std::fs::copy(source.join(name), dir.join(name)).unwrap();
    }
    drop(writer);
    let shm = dir.join("zepp.db-shm");
    let wal = dir.join("zepp.db-wal");
    assert!(shm.exists());
    set_read_only(&shm, true);
    set_read_only(&wal, true);

    let opened = Database::open_read_only(dir.join("zepp.db"));
    set_read_only(&shm, false);
    set_read_only(&wal, false);
    let db = opened.expect("a read-only open must work even when the leftover -shm is not writable");
    eprintln!("stale -shm read fallback: {:?}", db.read_fallback());
    assert_eq!(db.count_metric_samples().unwrap(), 1);
    drop(db);
    let _ = std::fs::remove_dir_all(dir);
    let _ = std::fs::remove_dir_all(source);
}

#[test]
fn read_only_open_failure_names_the_sqlite_code_and_a_masked_path() {
    let dir = temp_dir("ro-missing");
    let path = dir.join("no-such-folder").join("zepp.db");
    let error = match Database::open_read_only(path) {
        Ok(_) => panic!("A library that does not exist must not open"),
        Err(error) => error,
    };
    let message = error.user_message();
    assert!(message.contains("CannotOpen"), "{message}");
    assert!(message.contains("extended 14"), "{message}");
    assert!(message.contains("zepp.db"), "{message}");
    if let Some(home) = directories::BaseDirs::new() {
        let name = home
            .home_dir()
            .file_name()
            .map(|value| value.to_string_lossy().to_string())
            .unwrap_or_default();
        if name.len() > 2 && dir.starts_with(home.home_dir()) {
            assert!(
                !message.contains(&name),
                "The user name must not appear in the error: {message}"
            );
        }
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn immutable_fallback_is_refused_while_the_wal_still_holds_frames() {
    let dir = temp_dir("ro-wal-frames");
    let path = seeded_library(&dir);
    let writer = Connection::open(&path).unwrap();
    writer
        .execute_batch(
            "PRAGMA journal_mode = WAL; CREATE TABLE probe(x); INSERT INTO probe VALUES (1);",
        )
        .unwrap();
    assert!(std::fs::metadata(dir.join("zepp.db-wal")).unwrap().len() > 0);
    let failure = rusqlite::Error::SqliteFailure(
        rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CANTOPEN),
        None,
    );
    assert_eq!(
        crate::storage::open::read_fallback_for(&path, &failure),
        None
    );
    drop(writer);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn read_only_open_falls_back_to_immutable_when_the_shm_cannot_be_opened_at_all() {
    // The ACL denies read, another program holds it exclusively, or it is a folder with the same name: `-shm` cannot open.
    // With no live -wal, the immutable read gets around it (the app exited, the WAL is checkpointed).
    let dir = temp_dir("ro-shm-blocked");
    let path = seeded_library(&dir);
    let _ = std::fs::remove_file(dir.join("zepp.db-wal"));
    let _ = std::fs::remove_file(dir.join("zepp.db-shm"));
    std::fs::create_dir(dir.join("zepp.db-shm")).unwrap();
    let plain = Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .unwrap()
        .query_row("SELECT count(*) FROM sqlite_master", [], |row| {
            row.get::<_, i64>(0)
        });
    let db = Database::open_read_only(path).expect("a read-only open must work even when -shm cannot open");
    // On Windows a plain read-only open fails and must use immutable. On Linux / macOS with no -wal a plain read-only open works anyway
    // (CI), so the fallback must not be used.
    if plain.is_err() {
        assert_eq!(db.read_fallback(), Some(ReadFallback::Immutable));
    } else {
        assert_eq!(db.read_fallback(), None);
    }
    assert_eq!(db.count_metric_samples().unwrap(), 1);
    assert!(db.is_query_only());
    drop(db);
    let _ = std::fs::remove_dir_all(dir);
}
