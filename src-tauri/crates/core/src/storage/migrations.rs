//! SQLite schema and migrations.
//!
//! This is a separate file because it reads differently from the rest: each part here is **appended** history.
//! A released migration step can never change again. User databases already built their tables with that DDL,
//! and a later change only makes old and new databases differ. To change the structure, add a new version at the end.
//!
//! The version number is `PRAGMA user_version`. For the current value, see `super::CURRENT_SCHEMA_VERSION`.
//! `Database::open_migrated` calls the migration after it takes the cross-process write lock and makes a pre-upgrade
//! backup, so this code can assume it is the only writer.

use super::*;

impl Database {
    pub(super) fn reject_newer_schema(conn: &Connection) -> Result<()> {
        let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if version > CURRENT_SCHEMA_VERSION {
            return Err(ZeppBridgeError::DataUnavailable(format!(
                "Database schema v{version} is newer than supported v{CURRENT_SCHEMA_VERSION}. Upgrade ZeppBridge before opening this database."
            )));
        }
        Ok(())
    }

    /// Transaction boundary of the migration.
    ///
    /// Each step in here is idempotent on its own, but **together they are not atomic**, and the version number
    /// goes back and then forward during the run: the `PRAGMA user_version = 5` line is not inside any
    /// `if version < N` guard, so a v19 database rewrites every version from 5 up to 19 on each
    /// start. A power cut or forced kill in the middle left "schema already new,
    /// version says 8" on disk. Read-only connections (CLI / MCP) saw the version mismatch and refused to start,
    /// and the user had no idea what happened.
    ///
    /// Inside one transaction, this middle state does not exist for other connections or for the next start: either
    /// the whole migration lands, or not one byte is written. DDL is transactional in SQLite,
    /// and `user_version` lives in the header page, so it rolls back too.
    ///
    /// Pragmas that cannot run inside a transaction, such as `journal_mode = WAL`, are set by `from_connection`
    /// **before** this call. Do not add them to `migrate_steps`.
    pub(super) fn migrate(&self) -> Result<()> {
        self.conn.execute_batch("BEGIN IMMEDIATE;")?;
        match self.migrate_steps() {
            Ok(()) => {
                if self.conn.is_autocommit() {
                    // SQLite rolled the transaction back itself (I/O error, full disk or corruption
                    // can cause it). No change landed, so this must not count as a successful upgrade:
                    // the next step would then use the old schema as the new one.
                    return Err(ZeppBridgeError::DataUnavailable(
                        "The database upgrade was interrupted and no change was written. Check disk space and file permissions, then try again."
                            .into(),
                    ));
                }
                self.conn.execute_batch("COMMIT;")?;
                Ok(())
            }
            Err(error) => {
                if !self.conn.is_autocommit() {
                    let _ = self.conn.execute_batch("ROLLBACK;");
                }
                Err(error)
            }
        }
    }

    fn migrate_steps(&self) -> Result<()> {
        Self::reject_newer_schema(&self.conn)?;
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_migrations (
                version INTEGER PRIMARY KEY,
                applied_at TEXT NOT NULL
            );",
        )?;
        let version: i64 = self
            .conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))?;

        // Repair a missing or historical four-column key before the old v4
        // IF NOT EXISTS statement can recreate it and reject two devices.
        if version >= 7 {
            let columns: i64 = self.conn.query_row(
                "SELECT COUNT(*) FROM pragma_index_info('uq_daily_metric_key')",
                [],
                |row| row.get(0),
            )?;
            if columns == 0 || columns == 4 {
                self.conn.execute_batch(
                    "DROP INDEX IF EXISTS uq_daily_metric_key;
                     CREATE UNIQUE INDEX uq_daily_metric_key
                         ON daily_metrics(date, metric, unit, source_scope, COALESCE(device_id, ''));",
                )?;
            }
        }

        if version < 1 {
            self.conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS source_accounts (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    source_type TEXT NOT NULL,
                    region_host TEXT NOT NULL,
                    external_user_hash TEXT NOT NULL,
                    auth_state TEXT NOT NULL,
                    capabilities TEXT,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL
                );
                CREATE TABLE IF NOT EXISTS raw_records (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    stream TEXT NOT NULL,
                    source_key TEXT NOT NULL,
                    source_scope TEXT NOT NULL,
                    device_id TEXT,
                    start_utc TEXT NOT NULL,
                    end_utc TEXT,
                    payload TEXT NOT NULL,
                    payload_hash TEXT NOT NULL,
                    fetched_at TEXT NOT NULL,
                    UNIQUE(stream, source_key)
                );
                CREATE TABLE IF NOT EXISTS metric_samples (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    metric TEXT NOT NULL,
                    timestamp TEXT NOT NULL,
                    value REAL NOT NULL,
                    unit TEXT NOT NULL,
                    source_scope TEXT NOT NULL,
                    device_id TEXT,
                    raw_record_id INTEGER,
                    FOREIGN KEY(raw_record_id) REFERENCES raw_records(id)
                );
                CREATE TABLE IF NOT EXISTS daily_metrics (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    date TEXT NOT NULL,
                    metric TEXT NOT NULL,
                    value REAL NOT NULL,
                    unit TEXT NOT NULL,
                    source_scope TEXT NOT NULL,
                    device_id TEXT,
                    raw_record_id INTEGER,
                    FOREIGN KEY(raw_record_id) REFERENCES raw_records(id)
                );
                CREATE TABLE IF NOT EXISTS sleep_sessions (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    sleep_id TEXT NOT NULL UNIQUE,
                    start_time TEXT NOT NULL,
                    end_time TEXT NOT NULL,
                    score INTEGER,
                    duration_minutes INTEGER NOT NULL,
                    deep_minutes INTEGER NOT NULL,
                    light_minutes INTEGER NOT NULL,
                    rem_minutes INTEGER NOT NULL,
                    awake_minutes INTEGER NOT NULL,
                    source_scope TEXT NOT NULL,
                    device_id TEXT,
                    raw_record_id INTEGER,
                    FOREIGN KEY(raw_record_id) REFERENCES raw_records(id)
                );
                CREATE TABLE IF NOT EXISTS workouts (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    workout_id TEXT NOT NULL UNIQUE,
                    workout_type TEXT NOT NULL,
                    start_time TEXT NOT NULL,
                    end_time TEXT NOT NULL,
                    distance_meters REAL,
                    calories INTEGER,
                    avg_hr INTEGER,
                    max_hr INTEGER,
                    training_load REAL,
                    vo2max REAL,
                    source_scope TEXT NOT NULL,
                    device_id TEXT,
                    raw_record_id INTEGER,
                    FOREIGN KEY(raw_record_id) REFERENCES raw_records(id)
                );
                CREATE TABLE IF NOT EXISTS sleep_stages (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    sleep_id TEXT NOT NULL,
                    stage TEXT NOT NULL,
                    start_time TEXT NOT NULL,
                    end_time TEXT NOT NULL,
                    FOREIGN KEY(sleep_id) REFERENCES sleep_sessions(sleep_id) ON DELETE CASCADE
                );
                CREATE TABLE IF NOT EXISTS workout_samples (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    workout_id TEXT NOT NULL,
                    timestamp TEXT NOT NULL,
                    heart_rate INTEGER,
                    pace REAL,
                    speed REAL,
                    cadence REAL,
                    altitude REAL,
                    FOREIGN KEY(workout_id) REFERENCES workouts(workout_id) ON DELETE CASCADE
                );
                CREATE TABLE IF NOT EXISTS route_points (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    workout_id TEXT NOT NULL,
                    timestamp TEXT NOT NULL,
                    latitude REAL NOT NULL,
                    longitude REAL NOT NULL,
                    altitude REAL,
                    FOREIGN KEY(workout_id) REFERENCES workouts(workout_id) ON DELETE CASCADE
                );
                CREATE TABLE IF NOT EXISTS sync_state (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    stream TEXT NOT NULL UNIQUE,
                    last_sync TEXT,
                    cursor TEXT,
                    status TEXT NOT NULL,
                    error TEXT,
                    needs_reauth INTEGER NOT NULL DEFAULT 0,
                    records_written INTEGER NOT NULL DEFAULT 0,
                    capability TEXT NOT NULL DEFAULT 'verified',
                    message TEXT,
                    updated_at TEXT NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_metric_samples_metric_timestamp
                    ON metric_samples(metric, timestamp);
                CREATE INDEX IF NOT EXISTS idx_daily_metrics_date_metric
                    ON daily_metrics(date, metric);
                CREATE INDEX IF NOT EXISTS idx_raw_records_fetched_at
                    ON raw_records(fetched_at);
                PRAGMA user_version = 1;",
            )?;
            self.conn.execute(
                "INSERT OR IGNORE INTO schema_migrations(version, applied_at) VALUES(1, ?1)",
                [Utc::now().to_rfc3339()],
            )?;
        } else {
            // Databases created by the initial MVP may have the core tables but
            // lack the richer sync columns.  Add only missing columns so the
            // migration remains idempotent.
            self.ensure_table_columns(
                "sync_state",
                &[
                    ("cursor", "TEXT"),
                    ("needs_reauth", "INTEGER NOT NULL DEFAULT 0"),
                    ("records_written", "INTEGER NOT NULL DEFAULT 0"),
                    ("capability", "TEXT NOT NULL DEFAULT 'verified'"),
                    ("message", "TEXT"),
                ],
            )?;
            self.ensure_table_columns("raw_records", &[("payload_hash", "TEXT")])?;
        }

        // C9: ancient libraries can carry duplicate metric_samples that make
        // the historical unique-index CREATE below fail. Dedupe first; do not
        // rewrite that published DDL.
        if version < 28 {
            self.conn.execute_batch(
                "DELETE FROM metric_samples
                 WHERE id NOT IN (
                     SELECT MIN(id) FROM metric_samples
                     GROUP BY metric, timestamp, unit, source_scope, COALESCE(device_id, '')
                 );",
            )?;
        }

        // Expression indexes are needed because SQLite treats NULLs as distinct
        // in ordinary UNIQUE constraints.  COALESCE makes a missing device id a
        // deterministic part of the canonical key.
        //
        // The daily_metrics unique-key rebuild is destructive (DELETE + DROP +
        // CREATE INDEX). Running it on every launch of a 1GB library is how
        // a force-killed startup left a truncated file and flash-crashed the
        // next double-click. Only do that work when upgrading older schemas.
        if version < 4 {
            self.conn.execute_batch(
                "CREATE UNIQUE INDEX IF NOT EXISTS uq_metric_sample_key
                     ON metric_samples(metric, timestamp, unit, source_scope, COALESCE(device_id, ''));
                  DELETE FROM daily_metrics WHERE id NOT IN (
                      SELECT MIN(id) FROM daily_metrics
                      GROUP BY date, metric, unit, source_scope);
                  DROP INDEX IF EXISTS uq_daily_metric_key;
                  CREATE UNIQUE INDEX uq_daily_metric_key
                     ON daily_metrics(date, metric, unit, source_scope);
                 CREATE TABLE IF NOT EXISTS app_meta (
                     key TEXT PRIMARY KEY,
                     value TEXT NOT NULL,
                     updated_at TEXT NOT NULL
                 );
                 PRAGMA user_version = 4;",
            )?;
        } else {
            self.conn.execute_batch(
                "CREATE UNIQUE INDEX IF NOT EXISTS uq_metric_sample_key
                     ON metric_samples(metric, timestamp, unit, source_scope, COALESCE(device_id, ''));
                 CREATE UNIQUE INDEX IF NOT EXISTS uq_daily_metric_key
                     ON daily_metrics(date, metric, unit, source_scope);
                 CREATE TABLE IF NOT EXISTS app_meta (
                     key TEXT PRIMARY KEY,
                     value TEXT NOT NULL,
                     updated_at TEXT NOT NULL
                 );",
            )?;
        }
        self.conn.execute(
            "INSERT OR IGNORE INTO schema_migrations(version, applied_at) VALUES(2, ?1)",
            [Utc::now().to_rfc3339()],
        )?;
        self.conn.execute(
            "INSERT OR IGNORE INTO schema_migrations(version, applied_at) VALUES(3, ?1)",
            [Utc::now().to_rfc3339()],
        )?;
        self.ensure_table_columns(
            "sleep_sessions",
            &[("rem_available", "INTEGER NOT NULL DEFAULT 1")],
        )?;
        self.conn.execute(
            "INSERT OR IGNORE INTO schema_migrations(version, applied_at) VALUES(4, ?1)",
            [Utc::now().to_rfc3339()],
        )?;
        self.ensure_table_columns("sleep_sessions", &[("synced_at", "TEXT")])?;
        self.ensure_table_columns(
            "workouts",
            &[
                ("synced_at", "TEXT"),
                ("gps_available", "INTEGER NOT NULL DEFAULT 0"),
                ("sample_count", "INTEGER NOT NULL DEFAULT 0"),
            ],
        )?;
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS device_identities (
                alias TEXT PRIMARY KEY,
                name TEXT,
                firmware TEXT,
                serial TEXT,
                device_id TEXT,
                timezone TEXT,
                updated_at TEXT NOT NULL
            );",
        )?;
        if let Err(error) = self.conn.execute(
            "UPDATE sleep_sessions
             SET synced_at = (
                 SELECT fetched_at FROM raw_records
                 WHERE raw_records.id = sleep_sessions.raw_record_id
             )
             WHERE synced_at IS NULL",
            [],
        ) {
            if !is_corrupt_sqlite(&error) {
                return Err(error.into());
            }
        }
        if let Err(error) = self.conn.execute(
            "UPDATE workouts
             SET synced_at = (
                 SELECT fetched_at FROM raw_records
                 WHERE raw_records.id = workouts.raw_record_id
             )
             WHERE synced_at IS NULL",
            [],
        ) {
            if !is_corrupt_sqlite(&error) {
                return Err(error.into());
            }
        }
        self.conn.execute(
            "INSERT OR IGNORE INTO schema_migrations(version, applied_at) VALUES(5, ?1)",
            [Utc::now().to_rfc3339()],
        )?;
        self.conn.execute_batch("PRAGMA user_version = 5;")?;
        self.ensure_table_columns(
            "workouts",
            &[("zepp_source", "TEXT"), ("zepp_type", "INTEGER")],
        )?;
        self.ensure_table_columns("workout_samples", &[("stride", "REAL")])?;
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS workout_pauses (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                workout_id TEXT NOT NULL,
                start_time TEXT NOT NULL,
                end_time TEXT NOT NULL,
                kind TEXT NOT NULL,
                FOREIGN KEY(workout_id) REFERENCES workouts(workout_id) ON DELETE CASCADE
            );
            CREATE INDEX IF NOT EXISTS idx_workout_samples_workout
                ON workout_samples(workout_id, timestamp);
            CREATE INDEX IF NOT EXISTS idx_route_points_workout
                ON route_points(workout_id, timestamp);
            PRAGMA user_version = 6;",
        )?;
        self.conn.execute(
            "INSERT OR IGNORE INTO schema_migrations(version, applied_at) VALUES(6, ?1)",
            [Utc::now().to_rfc3339()],
        )?;
        // daily_metrics' canonical key predates device attribution, so two
        // devices reporting the same metric on the same day collide and one
        // silently overwrites the other. metric_samples already keys on
        // COALESCE(device_id, '') (version 4); bring daily_metrics in line.
        // Widening a unique key can never surface a duplicate, so unlike the
        // version-4 rebuild this needs no DELETE. Gate it on the version so a
        // large library does not rebuild the index on every launch.
        if version < 7 {
            self.conn.execute_batch(
                "DROP INDEX IF EXISTS uq_daily_metric_key;
                 CREATE UNIQUE INDEX uq_daily_metric_key
                     ON daily_metrics(date, metric, unit, source_scope, COALESCE(device_id, ''));
                 PRAGMA user_version = 7;",
            )?;
        }
        self.conn.execute(
            "INSERT OR IGNORE INTO schema_migrations(version, applied_at) VALUES(7, ?1)",
            [Utc::now().to_rfc3339()],
        )?;
        // Per-kilometre splits are derived from the raw detail payload, so the
        // table starts empty and fills in on the next normalizer replay.
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS workout_splits (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                workout_id TEXT NOT NULL,
                split_index INTEGER NOT NULL,
                start_time TEXT NOT NULL,
                end_time TEXT NOT NULL,
                distance_m REAL NOT NULL,
                duration_seconds INTEGER NOT NULL,
                pace_min_per_km REAL,
                avg_hr INTEGER,
                max_hr INTEGER,
                elevation_gain_m REAL,
                elevation_loss_m REAL,
                partial INTEGER NOT NULL,
                FOREIGN KEY(workout_id) REFERENCES workouts(workout_id) ON DELETE CASCADE
            );
            CREATE INDEX IF NOT EXISTS idx_workout_splits_workout
                ON workout_splits(workout_id, split_index);
            PRAGMA user_version = 8;",
        )?;
        self.conn.execute(
            "INSERT OR IGNORE INTO schema_migrations(version, applied_at) VALUES(8, ?1)",
            [Utc::now().to_rfc3339()],
        )?;
        // `wc` has been in every band payload all along; nights already stored
        // backfill on the next normalizer replay.
        self.ensure_table_columns("sleep_sessions", &[("wake_count", "INTEGER")])?;
        self.conn.execute_batch("PRAGMA user_version = 9;")?;
        self.conn.execute(
            "INSERT OR IGNORE INTO schema_migrations(version, applied_at) VALUES(9, ?1)",
            [Utc::now().to_rfc3339()],
        )?;
        // Running power and form come from the same detail payload the samples
        // already carry, so the columns start empty and fill in on the replay
        // that the revision bump triggers.
        self.ensure_table_columns(
            "workout_samples",
            &[
                ("power_watts", "REAL"),
                ("ground_contact_ms", "REAL"),
                ("vertical_oscillation_mm", "REAL"),
                ("vertical_ratio_pct", "REAL"),
                ("equivalent_pace_s", "REAL"),
            ],
        )?;
        self.conn.execute_batch("PRAGMA user_version = 10;")?;
        self.conn.execute(
            "INSERT OR IGNORE INTO schema_migrations(version, applied_at) VALUES(10, ?1)",
            [Utc::now().to_rfc3339()],
        )?;
        // Keep Zepp's raw numeric fact, our interpretation, and a user's local
        // correction as separate layers. Existing rows are classified from the
        // evidence already stored; the revision bump then replays retained raw
        // records using the current normalizer without changing cloud sync time.
        self.ensure_table_columns(
            "workouts",
            &[
                ("workout_type_source", "TEXT NOT NULL DEFAULT 'missing'"),
                ("workout_type_override", "TEXT"),
                ("workout_type_conflict", "TEXT"),
            ],
        )?;
        if version < 11 {
            self.conn.execute_batch(
                "UPDATE workouts
                    SET workout_type_source = CASE
                        WHEN zepp_type IS NOT NULL AND workout_type LIKE 'unknown:%' THEN 'unknown_code'
                        WHEN zepp_type IS NOT NULL THEN 'numeric_mapped'
                        WHEN workout_type <> 'unknown' THEN 'string_field'
                        ELSE 'missing'
                    END;",
            )?;
        }
        self.conn.execute_batch("PRAGMA user_version = 11;")?;
        self.conn.execute(
            "INSERT OR IGNORE INTO schema_migrations(version, applied_at) VALUES(11, ?1)",
            [Utc::now().to_rfc3339()],
        )?;
        // Per-stream provenance: fetch / parse / write are three different
        // things that can fail independently, and collapsing them into one
        // status is how "the data is stale" becomes unanswerable. The table
        // starts empty; `data_health` falls back to raw_records.fetched_at so
        // an upgraded library does not claim it has never fetched anything.
        //
        // The raw_record_id indexes make the "retained but never normalized"
        // count a lookup instead of four full scans of the canonical tables.
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS stream_provenance (
                stream TEXT PRIMARY KEY,
                last_fetch_ok_at TEXT,
                last_fetch_error_at TEXT,
                last_fetch_error_kind TEXT,
                last_fetch_error_message TEXT,
                last_parse_ok_at TEXT,
                last_parse_error_at TEXT,
                last_parse_error_kind TEXT,
                last_parse_error_message TEXT,
                last_write_ok_at TEXT,
                last_write_error_at TEXT,
                last_write_error_kind TEXT,
                last_write_error_message TEXT,
                last_written_records INTEGER NOT NULL DEFAULT 0,
                updated_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_metric_samples_raw
                ON metric_samples(raw_record_id);
            CREATE INDEX IF NOT EXISTS idx_daily_metrics_raw
                ON daily_metrics(raw_record_id);
            CREATE INDEX IF NOT EXISTS idx_sleep_sessions_raw
                ON sleep_sessions(raw_record_id);
            CREATE INDEX IF NOT EXISTS idx_workouts_raw
                ON workouts(raw_record_id);
            CREATE INDEX IF NOT EXISTS idx_raw_records_stream
                ON raw_records(stream);
            PRAGMA user_version = 12;",
        )?;
        self.conn.execute(
            "INSERT OR IGNORE INTO schema_migrations(version, applied_at) VALUES(12, ?1)",
            [Utc::now().to_rfc3339()],
        )?;
        // The user-correction layer. Kept in its own tables so a normalizer
        // replay can rewrite ZeppBridge's interpretation without touching what
        // the user told us, and so a correction is always displayable as
        // "you filled this in" rather than as a recognition result.
        //
        // Both tables answer real reports: a custom Zepp training template
        // arrives as a numeric code the bundled catalog does not know, and some
        // accounts' device responses carry no product-name field at all, so no
        // amount of matching can name the watch.
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS workout_code_labels (
                zepp_type INTEGER PRIMARY KEY,
                label TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS device_model_overrides (
                device_key TEXT PRIMARY KEY,
                catalog_id TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            PRAGMA user_version = 13;",
        )?;
        self.conn.execute(
            "INSERT OR IGNORE INTO schema_migrations(version, applied_at) VALUES(13, ?1)",
            [Utc::now().to_rfc3339()],
        )?;
        // History coverage ledger. "Requested", "received", "written" and "cloud returned nothing" are
        // four different states. Recording only "synced up to some month" makes them all look the same,
        // so we could neither resume nor honestly answer "is my history complete".
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS coverage_ledger (
                stream TEXT NOT NULL,
                chunk_start TEXT NOT NULL,
                chunk_end TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'pending',
                requested_at TEXT,
                fetched_at TEXT,
                persisted_at TEXT,
                records INTEGER NOT NULL DEFAULT 0,
                error TEXT,
                updated_at TEXT NOT NULL,
                PRIMARY KEY(stream, chunk_start)
            );
            CREATE INDEX IF NOT EXISTS idx_coverage_ledger_status
                ON coverage_ledger(status, chunk_start DESC);
            PRAGMA user_version = 14;",
        )?;
        self.conn.execute(
            "INSERT OR IGNORE INTO schema_migrations(version, applied_at) VALUES(14, ?1)",
            [Utc::now().to_rfc3339()],
        )?;
        // Compressed raw payloads.
        //
        // Raw payloads use the most space in the database: on this machine, about 2000 raw_records
        // used just over 1 GB. They are JSON text, and deflate leaves only 18%.
        //
        // Add a new column instead of changing `payload`: released DDL cannot change, and old rows must stay
        // readable as they are. Reads use `payload_zip` first and fall back to `payload`.
        self.ensure_table_columns("raw_records", &[("payload_zip", "BLOB")])?;
        self.conn.execute_batch("PRAGMA user_version = 15;")?;
        self.conn.execute(
            "INSERT OR IGNORE INTO schema_migrations(version, applied_at) VALUES(15, ?1)",
            [Utc::now().to_rfc3339()],
        )?;
        // Backfill attempt count.
        //
        // Without this column, `failed` and `pending` are the same in the to-do query: a chunk that fails
        // goes straight back to the front, the next round picks it again, and later chunks never get a turn. The attempt
        // count separates "already tried this round" from "can try again later". It also lets
        // deterministic failures (for example, a payload that arrives but has no parseable row) stop after some
        // automatic retries and wait for an explicit user retry, instead of spinning forever.
        self.ensure_table_columns(
            "coverage_ledger",
            &[
                ("attempts", "INTEGER NOT NULL DEFAULT 0"),
                ("last_attempt_at", "TEXT"),
                // Stable code for the failure reason. Without it, the UI could only show the backend's Chinese text,
                // so English users saw a line of Chinese in the backfill ledger.
                ("error_code", "TEXT"),
            ],
        )?;
        self.conn.execute_batch("PRAGMA user_version = 16;")?;
        self.conn.execute(
            "INSERT OR IGNORE INTO schema_migrations(version, applied_at) VALUES(16, ?1)",
            [Utc::now().to_rfc3339()],
        )?;
        // Raw mode value of sleep stages.
        //
        // Unknown modes were once mapped to `awake`. Now they map to `unknown`, but knowing only
        // "one segment was unknown" does not help. Keep the number the cloud gave, so there is something to
        // check next time. Old rows are `NULL`: they were written before this column existed, and inventing a value
        // is the same mistake as inventing `awake` was.
        self.ensure_table_columns("sleep_stages", &[("raw_mode", "INTEGER")])?;
        self.conn.execute_batch("PRAGMA user_version = 17;")?;
        self.conn.execute(
            "INSERT OR IGNORE INTO schema_migrations(version, applied_at) VALUES(17, ?1)",
            [Utc::now().to_rfc3339()],
        )?;
        // Remove firmware version numbers that were stored as devices.
        //
        // A user reported three "unknown data sources" in the sidebar that did nothing and could not be deleted. Their labels were
        // strings like `0.91.20.5` and `0.91.17.5`. Those are not devices but firmware versions:
        // some Zepp payloads put them in the `deviceId` / `sn` position, and the extraction logic checked only
        // field names, not values. The write-side gate is now in `looks_like_firmware_version`. This step
        // deletes the rows already stored. Otherwise they stay in the UI forever, and the user has
        // no way to delete them.
        //
        // The test must be the same as on the write side: call `looks_like_firmware_version` here
        // instead of writing it again in SQL. SQLite has no regex, and a GLOB approximation is looser than that
        // function (the `*` in `[0-9]*.[0-9]*.[0-9]*` crosses dots, so `1.a.b.2.3` also
        // matches). When the two tests differ, rows appear that "can be written but the migration deletes them".
        // So read the rows, filter in Rust, then delete by exact alias.
        //
        // Delete rows in device_identities only. Do not touch data rows with these aliases: those samples
        // are real but attached to a device name that does not exist. Deleting data would also delete real
        // records. The next sync assigns them to the correct device again.
        let phantom_aliases: Vec<String> = {
            let mut stmt = self.conn.prepare("SELECT alias FROM device_identities")?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
            rows.filter_map(std::result::Result::ok)
                .filter(|alias| super::looks_like_firmware_version(alias))
                .collect()
        };
        for alias in &phantom_aliases {
            self.conn
                .execute("DELETE FROM device_identities WHERE alias = ?1", [alias])?;
        }
        self.conn.execute_batch("PRAGMA user_version = 18;")?;
        self.conn.execute(
            "INSERT OR IGNORE INTO schema_migrations(version, applied_at) VALUES(18, ?1)",
            [Utc::now().to_rfc3339()],
        )?;
        // Workout summary fields the cloud always sent but we never read.
        //
        // An audit of raw payloads: the workouts stream has 198 fields and we read only 26.
        // After removing reserved slots that are always -1/0/"", 66 fields with real data were still dropped.
        // The fields below are the part that users notice directly.
        //
        // Why store the cloud elevation gain: the parser can compute its own from the altitude series with a 1 m noise floor,
        // but the two do not match (one measured walk: cloud 59 m, ours 37 m), and the user sees the cloud value in the Zepp App.
        // The cloud value wins. Ours is the fallback.
        self.ensure_table_columns(
            "workouts",
            &[
                ("min_hr", "INTEGER"),
                ("total_steps", "INTEGER"),
                ("moving_seconds", "INTEGER"),
                ("elevation_gain_m", "REAL"),
                ("elevation_loss_m", "REAL"),
                ("max_altitude_m", "REAL"),
                ("min_altitude_m", "REAL"),
                ("training_effect", "REAL"),
                ("anaerobic_training_effect", "REAL"),
                ("rpe", "INTEGER"),
                ("avg_cadence_spm", "REAL"),
                ("max_cadence_spm", "REAL"),
                ("avg_stride_cm", "REAL"),
            ],
        )?;
        // Heart rate zone distribution.
        //
        // A separate table instead of a JSON column in workouts: this is an ordered list of
        // (upper limit, seconds) that we aggregate and sort by zone. In JSON it would need another decode outside SQL.
        //
        // Store the limits too. Zone limits come from the user's watch settings and change with them, so
        // "how long in Z2" only makes sense together with the limits at that time.
        // v26 removes the redundant index; do not rebuild it on later launches.
        if version < 26 {
            self.conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS workout_hr_zones (
                workout_id TEXT NOT NULL,
                zone_index INTEGER NOT NULL,
                upper_bound_bpm INTEGER NOT NULL,
                seconds INTEGER NOT NULL,
                PRIMARY KEY (workout_id, zone_index)
            );
            CREATE INDEX IF NOT EXISTS idx_workout_hr_zones_workout
                ON workout_hr_zones(workout_id);",
            )?;
        }
        self.conn.execute_batch("PRAGMA user_version = 19;")?;
        self.conn.execute(
            "INSERT OR IGNORE INTO schema_migrations(version, applied_at) VALUES(19, ?1)",
            [Utc::now().to_rfc3339()],
        )?;

        // v20: store the cloud business error code in its own column.
        //
        // `classify_business_code` turns HTTP 200 + a non-success code into
        // `CloudRejected`, but the code only went into a human-readable Chinese sentence. What we
        // need now is exactly that number: all 1075 local stored payloads are
        // `code = 1`, with no failure code to observe, so we cannot just map some number
        // to "login required". It must come back from users who hit the problem,
        // and the diagnostics report sends only allow-listed fields. So it must be a field, not
        // a substring of a sentence. Matches `0007_cloud_rejection.sql` in the feedback database.
        self.ensure_table_columns("stream_provenance", &[("last_error_code", "INTEGER")])?;
        self.conn.execute_batch("PRAGMA user_version = 20;")?;
        self.conn.execute(
            "INSERT OR IGNORE INTO schema_migrations(version, applied_at) VALUES(20, ?1)",
            [Utc::now().to_rfc3339()],
        )?;
        // v21: laps recorded by the watch.
        //
        // These are **different** from `workout_splits`, so they get a separate table instead of a
        // type column: splits are cut locally per kilometre, and laps are what the watch recorded during the workout
        // (lap button, automatic distance laps, or intervals of a training session).
        // One 5820 m run can have both 5 kilometre splits and 14 laps of 415 m.
        // In one table, the "pace per km" chart would suddenly show something else.
        //
        // The table starts empty and the next normalization replay fills it. The raw payloads always had the data.
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS workout_laps (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                workout_id TEXT NOT NULL,
                lap_index INTEGER NOT NULL,
                start_time TEXT NOT NULL,
                end_time TEXT NOT NULL,
                distance_m REAL NOT NULL,
                duration_seconds INTEGER NOT NULL,
                avg_hr INTEGER,
                max_hr INTEGER,
                FOREIGN KEY(workout_id) REFERENCES workouts(workout_id) ON DELETE CASCADE
            );
            CREATE INDEX IF NOT EXISTS idx_workout_laps_workout
                ON workout_laps(workout_id, lap_index);
            PRAGMA user_version = 21;",
        )?;
        self.conn.execute(
            "INSERT OR IGNORE INTO schema_migrations(version, applied_at) VALUES(21, ?1)",
            [Utc::now().to_rfc3339()],
        )?;
        // Cloud code 7 is trail running (issue #24), not the device-protocol
        // open-water code. Repair stored interpretations even when retention
        // has removed their raw payloads. Overrides, metrics and provenance
        // stay intact; explicitly named swimming records are not affected.
        if version < 22 {
            self.conn.execute_batch(
                "UPDATE workouts SET workout_type = 'trail_running'
                 WHERE zepp_type = 7
                   AND workout_type = 'open_water_swimming'
                   AND workout_type_source = 'numeric_mapped';",
            )?;
        }
        self.conn.execute_batch("PRAGMA user_version = 22;")?;
        self.conn.execute(
            "INSERT OR IGNORE INTO schema_migrations(version, applied_at) VALUES(22, ?1)",
            [Utc::now().to_rfc3339()],
        )?;
        // Earlier migrations are intentionally idempotent and still stamp
        // their historical versions on every launch, so the current schema
        // marker is restored only after all of them have run.
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS life_events (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            title TEXT NOT NULL, category TEXT NOT NULL,
            start_date TEXT NOT NULL, end_date TEXT,
            notes TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL,
            CHECK(end_date IS NULL OR end_date >= start_date)
        );
        CREATE INDEX IF NOT EXISTS idx_life_events_dates ON life_events(start_date, end_date);
        PRAGMA user_version = 23;",
        )?;
        self.conn.execute(
            "INSERT OR IGNORE INTO schema_migrations(version, applied_at) VALUES(23, ?1)",
            [Utc::now().to_rfc3339()],
        )?;
        // Raw records that fail normalization go to the quarantine table, instead of a skip stamp or a rollback of the whole payload.
        // The logical key is (raw_record_id, revision = current parser revision): a new revision does not match
        // old quarantine rows, so it tries again.
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS raw_quarantine (
                raw_record_id INTEGER PRIMARY KEY,
                stream TEXT NOT NULL,
                source_key TEXT NOT NULL,
                error TEXT NOT NULL,
                revision TEXT NOT NULL,
                quarantined_at TEXT NOT NULL,
                FOREIGN KEY(raw_record_id) REFERENCES raw_records(id) ON DELETE CASCADE
            );
            PRAGMA user_version = 24;",
        )?;
        self.conn.execute(
            "INSERT OR IGNORE INTO schema_migrations(version, applied_at) VALUES(24, ?1)",
            [Utc::now().to_rfc3339()],
        )?;
        // v25: index child rows for detail reads and foreign-key cascades.
        self.conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_sleep_stages_sleep
                ON sleep_stages(sleep_id, start_time);
             CREATE INDEX IF NOT EXISTS idx_workout_pauses_workout
                ON workout_pauses(workout_id, start_time);
             PRAGMA user_version = 25;",
        )?;
        self.conn.execute(
            "INSERT OR IGNORE INTO schema_migrations(version, applied_at) VALUES(25, ?1)",
            [Utc::now().to_rfc3339()],
        )?;
        // v26: unique-key prefixes already cover these lookup indexes.
        self.conn.execute_batch(
            "DROP INDEX IF EXISTS idx_metric_samples_metric_timestamp;
             DROP INDEX IF EXISTS idx_daily_metrics_date_metric;
             DROP INDEX IF EXISTS idx_workout_hr_zones_workout;
             CREATE INDEX IF NOT EXISTS idx_daily_metrics_metric_date
                 ON daily_metrics(metric, date);
             PRAGMA user_version = 26;",
        )?;
        self.conn.execute(
            "INSERT OR IGNORE INTO schema_migrations(version, applied_at) VALUES(26, ?1)",
            [Utc::now().to_rfc3339()],
        )?;
        // v27: deep / light / awake get the same "was it given" flags as REM.
        //
        // Old rows DEFAULT 1: a stored 0 still means "measured, and it is 0". We must not later
        // change it to "not given". New writes set the flag from Option::is_some.
        self.ensure_table_columns(
            "sleep_sessions",
            &[
                ("deep_available", "INTEGER NOT NULL DEFAULT 1"),
                ("light_available", "INTEGER NOT NULL DEFAULT 1"),
                ("awake_available", "INTEGER NOT NULL DEFAULT 1"),
            ],
        )?;
        self.conn.execute_batch("PRAGMA user_version = 27;")?;
        self.conn.execute(
            "INSERT OR IGNORE INTO schema_migrations(version, applied_at) VALUES(27, ?1)",
            [Utc::now().to_rfc3339()],
        )?;
        // v28: metric_samples were deduped above; give workout_hr_zones the
        // same ON DELETE CASCADE the other workout children already have.
        if version < 28 {
            self.conn.execute_batch(
                "CREATE TABLE workout_hr_zones_v28 (
                    workout_id TEXT NOT NULL,
                    zone_index INTEGER NOT NULL,
                    upper_bound_bpm INTEGER NOT NULL,
                    seconds INTEGER NOT NULL,
                    PRIMARY KEY (workout_id, zone_index),
                    FOREIGN KEY(workout_id) REFERENCES workouts(workout_id) ON DELETE CASCADE
                );
                INSERT INTO workout_hr_zones_v28
                    SELECT workout_id, zone_index, upper_bound_bpm, seconds
                    FROM workout_hr_zones
                    WHERE workout_id IN (SELECT workout_id FROM workouts);
                DROP TABLE workout_hr_zones;
                ALTER TABLE workout_hr_zones_v28 RENAME TO workout_hr_zones;",
            )?;
        }
        self.conn.execute_batch("PRAGMA user_version = 28;")?;
        self.conn.execute(
            "INSERT OR IGNORE INTO schema_migrations(version, applied_at) VALUES(28, ?1)",
            [Utc::now().to_rfc3339()],
        )?;
        // v29: workout detail fetch failure count. Without a limit, workouts with a permanent 404 fill
        // the pending fetch queue of every sync and use up the whole sync deadline.
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS workout_detail_fetch_attempts (
                workout_id TEXT NOT NULL,
                source TEXT NOT NULL,
                attempts INTEGER NOT NULL DEFAULT 0,
                updated_at TEXT NOT NULL,
                PRIMARY KEY (workout_id, source)
            );
             PRAGMA user_version = 29;",
        )?;
        self.conn.execute(
            "INSERT OR IGNORE INTO schema_migrations(version, applied_at) VALUES(29, ?1)",
            [Utc::now().to_rfc3339()],
        )?;
        // v30: normalization is an attempt/result, not ownership of a canonical row.
        if version < 30 {
            // Repair retained canonical history even if its raw payload expired.
            self.conn.execute(
                "DELETE FROM daily_metrics WHERE value = 255 AND metric IN (
                    'readiness', 'physical_readiness', 'mental_readiness',
                    'hrv_readiness', 'rhr_readiness', 'skin_temp_readiness',
                    'afib_readiness', 'ahi_readiness')",
                [],
            )?;
        }
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS raw_normalization (
                raw_record_id INTEGER PRIMARY KEY,
                revision TEXT NOT NULL,
                records_written INTEGER NOT NULL,
                FOREIGN KEY(raw_record_id) REFERENCES raw_records(id) ON DELETE CASCADE
             );
             CREATE TRIGGER IF NOT EXISTS invalidate_raw_normalization
             AFTER UPDATE OF payload_hash ON raw_records
             BEGIN
                DELETE FROM raw_normalization WHERE raw_record_id = NEW.id;
                DELETE FROM raw_quarantine WHERE raw_record_id = NEW.id;
             END;
             PRAGMA user_version = 30;",
        )?;
        self.conn.execute(
            "INSERT OR IGNORE INTO schema_migrations(version, applied_at) VALUES(30, ?1)",
            [Utc::now().to_rfc3339()],
        )?;
        // v31: reserve source, account, raw object and delivery receipt for dual delivery from the official and legacy connectors.
        // Official data is not written to these tables by default yet. The fields set the contract first, shared by fixtures and later providers.
        // All of it is incremental DDL with ensure_table_columns + CREATE IF NOT EXISTS.
        // By the rule of this file, it has no if version guard and reruns idempotently on each start.
        // Known gap (listed in beta1 known-limitations): unique indexes such as uq_metric_sample_key /
        // uq_daily_metric_key do not include provider in the key yet. Before official writes
        // really land, another index rebuild migration is needed. Until then, dual-source writes with the same key overwrite
        // each other, so the official source can only write to the separate tables created in this step.
        self.ensure_table_columns(
            "raw_records",
            &[
                ("provider", "TEXT NOT NULL DEFAULT 'legacy'"),
                ("account_id", "TEXT"),
                ("channel", "TEXT NOT NULL DEFAULT 'legacy_pull'"),
                ("received_at", "TEXT"),
                ("parsed_at", "TEXT"),
                ("persisted_at", "TEXT"),
            ],
        )?;
        self.ensure_table_columns(
            "metric_samples",
            &[
                ("provider", "TEXT NOT NULL DEFAULT 'legacy'"),
                ("account_id", "TEXT"),
            ],
        )?;
        self.ensure_table_columns(
            "daily_metrics",
            &[
                ("provider", "TEXT NOT NULL DEFAULT 'legacy'"),
                ("account_id", "TEXT"),
            ],
        )?;
        self.ensure_table_columns(
            "sleep_sessions",
            &[
                ("provider", "TEXT NOT NULL DEFAULT 'legacy'"),
                ("account_id", "TEXT"),
                ("rem_seconds", "INTEGER"),
                ("nap_total_seconds", "INTEGER"),
                ("unmeasurable_seconds", "INTEGER"),
                ("validation", "TEXT"),
            ],
        )?;
        self.ensure_table_columns(
            "workouts",
            &[
                ("provider", "TEXT NOT NULL DEFAULT 'legacy'"),
                ("account_id", "TEXT"),
            ],
        )?;
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS raw_objects (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                provider TEXT NOT NULL,
                account_id TEXT,
                object_key TEXT NOT NULL,
                content BLOB NOT NULL,
                sha256 TEXT NOT NULL,
                byte_len INTEGER NOT NULL,
                received_at TEXT NOT NULL,
                parsed_at TEXT,
                UNIQUE(provider, account_id, object_key, sha256)
            );
            CREATE TABLE IF NOT EXISTS delivery_receipts (
                event_key TEXT PRIMARY KEY,
                provider TEXT NOT NULL,
                account_id TEXT,
                channel TEXT NOT NULL,
                received_at TEXT NOT NULL,
                persisted_at TEXT,
                cursor TEXT,
                status TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS official_activity_pings (
                summary_id TEXT PRIMARY KEY,
                account_id TEXT,
                received_at TEXT NOT NULL,
                status TEXT NOT NULL,
                payload_hash TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS backfill_requests (
                request_key TEXT PRIMARY KEY,
                provider TEXT NOT NULL,
                account_id TEXT,
                stream TEXT NOT NULL,
                chunk_start TEXT NOT NULL,
                chunk_end TEXT NOT NULL,
                requested_at TEXT,
                status TEXT NOT NULL,
                attempts INTEGER NOT NULL DEFAULT 0,
                error_code TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_raw_objects_provider_received
                ON raw_objects(provider, received_at);
            CREATE INDEX IF NOT EXISTS idx_delivery_receipts_status
                ON delivery_receipts(status, received_at);
            PRAGMA user_version = 31;",
        )?;
        self.conn.execute(
            "INSERT OR IGNORE INTO schema_migrations(version, applied_at) VALUES(31, ?1)",
            [Utc::now().to_rfc3339()],
        )?;
        // v32: AI analysis tasks and templates.
        //
        // `payload` stores the full AiTask / AiTaskTemplate JSON (schema_version=1).
        // The other columns are index columns for list sorting, authorization filters (mcp_shared) and previews, so
        // `ai_task_list` and the MCP authorization path do not need to parse JSON row by row.
        // `BUILTIN_TEMPLATE_SEEDS` seeds the built-in templates. The inline text in each row is only fallback text.
        // The UI takes localized text from `name_code` / `prompt_code` in the payload.
        // All of it is CREATE IF NOT EXISTS + INSERT OR IGNORE, with no if guard,
        // and it reruns idempotently on each start. Deleted built-in rows come back.
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS ai_tasks (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL DEFAULT '',
                template_id TEXT,
                payload TEXT NOT NULL,
                workout_count INTEGER NOT NULL DEFAULT 0,
                mcp_shared INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS ai_task_templates (
                id TEXT PRIMARY KEY,
                builtin INTEGER NOT NULL DEFAULT 0,
                name TEXT NOT NULL DEFAULT '',
                payload TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            PRAGMA user_version = 32;",
        )?;
        for seed in crate::ai_tasks::BUILTIN_TEMPLATE_SEEDS.iter() {
            self.conn.execute(
                "INSERT OR IGNORE INTO ai_task_templates
                    (id, builtin, name, payload, created_at, updated_at)
                 VALUES (?1, 1, ?2, ?3, ?4, ?4)",
                params![
                    seed.id,
                    seed.name,
                    seed.payload,
                    crate::ai_tasks::BUILTIN_SEED_TIMESTAMP
                ],
            )?;
        }
        self.conn.execute(
            "INSERT OR IGNORE INTO schema_migrations(version, applied_at) VALUES(32, ?1)",
            [Utc::now().to_rfc3339()],
        )?;
        // v33: add the two missing pieces where "the query can use an index, but the index does not exist".
        //
        // The local-day range filter on workouts, with the `start_time >= ? AND < ?` grace bounds,
        // falls on start_time (the list `ORDER BY start_time` also benefits).
        // With no index, every range query and sort was a full table scan. The ai_tasks filter
        // `WHERE mcp_shared = 1` runs on each request of the MCP authorization path.
        // Both are CREATE IF NOT EXISTS, rerun idempotently, and do not touch any released DDL.
        self.conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_workouts_start
                 ON workouts(start_time);
             CREATE INDEX IF NOT EXISTS idx_ai_tasks_mcp
                 ON ai_tasks(mcp_shared);
             PRAGMA user_version = 33;",
        )?;
        self.conn.execute(
            "INSERT OR IGNORE INTO schema_migrations(version, applied_at) VALUES(33, ?1)",
            [Utc::now().to_rfc3339()],
        )?;
        // v34: official sleep integration.
        //
        // When both sides have the same night, store both (the official `sleep_id` is `official:<start seconds>`, so keys do not collide).
        // The UI, export and weekly report read this view: legacy sessions that overlap an official session of that night are hidden.
        // Official data has true REM values and naps, so it wins. This only adds a view and does not touch any released table or index.
        // 2.x (v30) does not read it and still reads sleep_sessions.
        self.conn.execute_batch(
            "CREATE VIEW IF NOT EXISTS sleep_sessions_shown AS
                SELECT s.* FROM sleep_sessions s
                WHERE s.provider = 'official'
                   OR NOT EXISTS (
                       SELECT 1 FROM sleep_sessions o
                       WHERE o.provider = 'official'
                         AND o.start_time < s.end_time AND o.end_time > s.start_time);
             CREATE INDEX IF NOT EXISTS idx_sleep_sessions_provider_start
                 ON sleep_sessions(provider, start_time);
             PRAGMA user_version = 34;",
        )?;
        self.conn.execute(
            "INSERT OR IGNORE INTO schema_migrations(version, applied_at) VALUES(34, ?1)",
            [Utc::now().to_rfc3339()],
        )?;
        // v35: training plan writes (official `POST /users/-/workouts`).
        //
        // The official API has no read endpoint, so "what is on the watch now" comes only from this publish ledger:
        // - `training_plan_drafts`: plan source text written by a person, a pasted AI reply or MCP.
        // - `training_plan_workouts`: workouts active after publish. Rows are append-only, and `id` is the
        //   V2 `workoutId`. A plan change only flips `active`, so old pushes in the ledger can always be restored.
        // - `training_plan_publishes`: per push, the full window snapshot, the sent payload and the result.
        //   **Write this row before the request is sent**, then add the result after the send.
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS training_plan_drafts (
                id TEXT PRIMARY KEY,
                origin TEXT NOT NULL,
                document TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'open',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS training_plan_workouts (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                draft_id TEXT NOT NULL,
                workout_date TEXT NOT NULL,
                workout TEXT NOT NULL,
                active INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL
             );
             CREATE INDEX IF NOT EXISTS idx_training_plan_workouts_active
                 ON training_plan_workouts(active, workout_date);
             CREATE TABLE IF NOT EXISTS training_plan_publishes (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                kind TEXT NOT NULL,
                draft_id TEXT,
                window_start TEXT NOT NULL,
                window_ids TEXT NOT NULL DEFAULT '[]',
                body TEXT NOT NULL,
                activated TEXT NOT NULL DEFAULT '[]',
                deactivated TEXT NOT NULL DEFAULT '[]',
                state TEXT NOT NULL,
                http_status INTEGER,
                error_code TEXT,
                undone INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL,
                finished_at TEXT
             );
             PRAGMA user_version = 35;",
        )?;
        self.conn.execute(
            "INSERT OR IGNORE INTO schema_migrations(version, applied_at) VALUES(35, ?1)",
            [Utc::now().to_rfc3339()],
        )?;
        // A brand-new database (user_version = 0 on entry) defaults to long-term archive: records belong to the user, with no automatic cleanup by default
        // (long-term retention is recommended at first setup). Existing databases are not touched: when they never wrote this key,
        // they always kept "only the last N days", and a new default would change the user's setting for them. Real databases shared with 2.x are also unaffected.
        if version == 0 {
            self.conn.execute(
                "INSERT OR IGNORE INTO app_meta(key, value, updated_at) VALUES(?1, '1', ?2)",
                params![super::ARCHIVE_ENABLED_KEY, Utc::now().to_rfc3339()],
            )?;
        }
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS ai_exchanges (
                id TEXT PRIMARY KEY, task_id TEXT NOT NULL, provider TEXT NOT NULL,
                question TEXT NOT NULL, days_before INTEGER NOT NULL, categories TEXT NOT NULL,
                workout_ids TEXT NOT NULL DEFAULT '[]', personal_note TEXT NOT NULL DEFAULT '',
                sent_at TEXT NOT NULL, md_path TEXT NOT NULL, plan_draft_id TEXT,
                received_at TEXT, publish_id INTEGER
             );
             CREATE INDEX IF NOT EXISTS idx_ai_exchanges_sent ON ai_exchanges(sent_at);
             PRAGMA user_version = 36;",
        )?;
        self.conn.execute(
            "INSERT OR IGNORE INTO schema_migrations(version,applied_at) VALUES(36,?1)",
            [Utc::now().to_rfc3339()],
        )?;
        // v37: multi-window push for training plans.
        //
        // One publish pushes every 7-day window the plan covers, one row per window:
        // - `batch_id`: rows of one batch share the `id` of the first row in the batch. Older rows are NULL (one row is one batch).
        // - `role`: `window` normal full window / `clear` empty array for the current window / `placeholder` clear of a future window
        //   with a placeholder. Placeholder rows must be identifiable: they leave one placeholder workout on the day before the window.
        // Columns are only added, released DDL does not change. Old rows read batch_id as NULL and role as 'window'.
        self.ensure_table_columns(
            "training_plan_publishes",
            &[
                ("batch_id", "INTEGER"),
                ("role", "TEXT NOT NULL DEFAULT 'window'"),
            ],
        )?;
        self.conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_training_plan_publishes_batch
                 ON training_plan_publishes(batch_id);
             PRAGMA user_version = 37;",
        )?;
        self.conn.execute(
            "INSERT OR IGNORE INTO schema_migrations(version,applied_at) VALUES(37,?1)",
            [Utc::now().to_rfc3339()],
        )?;
        // v38: saved tasks can be pinned. Only one new column. Old rows default to not pinned.
        // "Untouched for 30 days moves to older" is computed from updated_at at read time, not stored.
        self.ensure_table_columns("ai_tasks", &[("pinned", "INTEGER NOT NULL DEFAULT 0")])?;
        self.conn.execute_batch("PRAGMA user_version = 38;")?;
        self.conn.execute(
            "INSERT OR IGNORE INTO schema_migrations(version,applied_at) VALUES(38,?1)",
            [Utc::now().to_rfc3339()],
        )?;
        // v39 (3.0.4):
        // - `workout_samples.distance_m`: the watch's own cumulative distance (#139, FIT
        //   `record.distance`). Column only. Old rows are empty, and the replay after a revision bump fills them.
        // - `workout_fit_overrides`: the user replaces a workout with a corrected .fit (#140).
        //   Like workout type corrections, this is layer 3, "user local corrections": local only, not sent to the cloud,
        //   and replay does not touch it. **No foreign key to workouts on purpose**: summary payload replay deletes and reinserts
        //   workout rows, and with CASCADE the override would go with them. `original_*` holds the latest cloud
        //   values, which undo puts back unchanged.
        self.ensure_table_columns("workout_samples", &[("distance_m", "REAL")])?;
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS workout_fit_overrides (
                workout_id TEXT PRIMARY KEY,
                file_name TEXT,
                sha256 TEXT NOT NULL,
                byte_len INTEGER NOT NULL,
                fit BLOB NOT NULL,
                imported_at TEXT NOT NULL,
                fit_start_time TEXT NOT NULL,
                fit_end_time TEXT NOT NULL,
                fit_distance_m REAL,
                fit_moving_seconds INTEGER,
                fit_avg_hr INTEGER,
                fit_max_hr INTEGER,
                original_end_time TEXT,
                original_distance_m REAL,
                original_moving_seconds INTEGER,
                original_avg_hr INTEGER,
                original_max_hr INTEGER
             );
             PRAGMA user_version = 39;",
        )?;
        self.conn.execute(
            "INSERT OR IGNORE INTO schema_migrations(version,applied_at) VALUES(39,?1)",
            [Utc::now().to_rfc3339()],
        )?;
        // v40 (3.0.6, #143):
        // - `raw_quarantine.empty_response`: the cloud clearly returned an empty set (no data for that period). Data health
        //   reports it apart from "cannot parse". Before, the heart rate API returned empty per day, and thousands of rows counted as "quarantined", which looked like an error.
        //   Old rows default to 0. The replay after a revision bump judges each payload again.
        // - `workouts.gear_id`: `gearId` from the Zepp summary (the shoe the user chose for this workout). Only the id,
        //   the cloud gives no shoe name, so we do not invent one. Old rows are empty, and replay fills them.
        self.ensure_table_columns(
            "raw_quarantine",
            &[("empty_response", "INTEGER NOT NULL DEFAULT 0")],
        )?;
        self.ensure_table_columns("workouts", &[("gear_id", "TEXT")])?;
        self.conn.execute_batch("PRAGMA user_version = 40;")?;
        self.conn.execute(
            "INSERT OR IGNORE INTO schema_migrations(version,applied_at) VALUES(40,?1)",
            [Utc::now().to_rfc3339()],
        )?;
        // v41 (3.0.6): no structure change, only a one-time coverage ledger fix. Retention cleanup before 3.0 deleted data but did not update the ledger,
        // so those months stayed marked "written" and backfill always skipped them (see `reopen_chunks_without_raw`).
        // The fix is guarded by `version < 41`: it runs once and must not scan on each start. The version number cannot go inside the guard:
        // the `user_version = 40` lines above rewrite it on each start, so this step must always write 41 back.
        if version < 41 {
            let reopened = self.reopen_chunks_without_raw()?;
            if reopened > 0 {
                tracing::info!("Coverage ledger: {reopened} months were marked written but have no local data, now set back to pending backfill");
            }
        }
        self.conn.execute_batch("PRAGMA user_version = 41;")?;
        self.conn.execute(
            "INSERT OR IGNORE INTO schema_migrations(version,applied_at) VALUES(41,?1)",
            [Utc::now().to_rfc3339()],
        )?;
        // v42 (keep understanding local).
        //
        // `workout_feedback`: post-workout rating, how the user "felt" about a workout.
        // Like workout type corrections, it belongs to the user: **no foreign key to workouts** (replay deletes and reinserts workout rows),
        // and retention cleanup and replay do not touch it. `plan_workout_id` points to an append-only plan row, so when the plan changes later,
        // this rating still matches the version that was pushed to the watch. `state`: `rated` has a rating / `dismissed`
        // was closed with no rating (do not ask again for it). Rating 1-5 = too easy -> too hard. `adjustment`: what this rating
        // changed in the plan (today only `lighter_10`: next similar session 10% lighter, drafted and then confirmed by the user before send).
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS workout_feedback (
                workout_id TEXT PRIMARY KEY,
                plan_workout_id INTEGER,
                state TEXT NOT NULL CHECK(state IN ('rated', 'dismissed')),
                rating INTEGER CHECK(rating BETWEEN 1 AND 5),
                tags TEXT NOT NULL DEFAULT '[]',
                note TEXT,
                adjustment TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                CHECK(state = 'dismissed' OR rating IS NOT NULL)
             );
             CREATE INDEX IF NOT EXISTS idx_workout_feedback_plan
                 ON workout_feedback(plan_workout_id);
             -- AI memory inbox: the AI can only propose (proposed). It counts only after user confirmation (accepted).
             -- Rejected items stay (rejected), so the same text is not proposed again and again. Source is user / mcp,
             -- source_name is the name the MCP client reports (Claude Desktop, Cursor, ...), for display only.
             CREATE TABLE IF NOT EXISTS coach_notes (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                text TEXT NOT NULL,
                state TEXT NOT NULL CHECK(state IN ('proposed', 'accepted', 'rejected')),
                source TEXT NOT NULL CHECK(source IN ('user', 'mcp')),
                source_name TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                decided_at TEXT
             );
             CREATE INDEX IF NOT EXISTS idx_coach_notes_state ON coach_notes(state, id);
             -- Subjective layer: shoes, how each workout felt, injuries and races. All written by the user,
             -- and like ratings there is no foreign key to workouts. Replay and retention cleanup do not touch them.
             -- Shoes: `sport` is the shoe use (running / trail / walking / hiking). `zepp_gear_id`
             -- maps the gearId in the Zepp summary to this shoe (the cloud gives only the id, no name). `start_km` is the distance
             -- run before registration. `retire_km` is the retirement reminder distance (default 700, common 600-800).
             CREATE TABLE IF NOT EXISTS shoes (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                sport TEXT NOT NULL,
                is_default INTEGER NOT NULL DEFAULT 0,
                zepp_gear_id TEXT,
                start_km REAL NOT NULL DEFAULT 0,
                retire_km REAL NOT NULL DEFAULT 700,
                started_on TEXT NOT NULL,
                retired_on TEXT,
                notes TEXT NOT NULL DEFAULT '',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
             );
             -- How a workout felt: own RPE (1-10, the watch value is in workouts.rpe and is not overwritten),
             -- sore body parts (body map part ids, JSON array), and which shoe was worn. `shoe_mode`:
             -- auto matches by Zepp id / default shoe, none means no registered shoe was worn, chosen uses `shoe_id`.
             CREATE TABLE IF NOT EXISTS workout_annotations (
                workout_id TEXT PRIMARY KEY,
                rpe INTEGER CHECK(rpe BETWEEN 1 AND 10),
                soreness TEXT NOT NULL DEFAULT '[]',
                shoe_mode TEXT NOT NULL DEFAULT 'auto' CHECK(shoe_mode IN ('auto', 'none', 'chosen')),
                shoe_id INTEGER,
                updated_at TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS injuries (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                title TEXT NOT NULL,
                body_part TEXT,
                severity INTEGER CHECK(severity BETWEEN 1 AND 3),
                start_date TEXT NOT NULL,
                end_date TEXT,
                notes TEXT NOT NULL DEFAULT '',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                CHECK(end_date IS NULL OR end_date >= start_date)
             );
             -- Races: registered and finished. `result_seconds` is the result the user entered (official results win over the watch),
             -- `workout_id` is the matched workout.
             CREATE TABLE IF NOT EXISTS races (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                race_date TEXT NOT NULL,
                sport TEXT NOT NULL,
                distance_m REAL,
                goal_seconds INTEGER,
                priority TEXT NOT NULL DEFAULT 'a' CHECK(priority IN ('a', 'b', 'c')),
                result_seconds INTEGER,
                workout_id TEXT,
                notes TEXT NOT NULL DEFAULT '',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
             );
             CREATE INDEX IF NOT EXISTS idx_races_date ON races(race_date);
             PRAGMA user_version = 42;",
        )?;
        // Epochs: name a period of days (for example training for a first marathon, or injury recovery), and charts get a background band. It is a life event with one more flag.
        self.ensure_table_columns("life_events", &[("epoch", "INTEGER NOT NULL DEFAULT 0")])?;
        self.conn.execute(
            "INSERT OR IGNORE INTO schema_migrations(version,applied_at) VALUES(42,?1)",
            [Utc::now().to_rfc3339()],
        )?;
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS workout_local_state (
                workout_id TEXT PRIMARY KEY,
                type_override TEXT,
                hidden INTEGER NOT NULL DEFAULT 0 CHECK(hidden IN (0, 1))
             );
             INSERT OR IGNORE INTO workout_local_state(workout_id, type_override)
                SELECT workout_id, workout_type_override FROM workouts WHERE workout_type_override IS NOT NULL;
             CREATE TABLE IF NOT EXISTS workout_edit_history (
                id TEXT PRIMARY KEY, before_json TEXT NOT NULL, after_json TEXT NOT NULL,
                created_at TEXT NOT NULL, undone INTEGER NOT NULL DEFAULT 0
             );
             CREATE VIEW IF NOT EXISTS workouts_visible AS SELECT * FROM workouts w
                WHERE NOT EXISTS (SELECT 1 FROM workout_local_state s WHERE s.workout_id = w.workout_id AND s.hidden = 1);
             CREATE VIEW IF NOT EXISTS workouts_hidden AS SELECT * FROM workouts w
                WHERE EXISTS (SELECT 1 FROM workout_local_state s WHERE s.workout_id = w.workout_id AND s.hidden = 1);
             CREATE TRIGGER IF NOT EXISTS restore_local_workout_type AFTER INSERT ON workouts
                WHEN EXISTS (SELECT 1 FROM workout_local_state s WHERE s.workout_id = NEW.workout_id)
                BEGIN UPDATE workouts SET workout_type_override = (SELECT type_override FROM workout_local_state WHERE workout_id = NEW.workout_id)
                      WHERE workout_id = NEW.workout_id; END;
             CREATE TABLE IF NOT EXISTS workout_detail_requests (
                workout_id TEXT PRIMARY KEY, status TEXT NOT NULL, attempts INTEGER NOT NULL DEFAULT 0,
                requested_at TEXT, retry_after TEXT, error_code TEXT
             );
             PRAGMA user_version = 43;",
        )?;
        self.conn.execute(
            "INSERT OR IGNORE INTO schema_migrations(version, applied_at) VALUES(43, ?1)",
            [Utc::now().to_rfc3339()],
        )?;
        // v44 (data letters in the letterbox). Weekly data letters and time capsules are snapshots of the moment they were written:
        // later reparsing or backfill can change derived data, but old letters do not change. A letter stays as it was sent.
        // `period_key` is only for weekly letters (`weekly:<last day of that week>`), so each week gets one letter only.
        // A capsule `note` is the user's message to the future. The UI cannot read the text before `open_on` (local day).
        // Like ratings and shoes, it belongs to the user. Replay and retention cleanup do not touch it.
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS data_letters (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                kind TEXT NOT NULL CHECK(kind IN ('weekly', 'capsule')),
                period_key TEXT UNIQUE,
                written_at TEXT NOT NULL,
                open_on TEXT,
                opened_at TEXT,
                note TEXT NOT NULL DEFAULT '',
                body TEXT NOT NULL
             );
             CREATE INDEX IF NOT EXISTS idx_data_letters_kind ON data_letters(kind, id);
             PRAGMA user_version = 44;",
        )?;
        self.conn.execute(
            "INSERT OR IGNORE INTO schema_migrations(version, applied_at) VALUES(44, ?1)",
            [Utc::now().to_rfc3339()],
        )?;
        // v45 (prompt library). The previous text of a task prompt each time it is changed. No foreign key to ai_tasks:
        // undo of a delete stores the task back unchanged, and its history must come back with it.
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS ai_prompt_versions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                task_id TEXT NOT NULL,
                prompt TEXT NOT NULL,
                replaced_at TEXT NOT NULL
             );
             CREATE INDEX IF NOT EXISTS idx_ai_prompt_versions_task ON ai_prompt_versions(task_id, id);
             PRAGMA user_version = 45;",
        )?;
        self.conn.execute(
            "INSERT OR IGNORE INTO schema_migrations(version, applied_at) VALUES(45, ?1)",
            [Utc::now().to_rfc3339()],
        )?;
        // v46 (multi-source import). Workouts imported from GPX / FIT / Strava export / Apple Health are written to
        // workouts as usual (raw_record_id is empty, replay does not touch them). This table records which batch and source they came from,
        // and undo of a whole batch deletes by batch_id. An undone batch keeps one row (undone_at), and its workouts are deleted.
        // Imported workouts are brought in by the user, so retention cleanup does not delete them.
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS import_batches (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                source TEXT NOT NULL CHECK(source IN ('gpx', 'fit', 'strava', 'apple_health')),
                file_name TEXT,
                imported_at TEXT NOT NULL,
                added INTEGER NOT NULL,
                duplicates INTEGER NOT NULL,
                unreadable INTEGER NOT NULL,
                undone_at TEXT
             );
             CREATE TABLE IF NOT EXISTS workout_imports (
                workout_id TEXT PRIMARY KEY,
                batch_id INTEGER NOT NULL,
                source TEXT NOT NULL,
                name TEXT
             );
             CREATE INDEX IF NOT EXISTS idx_workout_imports_batch ON workout_imports(batch_id);
             PRAGMA user_version = 46;",
        )?;
        self.conn.execute(
            "INSERT OR IGNORE INTO schema_migrations(version, applied_at) VALUES(46, ?1)",
            [Utc::now().to_rfc3339()],
        )?;
        // v47 (3.1.1: hidden devices). Devices the user hid in the device list, one row per device key (device_id or
        // serial number, case-insensitive, same scheme as device_model_overrides). It only hides the list entry:
        // samples, charts and exports ignore this table. No foreign key: when the device leaves the account, the row stays and does no harm.
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS device_local_state (
                device_key TEXT PRIMARY KEY,
                hidden INTEGER NOT NULL DEFAULT 0,
                updated_at TEXT NOT NULL
             );
             PRAGMA user_version = 47;",
        )?;
        self.conn.execute(
            "INSERT OR IGNORE INTO schema_migrations(version, applied_at) VALUES(47, ?1)",
            [Utc::now().to_rfc3339()],
        )?;
        // v48 (3.1.2: nap stages). Official sleep `napStage` is a list of segments with stages (light / deep / REM / awake),
        // with the same codes as night sleep. A separate table instead of sleep_stages: that table holds the stage bar of "the night",
        // and afternoon segments would stretch the whole night timeline to the next afternoon. Rows cascade-delete with their sleep session.
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS sleep_nap_stages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                sleep_id TEXT NOT NULL,
                stage TEXT NOT NULL,
                start_time TEXT NOT NULL,
                end_time TEXT NOT NULL,
                raw_mode INTEGER,
                FOREIGN KEY(sleep_id) REFERENCES sleep_sessions(sleep_id) ON DELETE CASCADE
             );
             CREATE INDEX IF NOT EXISTS idx_sleep_nap_stages_sleep
                ON sleep_nap_stages(sleep_id, start_time);
             PRAGMA user_version = 48;",
        )?;
        self.conn.execute(
            "INSERT OR IGNORE INTO schema_migrations(version, applied_at) VALUES(48, ?1)",
            [Utc::now().to_rfc3339()],
        )?;
        // v49 (3.1.2: change ledger of the local AI). Each MCP write tool call (injuries, life events, races, shoes, workout notes,
        // type changes, coach notes) records one row: snapshots before and after the change, so the desktop app can undo one by one.
        // Sync and export do not go into this table: they are actions and do not change records.
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS mcp_writes (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                tool TEXT NOT NULL,
                kind TEXT NOT NULL,
                target_id TEXT NOT NULL,
                label TEXT NOT NULL DEFAULT '',
                before_json TEXT,
                after_json TEXT NOT NULL,
                source_name TEXT,
                created_at TEXT NOT NULL,
                undone_at TEXT
             );
             CREATE INDEX IF NOT EXISTS idx_mcp_writes_target ON mcp_writes(kind, target_id);
             PRAGMA user_version = 49;",
        )?;
        self.conn.execute(
            "INSERT OR IGNORE INTO schema_migrations(version, applied_at) VALUES(49, ?1)",
            [Utc::now().to_rfc3339()],
        )?;
        // v50 (Beta lab: unstrap backup import). Add 'unstrap' to the CHECK on import_batches.source.
        // SQLite cannot alter a CHECK, so rebuild the table as is and copy the data. workout_imports.source has no constraint and is not touched.
        if version < 50 {
            self.conn.execute_batch(
                "CREATE TABLE import_batches_v50 (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    source TEXT NOT NULL CHECK(source IN ('gpx', 'fit', 'strava', 'apple_health', 'unstrap')),
                    file_name TEXT,
                    imported_at TEXT NOT NULL,
                    added INTEGER NOT NULL,
                    duplicates INTEGER NOT NULL,
                    unreadable INTEGER NOT NULL,
                    undone_at TEXT
                 );
                 INSERT INTO import_batches_v50(id, source, file_name, imported_at, added, duplicates, unreadable, undone_at)
                    SELECT id, source, file_name, imported_at, added, duplicates, unreadable, undone_at FROM import_batches;
                 DROP TABLE import_batches;
                 ALTER TABLE import_batches_v50 RENAME TO import_batches;",
            )?;
        }
        self.conn.execute_batch("PRAGMA user_version = 50;")?;
        self.conn.execute(
            "INSERT OR IGNORE INTO schema_migrations(version, applied_at) VALUES(50, ?1)",
            [Utc::now().to_rfc3339()],
        )?;
        // v51 (Beta lab: unstrap health sample import). Imported samples / daily values / sleeps record their batch.
        // Undo of a whole batch deletes by it, and retention cleanup and "latest data" skip it. The batch also records how many health rows it added.
        // Add the column when it is missing (not by version number): the v50 rebuild of import_batches drops this column, and a downgrade rerun must add it back.
        {
            for table in ["metric_samples", "daily_metrics", "sleep_sessions"] {
                self.ensure_table_columns(table, &[("import_batch_id", "INTEGER")])?;
            }
            self.ensure_table_columns(
                "import_batches",
                &[("health_added", "INTEGER NOT NULL DEFAULT 0")],
            )?;
            self.conn.execute_batch(
                "CREATE INDEX IF NOT EXISTS idx_metric_samples_import
                    ON metric_samples(import_batch_id) WHERE import_batch_id IS NOT NULL;
                 CREATE INDEX IF NOT EXISTS idx_daily_metrics_import
                    ON daily_metrics(import_batch_id) WHERE import_batch_id IS NOT NULL;
                 CREATE INDEX IF NOT EXISTS idx_sleep_sessions_import
                    ON sleep_sessions(import_batch_id) WHERE import_batch_id IS NOT NULL;",
            )?;
        }
        self.conn.execute_batch("PRAGMA user_version = 51;")?;
        self.conn.execute(
            "INSERT OR IGNORE INTO schema_migrations(version, applied_at) VALUES(51, ?1)",
            [Utc::now().to_rfc3339()],
        )?;
        // v52 (#163: per-set strength training detail and summary fallback). The 62-column detail laps decode into workout_strength_sets.
        // Set count, total reps, total rest between sets and the exercise list from the summary stream go into workouts columns. With a revision bump replay, old databases fill in automatically.
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS workout_strength_sets (
                workout_id TEXT NOT NULL,
                set_index INTEGER NOT NULL,
                action_code INTEGER NOT NULL,
                exercise_key TEXT,
                muscle_group TEXT,
                reps INTEGER NOT NULL,
                weight_value REAL,
                weight_unit TEXT NOT NULL CHECK(weight_unit IN ('kg', 'lb')),
                weight_kg REAL,
                duration_ms INTEGER,
                avg_hr INTEGER,
                calories_kcal REAL,
                PRIMARY KEY (workout_id, set_index),
                FOREIGN KEY(workout_id) REFERENCES workouts(workout_id) ON DELETE CASCADE
             );",
        )?;
        self.ensure_table_columns(
            "workouts",
            &[
                ("strength_total_sets", "INTEGER"),
                ("strength_total_reps", "INTEGER"),
                ("strength_rest_seconds", "INTEGER"),
                ("strength_groups_json", "TEXT"),
            ],
        )?;
        self.conn.execute_batch("PRAGMA user_version = 52;")?;
        self.conn.execute(
            "INSERT OR IGNORE INTO schema_migrations(version, applied_at) VALUES(52, ?1)",
            [Utc::now().to_rfc3339()],
        )?;
        self.ensure_table_columns("workout_fit_overrides", &[("original_series_json", "TEXT")])?;
        self.ensure_table_columns(
            "route_points",
            &[("segment_start", "INTEGER NOT NULL DEFAULT 0")],
        )?;
        self.conn.execute_batch("PRAGMA user_version = 53;")?;
        self.conn.execute(
            "INSERT OR IGNORE INTO schema_migrations(version, applied_at) VALUES(53, ?1)",
            [Utc::now().to_rfc3339()],
        )?;
        self.ensure_cloud_sync_metadata()?;
        Ok(())
    }
}
