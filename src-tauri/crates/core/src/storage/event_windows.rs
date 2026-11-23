//! Compact old "full window" daily event payloads into per-day payloads (see `crate::event_days`).
//!
//! Before 3.0, each sync stored the last 30 days of Charge / readiness / DailyHealth as one
//! new window payload. Later payloads took over their derived rows long ago, but the payloads stayed. In one real database
//! 719 Charge window payloads used 525 MB, and no derived row pointed to 694 of them.
//!
//! Compaction has one rule: **for each day, keep only the newest fetched version**.
//!
//! 1. Walk the window payloads of one family (`events:<type>:<sub>`) from newest to oldest.
//! 2. The first time a day appears, write its items unchanged as one per-day payload. `fetched_at` keeps the cloud fetch time
//!    of the original payload, and the current parser normalizes it, so derived rows point to the new payload. Skip days that already
//!    have a newer per-day payload.
//! 3. Then delete a window payload when no derived row points to it and it has no item without a readable timestamp.
//!
//! Normalizing the per-day split gives the same result as the full window (checked one by one on 2161 payloads in a real database), so derived
//! data does not change. Only the payload storage changes. Before a delete we always confirm no derived row points to it, and the foreign key
//! also blocks a wrong delete.

use super::*;
use crate::event_days::{self, EventDays};

/// The last compacted payload (`raw_records.id`). New window payloads that come later (for example, 2.x still syncing to the same
/// database) get compacted at the next start.
const EVENT_WINDOW_MARKER_KEY: &str = "event_window_consolidated_id";

/// Window payloads smaller than this byte count are not "pending compaction": an empty window `{"items":[]}` is only 12 bytes,
/// and a "compacting" message at start is not worth it. A window with one item is far larger. Compaction
/// still removes empty windows with the rest.
const EVENT_WINDOW_MIN_PENDING_BYTES: i64 = 64;

/// Window payloads per transaction. One decoded Charge payload is 4.7 MB, so this sets peak memory
/// and peak WAL, not speed.
const EVENT_WINDOW_BATCH: usize = 8;

struct WindowRaw {
    id: i64,
    family: String,
    fetched_at: String,
    fetched: DateTime<Utc>,
    stored_bytes: u64,
}

impl Database {
    /// Count of non-empty window-shaped daily event payloads not yet compacted.
    pub(crate) fn pending_event_window_count(&self) -> Result<i64> {
        let marker = self.event_window_marker()?;
        let mut stmt = self.conn.prepare(
            "SELECT source_key FROM raw_records
             WHERE stream = 'daily_summary' AND source_key GLOB 'events:*' AND id > ?1
               AND LENGTH(payload) + COALESCE(LENGTH(payload_zip), 0) > ?2",
        )?;
        let keys = stmt.query_map(params![marker, EVENT_WINDOW_MIN_PENDING_BYTES], |row| {
            row.get::<_, String>(0)
        })?;
        let mut count = 0;
        for key in keys {
            if event_days::window_key_family(&key?).is_some() {
                count += 1;
            }
        }
        Ok(count)
    }

    /// Run one compaction pass. Deleted window payloads count in `report.compacted`. Freed and newly written bytes count in
    /// `bytes_before` / `bytes_after`. A quit request stops it at a batch boundary, the marker does not move,
    /// and the next start continues.
    pub(super) fn consolidate_event_windows(
        &self,
        report: &mut RawPayloadCompaction,
    ) -> Result<()> {
        let marker = self.event_window_marker()?;
        let windows = self.window_raws_after(marker)?;
        let Some(max_id) = windows.iter().map(|raw| raw.id).max() else {
            return Ok(());
        };
        // On the first compaction of this database, keep a verifiable snapshot first: compaction deletes thousands of payloads, and deleted ones
        // cannot come back. Same rule as migrations: no start without a successful backup.
        if marker == 0 && self.pending_event_window_count()? > 0 {
            self.backup_before_consolidation()?;
        }
        let mut families: BTreeMap<String, Vec<WindowRaw>> = BTreeMap::new();
        for raw in windows {
            families.entry(raw.family.clone()).or_default().push(raw);
        }
        for (family, raws) in families {
            if !self.consolidate_family(&family, raws, report)? {
                return Ok(());
            }
        }
        self.set_app_meta(EVENT_WINDOW_MARKER_KEY, &max_id.to_string())?;
        // Compaction is fully done (interrupted passes do not get here). If the database is intact, delete the pre-compaction snapshot.
        if let Some(data_dir) = self.data_dir() {
            self.discard_snapshots_if_healthy(&data_dir);
        }
        Ok(())
    }

    fn backup_before_consolidation(&self) -> Result<()> {
        // An in-memory database (tests) has no file, so there is nothing to back up.
        let Some(data_dir) = self.data_dir() else {
            return Ok(());
        };
        match backup::create_backup(&data_dir, backup::BackupKind::PreMigration, APP_VERSION) {
            Ok(_) => {
                let _ = backup::prune_migration_backups(&data_dir);
                Ok(())
            }
            Err(error) => Err(ZeppBridgeError::DataUnavailable(format!(
                "The automatic backup before compacting old payloads failed, so compaction did not start: {}",
                error.user_message()
            ))),
        }
    }

    fn event_window_marker(&self) -> Result<i64> {
        Ok(self
            .get_app_meta(EVENT_WINDOW_MARKER_KEY)?
            .and_then(|value| value.parse().ok())
            .unwrap_or(0))
    }

    /// All window payloads after the marker, newest to oldest in each family.
    fn window_raws_after(&self, marker: i64) -> Result<Vec<WindowRaw>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, source_key, fetched_at,
                    LENGTH(payload) + COALESCE(LENGTH(payload_zip), 0)
             FROM raw_records
             WHERE stream = 'daily_summary' AND source_key GLOB 'events:*' AND id > ?1",
        )?;
        let rows = stmt.query_map([marker], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, key, fetched_at, bytes) = row?;
            let Some(family) = event_days::window_key_family(&key) else {
                continue;
            };
            let Ok(fetched) = parse_datetime(&fetched_at, "raw_records.fetched_at") else {
                continue;
            };
            out.push(WindowRaw {
                id,
                family: family.to_string(),
                fetched_at,
                fetched,
                stored_bytes: bytes.max(0) as u64,
            });
        }
        out.sort_by(|a, b| b.fetched.cmp(&a.fetched).then(b.id.cmp(&a.id)));
        Ok(out)
    }

    /// When the existing per-day payload of each day was fetched.
    fn family_day_fetches(&self, family: &str) -> Result<HashMap<NaiveDate, DateTime<Utc>>> {
        let prefix = format!("{family}:day:");
        let mut stmt = self.conn.prepare(
            "SELECT source_key, fetched_at FROM raw_records
             WHERE stream = 'daily_summary' AND substr(source_key, 1, ?2) = ?1",
        )?;
        let rows = stmt.query_map(params![prefix, prefix.len() as i64], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut out = HashMap::new();
        for row in rows {
            let (key, fetched_at) = row?;
            let Some(day) = key
                .strip_prefix(&prefix)
                .and_then(|day| NaiveDate::parse_from_str(day, "%Y-%m-%d").ok())
            else {
                continue;
            };
            if let Ok(at) = parse_datetime(&fetched_at, "raw_records.fetched_at") {
                out.insert(day, at);
            }
        }
        Ok(out)
    }

    /// Returns `false` when a quit request interrupted it.
    fn consolidate_family(
        &self,
        family: &str,
        raws: Vec<WindowRaw>,
        report: &mut RawPayloadCompaction,
    ) -> Result<bool> {
        let mut parts = family.splitn(3, ':').skip(1);
        let (Some(event_type), Some(sub_type)) = (parts.next(), parts.next()) else {
            return Ok(true);
        };
        let mut days = self.family_day_fetches(family)?;
        // Time of the newest fetch in this family: an empty window can be deleted only when "a later fetch happened".
        // Otherwise it is the only evidence that "the last fetch came back empty".
        let mut newest = days.values().max().copied();
        for batch in raws.chunks(EVENT_WINDOW_BATCH) {
            if background_write_abort_requested() {
                return Ok(false);
            }
            let transaction = ReplayBatch::begin(&self.conn)?;
            for raw in batch {
                let has_newer = newest.is_some_and(|at| at > raw.fetched);
                newest = Some(newest.map_or(raw.fetched, |at| at.max(raw.fetched)));
                let Some((payload, zip)) = self.raw_payload(raw.id)? else {
                    continue;
                };
                // Keep unreadable payloads unchanged: replay depends only on them, and compaction must not decide for them.
                let Ok(text) = decode_raw_payload(payload, zip) else {
                    continue;
                };
                let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
                    continue;
                };
                let deletable = match event_days::split_by_utc_day(&value) {
                    None => has_newer,
                    Some(EventDays {
                        days: split,
                        residual,
                    }) => {
                        for (day, day_payload) in split {
                            if days.get(&day).is_some_and(|at| *at >= raw.fetched) {
                                continue;
                            }
                            report.bytes_after += self.write_consolidated_day(
                                event_type,
                                sub_type,
                                day,
                                day_payload,
                                &raw.fetched_at,
                            )?;
                            days.insert(day, raw.fetched);
                        }
                        residual.is_none()
                    }
                };
                if deletable && self.delete_unreferenced_raw(raw.id)? {
                    report.compacted += 1;
                    report.bytes_before += raw.stored_bytes;
                }
            }
            transaction.commit()?;
        }
        Ok(true)
    }

    /// Write and normalize the per-day payload of one day. Returns its compressed size in bytes.
    fn write_consolidated_day(
        &self,
        event_type: &str,
        sub_type: &str,
        day: NaiveDate,
        payload: serde_json::Value,
        fetched_at: &str,
    ) -> Result<u64> {
        let (start_utc, end_utc) = event_days::day_bounds(day);
        let record = RawRecord {
            stream: "daily_summary".into(),
            source_key: event_days::day_source_key(event_type, sub_type, day),
            source_scope: SourceScope::UserFused,
            device_id: None,
            start_utc,
            end_utc: Some(end_utc),
            payload,
            capability: CapabilityStatus::Verified,
        };
        let serialized = SerializedPayload::of(&record)?;
        let id = self.insert_serialized_raw(&record, &serialized, fetched_at)?;
        // Same as replay: wrap "clear old rows + insert new rows" for one payload on its own, so a failure rolls back only that payload.
        self.conn.execute("SAVEPOINT consolidate_day", [])?;
        match self.normalize_and_persist_raw(
            id,
            &record.stream,
            &record.source_key,
            &record.payload,
        ) {
            Ok(_) => {
                self.conn.execute("RELEASE consolidate_day", [])?;
                self.clear_raw_quarantine(id)?;
            }
            Err(error) => {
                self.conn.execute("ROLLBACK TO consolidate_day", [])?;
                self.conn.execute("RELEASE consolidate_day", [])?;
                self.insert_raw_quarantine(
                    id,
                    &record.stream,
                    &record.source_key,
                    &error,
                    Some(&record.payload),
                )?;
            }
        }
        Ok(self
            .conn
            .query_row(
                "SELECT LENGTH(payload) + COALESCE(LENGTH(payload_zip), 0)
                 FROM raw_records WHERE id = ?1",
                [id],
                |row| row.get::<_, i64>(0),
            )?
            .max(0) as u64)
    }

    /// Delete only when no derived row points to it. Returns whether it was deleted.
    fn delete_unreferenced_raw(&self, id: i64) -> Result<bool> {
        let deleted = self.conn.execute(
            "DELETE FROM raw_records
             WHERE id = ?1
               AND NOT EXISTS (SELECT 1 FROM metric_samples m WHERE m.raw_record_id = ?1)
               AND NOT EXISTS (SELECT 1 FROM daily_metrics d WHERE d.raw_record_id = ?1)
               AND NOT EXISTS (SELECT 1 FROM sleep_sessions s WHERE s.raw_record_id = ?1)
               AND NOT EXISTS (SELECT 1 FROM workouts w WHERE w.raw_record_id = ?1)",
            [id],
        )?;
        Ok(deleted > 0)
    }
}
