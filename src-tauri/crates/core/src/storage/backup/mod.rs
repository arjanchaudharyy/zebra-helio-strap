use super::write_lock::{self, WritePurpose};

use super::{Database, CURRENT_SCHEMA_VERSION, NORMALIZER_REVISION};

use crate::models::{error::Result, ZeppBridgeError};

use chrono::Utc;

use serde::{Deserialize, Serialize};

use sha2::{Digest, Sha256};

use std::collections::BTreeMap;

use std::path::{Path, PathBuf};

mod restore;

pub use restore::*;

pub const BACKUP_DIR: &str = "backups";

const PENDING_RESTORE_FILE: &str = "restore-pending.json";

/// How many pre-migration backups the rolling cleanup keeps. Enough to go back a few versions, without filling the disk.
pub const MIGRATION_BACKUP_KEEP: usize = 5;

/// Which tables the manifest counts. The order is fixed so diffs stay readable.
const COUNTED_TABLES: [&str; 7] = [
    "raw_records",
    "metric_samples",
    "daily_metrics",
    "sleep_sessions",
    "workouts",
    "workout_samples",
    "life_events",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackupKind {
    /// Created by the user.
    Manual,
    /// Created automatically before a schema migration.
    PreMigration,
    /// A rollback snapshot of the current database, taken before a restore.
    PreRestore,
}

impl BackupKind {
    fn prefix(self) -> &'static str {
        match self {
            BackupKind::Manual => "manual",
            BackupKind::PreMigration => "pre-migration",
            BackupKind::PreRestore => "pre-restore",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            BackupKind::Manual => "Manual backup",
            BackupKind::PreMigration => "Automatic backup before upgrade",
            BackupKind::PreRestore => "Rollback backup before restore",
        }
    }
}

/// The health data range a snapshot covers. The user sees "which period of data is in this backup".
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupCoverage {
    pub earliest_sample_at: Option<String>,
    pub latest_sample_at: Option<String>,
    pub last_cloud_sync_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupManifest {
    /// The file name stem. It is also the identifier for a restore.
    pub id: String,
    pub created_at: String,
    pub app_version: String,
    pub schema_version: i64,
    pub normalizer_revision: String,
    pub kind: BackupKind,
    pub coverage: BackupCoverage,
    pub table_counts: BTreeMap<String, i64>,
    pub bytes: u64,
    pub sha256: String,
    /// The result of the check right after creation. A snapshot with false does not go into the restorable list.
    pub integrity_ok: bool,
    /// The user marked it "do not clean up automatically". Rolling cleanup always skips it.
    #[serde(default)]
    pub pinned: bool,
}

/// The result of verifying an existing snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupVerification {
    pub id: String,
    pub file_present: bool,
    pub bytes_match: bool,
    pub sha256_match: bool,
    pub integrity_ok: bool,
    /// The raw message text. The UI uses `problem_code` first and shows this only as a fallback.
    pub problem: Option<String>,
    /// Stable code for the failure reason. The UI uses it to get the text in its own language.
    #[serde(default)]
    pub problem_code: Option<String>,
}

impl BackupVerification {
    pub fn is_usable(&self) -> bool {
        self.file_present && self.bytes_match && self.sha256_match && self.integrity_ok
    }
}

/// Restore compatibility check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RestoreCompatibility {
    /// The schema matches the current one. Restore directly.
    SameSchema,
    /// Older than the current schema. The restore migrates it forward.
    OlderSchemaWillMigrate,
    /// Newer than the current schema: this snapshot comes from a newer ZeppBridge version. Opening it on an older version silently drops fields,
    /// so we refuse it and do not touch the current database at all.
    FutureSchemaRefused,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RestorePreview {
    pub manifest: BackupManifest,
    pub verification: BackupVerification,
    pub compatibility: RestoreCompatibility,
    pub current_schema_version: i64,
    /// Row counts of each table in the current database, shown next to the snapshot so the user sees the differences.
    pub current_table_counts: BTreeMap<String, i64>,
    /// Whether the restore is possible. When it is not, `blocker` gives the reason.
    pub can_restore: bool,
    pub blocker: Option<String>,
}

/// A restore that is queued and runs at the next startup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingRestore {
    pub backup_id: String,
    pub staged_at: String,
    /// The id of the rollback snapshot of the current database, taken before the restore.
    pub rollback_backup_id: String,
}

/// The result after a restore actually runs, for the startup path to show to the user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RestoreOutcome {
    pub backup_id: String,
    pub rollback_backup_id: String,
    pub succeeded: bool,
    pub message: String,
    /// Stable code for `message`. The startup path currently shows the raw text as a fallback.
    #[serde(default)]
    pub message_code: Option<String>,
}

pub fn backup_dir(data_dir: &Path) -> PathBuf {
    data_dir.join(BACKUP_DIR)
}

/// The valid shape of a backup id.
///
/// The rule that creates ids is `<prefix>-<UTC timestamp>` (see `create_backup`), so a real id
/// uses only letters, digits, `-` and `_`.
///
/// We check it not because anyone can pass a bad value now: all ids currently come from our own directory listing.
/// We check it because the id ends up in a file path (`{id}.db` / `{id}.json`). If the render process is ever
/// compromised, or a new entry point passes user input here, `../../` lands directly on disk.
/// This layer guards against that day.
fn validate_backup_id(id: &str) -> Result<()> {
    let ok = !id.is_empty()
        && id.len() <= 128
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'));
    if ok {
        Ok(())
    } else {
        Err(ZeppBridgeError::ConfigError("Invalid backup ID".into()))
    }
}

fn snapshot_path(data_dir: &Path, id: &str) -> PathBuf {
    backup_dir(data_dir).join(format!("{id}.db"))
}

fn manifest_path(data_dir: &Path, id: &str) -> PathBuf {
    backup_dir(data_dir).join(format!("{id}.json"))
}

fn database_path(data_dir: &Path) -> PathBuf {
    data_dir.join("zepp.db")
}

/// Create a consistent snapshot.
///
/// Copy from the source database to a new file with the SQLite Backup API, then verify at once and write the manifest.
/// If the check fails, delete the partial file and return an error. Never leave a bad backup that looks successful.
pub fn create_backup(
    data_dir: &Path,
    kind: BackupKind,
    app_version: &str,
) -> Result<BackupManifest> {
    let source_path = database_path(data_dir);
    if !source_path.exists() {
        return Err(ZeppBridgeError::DataUnavailable(
            "There is no database on this machine yet, so there is nothing to back up".into(),
        ));
    }
    let dir = backup_dir(data_dir);
    std::fs::create_dir_all(&dir)?;

    let created_at = Utc::now();
    let id = format!(
        "{}-{}",
        kind.prefix(),
        created_at.format("%Y%m%dT%H%M%S%3fZ")
    );
    let target = snapshot_path(data_dir, &id);

    let result = (|| -> Result<BackupManifest> {
        let source = rusqlite::Connection::open(&source_path)?;
        {
            let mut destination = rusqlite::Connection::open(&target)?;
            let backup = rusqlite::backup::Backup::new(&source, &mut destination)?;
            // Copy in one step (nPage is larger than the page count of any real database). A step-by-step copy restarts
            // the whole copy when the source database gets a write. That has no value for a caller that just took the write lock,
            // and it makes the time for a large database unpredictable.
            backup.run_to_completion(i32::MAX, std::time::Duration::ZERO, None)?;
        }

        let snapshot = Database::open_read_only_any_version(target.clone())?;
        let first: String = snapshot
            .conn
            .query_row("PRAGMA integrity_check(1)", [], |row| row.get(0))?;
        let integrity_ok = first.eq_ignore_ascii_case("ok");
        if !integrity_ok {
            return Err(ZeppBridgeError::DataUnavailable(format!(
                "The new backup did not pass the integrity check: {first}"
            )));
        }
        let schema_version: i64 = snapshot
            .conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))?;
        let mut table_counts = BTreeMap::new();
        for table in COUNTED_TABLES {
            let count: i64 = snapshot
                .conn
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                    row.get(0)
                })
                .unwrap_or(0);
            table_counts.insert(table.to_string(), count);
        }
        let coverage = BackupCoverage {
            earliest_sample_at: snapshot
                .conn
                .query_row("SELECT MIN(start_utc) FROM raw_records", [], |row| {
                    row.get(0)
                })
                .unwrap_or(None),
            latest_sample_at: snapshot
                .conn
                .query_row("SELECT MAX(start_utc) FROM raw_records", [], |row| {
                    row.get(0)
                })
                .unwrap_or(None),
            last_cloud_sync_at: snapshot
                .cloud_sync_metadata()
                .map(|(at, _)| at)
                .unwrap_or(None),
        };
        let normalizer_revision = snapshot
            .conn
            .query_row(
                "SELECT value FROM app_meta WHERE key = 'normalizer_revision'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap_or_else(|_| NORMALIZER_REVISION.to_string());
        drop(snapshot);

        let bytes = std::fs::metadata(&target)?.len();
        let sha256 = file_sha256(&target)?;
        let manifest = BackupManifest {
            id: id.clone(),
            created_at: created_at.to_rfc3339(),
            app_version: app_version.to_string(),
            schema_version,
            normalizer_revision,
            kind,
            coverage,
            table_counts,
            bytes,
            sha256,
            integrity_ok,
            pinned: false,
        };
        write_manifest(data_dir, &manifest)?;
        Ok(manifest)
    })();

    if result.is_err() {
        // A partial file must not stay in the backup directory and pass as a usable snapshot.
        let _ = std::fs::remove_file(&target);
        let _ = std::fs::remove_file(manifest_path(data_dir, &id));
    }
    result
}

fn write_manifest(data_dir: &Path, manifest: &BackupManifest) -> Result<()> {
    validate_backup_id(&manifest.id)?;
    let encoded = serde_json::to_vec_pretty(manifest)
        .map_err(|error| ZeppBridgeError::ParseError(format!("Cannot create the backup manifest: {error}")))?;
    std::fs::write(manifest_path(data_dir, &manifest.id), encoded)?;
    Ok(())
}

pub fn file_sha256(path: &Path) -> Result<String> {
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// List all snapshots, newest first. Files whose manifest cannot be read are skipped.
pub fn list_backups(data_dir: &Path) -> Result<Vec<BackupManifest>> {
    let dir = backup_dir(data_dir);
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(manifest) = serde_json::from_str::<BackupManifest>(&text) else {
            continue;
        };
        if validate_backup_id(&manifest.id).is_err()
            || path.file_stem().and_then(|value| value.to_str()) != Some(manifest.id.as_str())
        {
            continue;
        }
        out.push(manifest);
    }
    out.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(out)
}

pub fn load_manifest(data_dir: &Path, id: &str) -> Result<BackupManifest> {
    validate_backup_id(id)?;
    let text = std::fs::read_to_string(manifest_path(data_dir, id))?;
    let manifest: BackupManifest = serde_json::from_str(&text)
        .map_err(|error| ZeppBridgeError::ParseError(format!("Cannot parse the backup manifest: {error}")))?;
    validate_backup_id(&manifest.id)?;
    if manifest.id != id {
        return Err(ZeppBridgeError::ConfigError("Invalid backup ID".into()));
    }
    Ok(manifest)
}

/// The user marks or unmarks "do not clean up automatically".
///
/// This changes the manifest file, so it must take the write lock. Otherwise a cleanup could delete snapshots by "not pinned" while
/// the UI pins the same one, and the two sides would see two different lists.
pub fn set_pinned(data_dir: &Path, id: &str, pinned: bool) -> Result<BackupManifest> {
    let _guard = write_lock::try_acquire(data_dir, WritePurpose::Backup).map_err(map_write_lock)?;
    set_pinned_unlocked(data_dir, id, pinned)
}

/// Use this when the caller already holds the write lock (Tauri command layer).
pub fn set_pinned_unlocked(data_dir: &Path, id: &str, pinned: bool) -> Result<BackupManifest> {
    validate_backup_id(id)?;
    let mut manifest = load_manifest(data_dir, id)?;
    manifest.pinned = pinned;
    write_manifest(data_dir, &manifest)?;
    Ok(manifest)
}

fn map_write_lock(error: write_lock::WriteLockError) -> ZeppBridgeError {
    match error {
        write_lock::WriteLockError::Busy { .. } => ZeppBridgeError::Busy(error.to_string()),
        other => ZeppBridgeError::ConfigError(other.to_string()),
    }
}

/// Verify a snapshot again: is the file there, is the size right, is the SHA-256 right, does it pass
/// `integrity_check`.
pub fn verify_backup(data_dir: &Path, id: &str) -> Result<BackupVerification> {
    validate_backup_id(id)?;
    let manifest = load_manifest(data_dir, id)?;
    let path = snapshot_path(data_dir, id);
    if !path.exists() {
        return Ok(BackupVerification {
            id: id.to_string(),
            file_present: false,
            bytes_match: false,
            sha256_match: false,
            integrity_ok: false,
            problem: Some("The backup file is no longer in the backup directory".into()),
            problem_code: Some("ui.backup.file_missing".into()),
        });
    }
    let bytes = std::fs::metadata(&path)?.len();
    let bytes_match = bytes == manifest.bytes;
    let sha256 = file_sha256(&path)?;
    let sha256_match = sha256 == manifest.sha256;
    // When the content does not match, do not open it: SQLite can open a truncated file and return
    // what looks like a normal empty database. That would turn "check failed" into "check passed but no data".
    let integrity_ok = if bytes_match && sha256_match {
        match Database::open_read_only_any_version(path.clone()) {
            Ok(db) => db
                .conn
                .query_row("PRAGMA integrity_check(1)", [], |row| {
                    row.get::<_, String>(0)
                })
                .map(|value| value.eq_ignore_ascii_case("ok"))
                .unwrap_or(false),
            Err(_) => false,
        }
    } else {
        false
    };
    let (problem_code, problem): (Option<&str>, Option<String>) = if !bytes_match {
        (
            Some("ui.backup.size_mismatch"),
            Some("The backup file size does not match the manifest. The file may be damaged".into()),
        )
    } else if !sha256_match {
        (
            Some("ui.backup.sha256_mismatch"),
            Some("The backup file's SHA-256 does not match the manifest. The file may be damaged or changed".into()),
        )
    } else if !integrity_ok {
        (
            Some("ui.backup.integrity_failed"),
            Some("The backup file did not pass the SQLite integrity check".into()),
        )
    } else {
        (None, None)
    };
    Ok(BackupVerification {
        id: id.to_string(),
        file_present: true,
        bytes_match,
        sha256_match,
        integrity_ok,
        problem,
        problem_code: problem_code.map(str::to_string),
    })
}

/// Rolling cleanup of pre-migration backups. Keeps only the latest [`MIGRATION_BACKUP_KEEP`].
///
/// Manual backups and snapshots the user marked to keep are never deleted. Automatic cleanup that deletes backups the user saved
/// is the worst thing this kind of feature can do.
pub fn prune_migration_backups(data_dir: &Path) -> Result<Vec<String>> {
    let all = list_backups(data_dir)?;
    let mut candidates: Vec<&BackupManifest> = all
        .iter()
        .filter(|manifest| manifest.kind == BackupKind::PreMigration && !manifest.pinned)
        .collect();
    candidates.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    let mut removed = Vec::new();
    for manifest in candidates.into_iter().skip(MIGRATION_BACKUP_KEEP) {
        let _ = std::fs::remove_file(snapshot_path(data_dir, &manifest.id));
        let _ = std::fs::remove_file(manifest_path(data_dir, &manifest.id));
        removed.push(manifest.id.clone());
    }
    Ok(removed)
}

/// Delete all automatic pre-upgrade / pre-compaction snapshots (never manual backups or ones marked to keep),
/// together with any `-wal` / `-shm` files they left behind. Call it only after "the upgrade or compaction is complete and the current database passes
/// the integrity check" (once we confirm nothing is lost, delete them silently so they use no disk space).
pub fn discard_automatic_snapshots(data_dir: &Path) -> Result<Vec<String>> {
    let mut removed = Vec::new();
    for manifest in list_backups(data_dir)? {
        if manifest.kind != BackupKind::PreMigration || manifest.pinned {
            continue;
        }
        let snapshot = snapshot_path(data_dir, &manifest.id);
        for path in [
            snapshot.clone(),
            PathBuf::from(format!("{}-wal", snapshot.display())),
            PathBuf::from(format!("{}-shm", snapshot.display())),
            manifest_path(data_dir, &manifest.id),
        ] {
            match std::fs::remove_file(&path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        removed.push(manifest.id);
    }
    Ok(removed)
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod backup_id_tests {
    use super::*;

    #[test]
    fn a_traversing_backup_id_is_rejected_before_it_reaches_the_filesystem() {
        for bad in [
            "",
            "../../etc/passwd",
            r"..\..\windows\system32",
            "auto-2026/09/01",
            "auto 2026",
            "auto-2026\0",
        ] {
            assert!(
                validate_backup_id(bad).is_err(),
                "{bad:?} must not be accepted as a valid backup ID"
            );
        }
        // The shape we really generate must stay valid. Otherwise this check would turn off the whole backup feature.
        assert!(validate_backup_id("auto-20260901T124736123Z").is_ok());
        assert!(validate_backup_id("manual-20260901T124736123Z").is_ok());
    }
}
