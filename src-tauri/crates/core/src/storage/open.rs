//! Open, migrate, read-only connections and quarantine of corrupt libraries (split from storage/mod.rs by domain, same logic).

use super::*;

pub fn is_corrupt_error(error: &ZeppBridgeError) -> bool {
    match error {
        ZeppBridgeError::DatabaseError(inner) => is_corrupt_sqlite(inner),
        other => looks_corrupt(&other.to_string()),
    }
}

pub(super) fn is_corrupt_sqlite(error: &rusqlite::Error) -> bool {
    match error {
        rusqlite::Error::SqliteFailure(code, message) => {
            matches!(
                code.code,
                rusqlite::ErrorCode::DatabaseCorrupt | rusqlite::ErrorCode::NotADatabase
            ) || message.as_deref().is_some_and(looks_corrupt)
        }
        other => looks_corrupt(&other.to_string()),
    }
}

pub(super) fn looks_corrupt(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("malformed")
        || lower.contains("not a database")
        || lower.contains("database disk image")
        || lower.contains("file is not a database")
}

/// If the SQLite header claims more pages than the file actually has, rewrite
/// the page count. This is the usual leftover of a force-killed WAL checkpoint
/// or index rebuild.
/// The two read modes of the read-only fallback.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadFallback {
    /// `immutable=1`: does not touch `-shm` and takes no lock. Use it only when no live `-wal` exists.
    Immutable,
    /// Header page count is larger than the file: `writable_schema` makes SQLite read the real length. Nothing is written to disk.
    TruncatedHeader,
}

impl ReadFallback {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Immutable => "immutable",
            Self::TruncatedHeader => "truncated_header",
        }
    }
}

/// After the first read-only open fails: can we retry, and with which read mode?
pub(super) fn read_fallback_for(db_path: &Path, error: &rusqlite::Error) -> Option<ReadFallback> {
    let rusqlite::Error::SqliteFailure(..) = error else {
        return None;
    };
    if is_corrupt_sqlite(error) {
        return header_claims_missing_pages(db_path)
            .unwrap_or(false)
            .then_some(ReadFallback::TruncatedHeader);
    }
    // `-wal` still has frames: immutable would miss those committed transactions, so we prefer an error.
    let wal_idle = match std::fs::metadata(sqlite_sidecar(db_path, "-wal")) {
        Ok(meta) => meta.len() == 0,
        Err(error) => error.kind() == std::io::ErrorKind::NotFound,
    };
    (wal_idle && immutable_uri(db_path).is_some()).then_some(ReadFallback::Immutable)
}

fn sqlite_sidecar(db_path: &Path, suffix: &str) -> PathBuf {
    let mut name = db_path.as_os_str().to_os_string();
    name.push(suffix);
    PathBuf::from(name)
}

/// The path as a `file:` URI with `immutable=1`. UNC paths are not built (SQLite URIs
/// do not accept a host name), so this fallback is skipped for them.
fn immutable_uri(db_path: &Path) -> Option<String> {
    let text = db_path.to_str()?;
    let text = text.strip_prefix(r"\\?\").unwrap_or(text);
    if text.starts_with(r"\\") {
        return None;
    }
    let mut encoded = String::with_capacity(text.len() + 32);
    for character in text.chars() {
        match character {
            '\\' => encoded.push('/'),
            '%' => encoded.push_str("%25"),
            '?' => encoded.push_str("%3f"),
            '#' => encoded.push_str("%23"),
            other => encoded.push(other),
        }
    }
    let prefix = if encoded.starts_with('/') {
        "file://"
    } else {
        "file:///"
    };
    Some(format!("{prefix}{encoded}?mode=ro&immutable=1"))
}

/// Add the masked path to an open error. Keep the error code as is (the corruption check needs it).
fn annotate_with_path(error: rusqlite::Error, db_path: &Path) -> rusqlite::Error {
    match error {
        rusqlite::Error::SqliteFailure(code, message) => {
            let masked = crate::paths::mask_user_path(db_path);
            // When rusqlite cannot open the file, it appends the full path (with the user name) to the message. Replace it with the masked path.
            let base = message
                .unwrap_or_else(|| code.to_string())
                .replace(&db_path.display().to_string(), &masked);
            let text = if base.contains(&masked) {
                base
            } else {
                format!("{base}; read-only open of {masked}")
            };
            rusqlite::Error::SqliteFailure(code, Some(text))
        }
        other => other,
    }
}

/// Diagnostic text for a SQLite error: error class, extended code and message, for example
/// `CannotOpen (extended 14): unable to open database file`. Contains no SQL parameter values.
pub fn sqlite_error_detail(error: &rusqlite::Error) -> String {
    let text = match error {
        rusqlite::Error::SqliteFailure(code, message) => format!(
            "{:?} (extended {}): {}",
            code.code,
            code.extended_code,
            message.clone().unwrap_or_else(|| code.to_string())
        ),
        other => other.to_string(),
    };
    if text.chars().count() > 300 {
        format!("{}…", text.chars().take(297).collect::<String>())
    } else {
        text
    }
}

/// (page count in the header, real page count of the file). `None` when the file is not SQLite or the page size does not match. Read-only.
fn header_page_counts(file: &mut std::fs::File) -> std::io::Result<Option<(u64, u64)>> {
    use std::io::Read;
    let file_len = file.metadata()?.len();
    let mut header = [0u8; 100];
    if file.read(&mut header)? < 100 {
        return Ok(None);
    }
    if &header[0..16] != b"SQLite format 3\0" {
        return Ok(None);
    }
    let mut page_size = u16::from_be_bytes([header[16], header[17]]) as u64;
    if page_size == 1 {
        page_size = 65_536;
    }
    if page_size == 0 || file_len % page_size != 0 {
        return Ok(None);
    }
    let actual_pages = file_len / page_size;
    let claimed_pages = u32::from_be_bytes([header[28], header[29], header[30], header[31]]) as u64;
    Ok(Some((claimed_pages, actual_pages)))
}

/// The header claims more pages than the file has (a typical truncation). Read-only check.
fn header_claims_missing_pages(path: &Path) -> std::io::Result<bool> {
    let mut file = std::fs::File::open(path)?;
    Ok(header_page_counts(&mut file)?
        .is_some_and(|(claimed, actual)| claimed > actual && actual > 0))
}

pub(super) fn salvage_truncated_page_count(path: &Path) -> std::io::Result<bool> {
    use std::fs::OpenOptions;
    use std::io::{Seek, SeekFrom, Write};

    let mut file = OpenOptions::new().read(true).write(true).open(path)?;
    let Some((claimed_pages, actual_pages)) = header_page_counts(&mut file)? else {
        return Ok(false);
    };
    if claimed_pages <= actual_pages || actual_pages == 0 {
        return Ok(false);
    }
    file.seek(SeekFrom::Start(28))?;
    file.write_all(&(u32::try_from(actual_pages).unwrap_or(u32::MAX)).to_be_bytes())?;
    file.flush()?;
    Ok(true)
}

/// Common settings for writable connections.
///
/// - Under WAL, `synchronous = NORMAL` is the combination SQLite recommends: fsync only at checkpoints.
///   A power loss can lose at most the last transaction, and the database does not become corrupt. A sync that writes tens of thousands of rows saves one fsync per transaction.
/// - `journal_size_limit`: after a checkpoint, truncate the WAL back to 32 MB. Without it the WAL only grows.
///   A user library had a 76 MB `zepp.db-wal`.
const WRITABLE_PRAGMAS: &str = "PRAGMA foreign_keys = ON;
     PRAGMA busy_timeout = 30000;
     PRAGMA journal_mode = WAL;
     PRAGMA synchronous = NORMAL;
     PRAGMA journal_size_limit = 33554432;";

impl Database {
    #[cfg(test)]
    pub fn new(db_path: PathBuf) -> Result<Self> {
        Self::open_migrated(&db_path)
    }

    /// Open the local library, repairing a truncated SQLite header when that is
    /// enough, or quarantining a still-unreadable file and starting empty.
    ///
    /// A malformed database must never fail process startup: Tauri treats a
    /// setup-hook error as a panic, which looks like a flash-crash from the
    /// desktop shortcut.
    pub fn open_resilient(db_path: PathBuf) -> Result<(Self, Option<String>)> {
        match Self::open_migrated(&db_path) {
            Ok(db) => Ok((db, None)),
            Err(error) if is_corrupt_error(&error) => Self::recover_corrupt(&db_path),
            Err(error) => Err(error),
        }
    }

    /// Corrupt library recovery: "keep the original, fix the header, move the bad file away, rebuild".
    /// The whole sequence holds one cross-process write lock, so no other CLI or desktop process can step in.
    ///
    /// The whole file group is first copied into the quarantine directory and checked. Only then are the files in the original place touched.
    /// If the original cannot be kept, or the bad file cannot be moved, nothing is deleted. This run starts with a temporary empty database (nothing is saved),
    /// and the user can free disk space or close the program that holds the file, then restart. Startup itself never fails.
    fn recover_corrupt(db_path: &Path) -> Result<(Self, Option<String>)> {
        let _guard = Self::acquire_migration_lock(db_path)?;
        // Check again inside the lock: while we waited, another process may have repaired or rebuilt the library.
        match Self::open_migrated_locked(db_path) {
            Ok(db) => return Ok((db, None)),
            Err(error) if is_corrupt_error(&error) => {}
            Err(error) => return Err(error),
        }
        let preserved = match crate::paths::preserve_sqlite_group(db_path) {
            Ok(dir) => dir,
            Err(_) => {
                return Ok((
                    Self::from_connection(Connection::open_in_memory()?)?,
                    Some(
                        "The local library is corrupt, but the original could not be saved first (probably not enough disk space), so it was left untouched. This run uses a temporary empty database and nothing will be saved. Free some space and restart the program to try again."
                            .into(),
                    ),
                ));
            }
        };
        if salvage_truncated_page_count(db_path).unwrap_or(false) {
            match Self::open_migrated_locked(db_path) {
                Ok(db) => {
                    return Ok((
                        db,
                        Some(format!(
                            "The local library file was truncated, and the page header was realigned. The original from before the repair is kept in {}. Some history may need to sync again.",
                            preserved.display()
                        )),
                    ));
                }
                Err(error) if is_corrupt_error(&error) => {}
                Err(error) => return Err(error),
            }
        }
        if crate::paths::remove_live_sqlite_group(db_path).is_err() {
            return Ok((
                Self::from_connection(Connection::open_in_memory()?)?,
                Some(format!(
                    "The local library is corrupt. The original was saved to {}, but the bad file could not be moved from its place (another program may hold it). This run uses a temporary empty database and nothing will be saved. Close the program that holds it and restart to try again.",
                    preserved.display()
                )),
            ));
        }
        let db = Self::open_migrated_locked(db_path)?;
        Ok((
            db,
            Some(format!(
                "The local library is corrupt. It was quarantined to {} and an empty library was created. Sync again.",
                preserved.display()
            )),
        ))
    }

    fn acquire_migration_lock(db_path: &Path) -> Result<Option<write_lock::ExclusiveWriteGuard>> {
        let Some(data_dir) = db_path.parent() else {
            return Ok(None);
        };
        write_lock::acquire_with_timeout(
            data_dir,
            write_lock::WritePurpose::Migration,
            std::time::Duration::from_secs(30),
        )
        .map(Some)
        .map_err(|error| match error {
            error @ write_lock::WriteLockError::Busy { .. } => {
                ZeppBridgeError::Busy(error.to_string())
            }
            error => ZeppBridgeError::ConfigError(error.to_string()),
        })
    }

    /// `open_migrated` without the lock step: the caller already holds the migration write lock.
    ///
    /// If this run really upgraded (a snapshot was taken) and the upgraded library passes the integrity check,
    /// delete the automatic snapshot silently. If the check fails, keep it for recovery.
    fn open_migrated_locked(db_path: &Path) -> Result<Self> {
        backup::check_incomplete_restore(db_path)?;
        let snapshot_taken = Self::backup_before_schema_change(db_path)?;
        let conn = Connection::open(db_path)?;
        let db = Self::from_connection(conn)?;
        if snapshot_taken {
            if let Some(data_dir) = db_path.parent() {
                db.discard_snapshots_if_healthy(data_dir);
            }
        }
        Ok(db)
    }

    /// Delete automatic snapshots only when the current library passes `PRAGMA quick_check`. Any error
    /// means no delete: an extra snapshot does not harm data, a wrong delete does.
    pub(super) fn discard_snapshots_if_healthy(&self, data_dir: &Path) {
        let healthy = self
            .conn
            .query_row("PRAGMA quick_check", [], |row| row.get::<_, String>(0))
            .is_ok_and(|result| result == "ok");
        if healthy {
            let _ = backup::discard_automatic_snapshots(data_dir);
        }
    }

    /// Open and migrate. A failure is a failure: no quarantine and rebuild.
    ///
    /// `open_resilient` quarantines a corrupt library and creates an empty one. Only the desktop app
    /// should do that, because it has a UI to explain it. A non-interactive process such as the CLI must get the error
    /// and exit. It must not silently swap the user's library for an empty one.
    pub fn open_migrated(db_path: &std::path::Path) -> Result<Self> {
        // The pre-migration backup and the DDL must run under the same cross-process lock: two processes that upgrade the same
        // library at the same time is the most dangerous case in this system. Wait for the lock. If it times out, return a recoverable error.
        let guard = Self::acquire_migration_lock(db_path)?;
        let db = Self::open_migrated_locked(db_path);
        drop(guard);
        db
    }

    /// Take a verifiable snapshot of the existing library before any DDL.
    ///
    /// Migrations only go forward. A bad change cannot be undone. If the backup or its check fails, return a recoverable error,
    /// so the user sees "the upgrade did not start". A half-done migration must not leave the library in a state nobody can read.
    /// A new empty library (`user_version = 0`) has nothing to lose, so skip it.
    /// Returns whether a snapshot was really taken (only when an upgrade is needed).
    pub(super) fn backup_before_schema_change(db_path: &std::path::Path) -> Result<bool> {
        let Some(data_dir) = db_path.parent() else {
            return Ok(false);
        };
        if !db_path.exists() {
            return Ok(false);
        }
        let version = {
            let probe = Connection::open(db_path)?;
            probe
                .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
                .unwrap_or(0)
        };
        if version == 0 || version >= CURRENT_SCHEMA_VERSION {
            return Ok(false);
        }
        match backup::create_backup(data_dir, backup::BackupKind::PreMigration, APP_VERSION) {
            Ok(_) => {
                // Rolling cleanup touches only automatic migration backups. It never touches manual backups or backups marked to keep.
                let _ = backup::prune_migration_backups(data_dir);
                Ok(true)
            }
            Err(error) => Err(ZeppBridgeError::DataUnavailable(format!(
                "The database needs an upgrade to a new version, but the automatic backup before the upgrade failed, so the upgrade did not start: {}. Make sure the disk with the data folder has free space, then try again.",
                error.user_message()
            ))),
        }
    }

    /// Open a connection that assumes the schema was already migrated by the
    /// primary connection (`AppState::new`).  Sync workers use this so a
    /// long-running background sync never competes with command paths over
    /// DDL locks (SQLITE_BUSY on ALTER/CREATE INDEX while writing).
    pub fn open_without_migration(db_path: PathBuf) -> Result<Self> {
        backup::check_incomplete_restore(&db_path)?;
        let conn = Connection::open(db_path)?;
        conn.execute_batch(WRITABLE_PRAGMAS)?;
        Ok(Self::from_conn(conn))
    }

    /// The data directory of the library file this connection opened. `None` for an in-memory database.
    ///
    /// A read-only process sometimes needs one short write (MCP drafts a training plan). It uses this to find the write lock of the same directory.
    pub fn data_dir(&self) -> Option<PathBuf> {
        let file = self.conn.path().filter(|path| !path.is_empty())?;
        std::path::Path::new(file)
            .parent()
            .map(std::path::Path::to_path_buf)
    }

    /// Whether this connection is read-only (`query_only`).
    pub fn is_query_only(&self) -> bool {
        self.conn
            .query_row("PRAGMA query_only", [], |row| row.get::<_, i64>(0))
            .map(|value| value != 0)
            .unwrap_or(false)
    }

    /// Replace a read-only connection with a writable one. Returns whether it was replaced.
    ///
    /// At desktop app startup, if another process (CLI reparse, restore) holds the write lock, the command side can only get
    /// a read-only connection. Before, it stayed read-only: for the whole session, settings changes and life events failed with
    /// "readonly database" until a restart. The caller calls this after it gets the write lock. With the lock held,
    /// nobody else writes or migrates. If the library version is wrong (not upgraded yet), do not replace it and return the error.
    pub fn reopen_writable_if_query_only(&mut self, db_path: PathBuf) -> Result<bool> {
        if !self.is_query_only() {
            return Ok(false);
        }
        let fresh = Self::open_without_migration(db_path)?;
        let version: i64 = fresh
            .conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if version != CURRENT_SCHEMA_VERSION {
            return Err(ZeppBridgeError::Busy(format!(
                "The local library is version v{version}, but this program needs v{CURRENT_SCHEMA_VERSION}. Restart the app to finish the upgrade, then try again."
            )));
        }
        *self = fresh;
        Ok(true)
    }

    /// Open a query-only connection.
    ///
    /// Read paths that must never write — the local REST API, the MCP server,
    /// `zeppbridge status` — use this so a bug in an adapter cannot mutate the
    /// user's library, and so they never contend for the write lock.
    /// `query_only` is belt-and-braces on top of the read-only open flag.
    /// Open a library of any version as read-only.
    ///
    /// Backup and restore must read libraries of other versions: a pre-upgrade snapshot is an old version by definition,
    /// and a restore preview can be too. So this layer only gives the mechanism, with no version check.
    ///
    /// Callers that answer user queries must use `open_read_only`.
    ///
    /// When a normal read-only open fails (Windows report: app closed, WAL checkpointed,
    /// but the CLI, MCP and "Send to AI" still always reported "database temporarily unavailable", while Python
    /// opened the same library with `mode=ro`), fall back when it is safe:
    ///
    /// - With no live `-wal` (missing or empty), use `immutable=1`: do not touch `-shm`, take no lock.
    ///   This gets past a leftover `-shm` that is not writable, is held by another account or antivirus, or that an ACL does not let us create.
    ///   Never use it when `-wal` still has frames: committed data would be missing.
    /// - The header claims more pages than the file has (truncated): a writable connection repairs the header first, a read-only one cannot.
    ///   So turn on `writable_schema`, and SQLite reads the real file length. Nothing is written.
    ///
    /// If nothing works, return the first error, with the raw SQLite error code and the masked path,
    /// so the next report needs no guessing.
    pub fn open_read_only_any_version(db_path: PathBuf) -> Result<Self> {
        let first = match Self::try_open_read_only(&db_path, None) {
            Ok(db) => return Ok(db),
            Err(error) => error,
        };
        if let Some(fallback) = read_fallback_for(&db_path, &first) {
            match Self::try_open_read_only(&db_path, Some(fallback)) {
                Ok(mut db) => {
                    tracing::warn!(
                        fallback = fallback.as_str(),
                        error = %sqlite_error_detail(&first),
                        "read-only open fell back"
                    );
                    db.read_fallback = Some(fallback);
                    return Ok(db);
                }
                Err(error) => {
                    tracing::warn!(
                        fallback = fallback.as_str(),
                        error = %sqlite_error_detail(&error),
                        "read-only fallback failed too"
                    );
                }
            }
        }
        Err(ZeppBridgeError::DatabaseError(annotate_with_path(
            first, &db_path,
        )))
    }

    /// One read-only open, then read the schema at once. SQLite opens files lazily, so `-wal` /
    /// `-shm` problems appear only at the first read. They must appear here, while a fallback is still possible.
    fn try_open_read_only(
        db_path: &Path,
        fallback: Option<ReadFallback>,
    ) -> std::result::Result<Self, rusqlite::Error> {
        let flags =
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI;
        let conn = match fallback {
            Some(ReadFallback::Immutable) => {
                let uri = immutable_uri(db_path)
                    .ok_or_else(|| rusqlite::Error::InvalidPath(db_path.to_path_buf()))?;
                Connection::open_with_flags(uri, flags)?
            }
            _ => Connection::open_with_flags(db_path, flags)?,
        };
        if fallback == Some(ReadFallback::TruncatedHeader) {
            // Only a connection flag. It reads nothing, and a read-only connection cannot write anything with it.
            conn.execute_batch("PRAGMA writable_schema = ON;")?;
        }
        conn.execute_batch(
            "PRAGMA busy_timeout = 30000;
             PRAGMA query_only = ON;",
        )?;
        conn.query_row("SELECT count(*) FROM sqlite_master", [], |row| {
            row.get::<_, i64>(0)
        })?;
        Ok(Self::from_conn(conn))
    }

    /// Whether this read-only connection used a fallback (see [`Self::open_read_only_any_version`]).
    pub fn read_fallback(&self) -> Option<ReadFallback> {
        self.read_fallback
    }

    /// Open read-only, and first check that the schema version matches.
    ///
    /// A read-only connection cannot migrate, so a version mismatch must become a clear instruction here.
    /// Otherwise the CLI or MCP runs until a query hits "no such table",
    /// and the user sees "database temporarily unavailable", with no cause and no next step.
    ///
    /// This version check is **product policy**, not the file open mechanism. If it were in the mechanism,
    /// it would also block the automatic pre-upgrade backup, and then nobody could upgrade.
    pub fn open_read_only(db_path: PathBuf) -> Result<Self> {
        let db = Self::open_read_only_any_version(db_path)?;
        let version: i64 = db
            .conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))?;
        match version.cmp(&CURRENT_SCHEMA_VERSION) {
            std::cmp::Ordering::Equal => Ok(db),
            // Own error code: people who hit this are almost always headless, and the command line has no
            // i18n layer. Only an error code can give English text. See `HeadlessProblem`.
            std::cmp::Ordering::Less => Err(ZeppBridgeError::Headless(
                crate::models::error::HeadlessProblem::SchemaUpgradeRequired {
                    found: version,
                    required: CURRENT_SCHEMA_VERSION,
                },
            )),
            // An old CLI or MCP reads a newer library. The message says which program it is and how to point the AI tool at
            // the new version (user report: "database v47 / program v41", and the user did not know what to do).
            std::cmp::Ordering::Greater => Err(ZeppBridgeError::Headless(
                crate::models::error::HeadlessProblem::ProgramOutdated {
                    found: version,
                    supported: CURRENT_SCHEMA_VERSION,
                    program: std::env::current_exe()
                        .map(|path| crate::paths::mask_user_path(&path))
                        .unwrap_or_default(),
                    version: env!("CARGO_PKG_VERSION").to_string(),
                },
            )),
        }
    }

    #[cfg(test)]
    pub fn in_memory() -> Result<Self> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    pub(super) fn from_connection(conn: Connection) -> Result<Self> {
        Self::reject_newer_schema(&conn)?;
        // These pragmas are set for every connection, including test databases.
        conn.execute_batch(WRITABLE_PRAGMAS)?;
        let db = Self::from_conn(conn);
        db.migrate()?;
        Ok(db)
    }
}
