//! Replays local raw payloads by NORMALIZER_REVISION (split out of storage/mod.rs by domain, logic unchanged).

use super::*;

#[derive(Debug, Clone, Default)]
pub struct NormalizationCounts {
    pub primary_records: i64,
    pub band_heart_rate_records: i64,
    pub supplemental_daily_records: i64,
}

/// A replay this database owes: from which revision to which revision, and how many payloads to process.
///
/// It exists to "describe the work before doing it". A replay takes minutes, but a command like `status`
/// must answer at once. So computing the plan (read-only, two SELECTs) and running it must be separate calls:
/// short commands use it to say "your history still uses the old parser", and `reprocess` uses it to do the work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayPlan {
    /// The parser revision that produced the derived data in the database. `None` = never replayed.
    pub stored_revision: Option<String>,
    /// The revision this program upgrades it to.
    pub target_revision: String,
    /// The streams to replay. Empty = all streams.
    pub streams: Vec<String>,
    /// The number of raw payloads this replay processes. 0 = the database is empty and the replay is instant.
    pub raw_records: i64,
}

/// How to run one replay.
#[derive(Debug, Clone, Copy, Default)]
struct ReplayOptions<'a> {
    /// Only process payloads that do not yet have the current revision stamp.
    unstamped_only: bool,
    /// With a data directory, take the write lock once per batch (see `reprocess_raw_records_if_needed_batched`).
    batch_lock: Option<&'a Path>,
}

impl Database {
    /// The parser revision stored in the database. `None` = this database was never replayed.
    ///
    /// This is not the same as `NORMALIZER_REVISION`. That one says which rules **this program** parses with.
    /// This one says which rules produced the **derived data in the database**. When they differ, the historical
    /// records still use the old rules, and only a replay can bring them in line.
    pub fn stored_normalizer_revision(&self) -> Result<Option<String>> {
        self.conn
            .query_row(
                "SELECT value FROM app_meta WHERE key = 'normalizer_revision'",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(Into::into)
    }

    /// The replay this database owes now. `None` = nothing owed.
    ///
    /// Read-only: two SELECTs, so a read-only connection can call it. `status` and the MCP health report must
    /// be able to say "your history still uses the old parser". They must not write a single byte, and they must
    /// not silently run for minutes in a command that must answer at once.
    pub fn pending_replay_plan(&self) -> Result<Option<ReplayPlan>> {
        let stored = self.stored_normalizer_revision()?;
        if stored.as_deref() == Some(NORMALIZER_REVISION) && self.unstamped_raw_count()? == 0 {
            return Ok(None);
        }
        // Revisit all streams: date parsing is shared, and older upgrades
        // still need readiness repair and per-raw attempt history.
        let streams: Vec<String> = Vec::new();
        let raw_records = self.count_raw_records_for_streams(&streams)?;
        Ok(Some(ReplayPlan {
            stored_revision: stored,
            target_revision: NORMALIZER_REVISION.to_string(),
            streams,
            raw_records,
        }))
    }

    /// The number of raw payloads not yet stamped or quarantined by the current parser. When the revision is unchanged, the replay only processes these.
    pub fn unstamped_raw_count(&self) -> Result<i64> {
        self.conn
            .query_row(
                "SELECT COUNT(*) FROM raw_records r
                 WHERE NOT EXISTS (SELECT 1 FROM raw_normalization n
                                   WHERE n.raw_record_id = r.id AND n.revision = ?1)
                   AND NOT EXISTS (SELECT 1 FROM raw_quarantine q
                                   WHERE q.raw_record_id = r.id AND q.revision = ?1)",
                [NORMALIZER_REVISION],
                |row| row.get(0),
            )
            .map_err(Into::into)
    }

    /// The total number of raw payloads in the database. Before a full replay, use it to state "how many to process".
    pub fn raw_record_count(&self) -> Result<i64> {
        self.count_raw_records_for_streams(&[])
    }

    /// An empty `streams` means all streams, the same convention as `ReplayPlan::streams`.
    pub(super) fn count_raw_records_for_streams(&self, streams: &[String]) -> Result<i64> {
        if streams.is_empty() {
            return self
                .conn
                .query_row("SELECT COUNT(*) FROM raw_records", [], |row| row.get(0))
                .map_err(Into::into);
        }
        let placeholders = (1..=streams.len())
            .map(|index| format!("?{index}"))
            .collect::<Vec<_>>()
            .join(", ");
        self.conn
            .query_row(
                &format!("SELECT COUNT(*) FROM raw_records WHERE stream IN ({placeholders})"),
                rusqlite::params_from_iter(streams.iter()),
                |row| row.get(0),
            )
            .map_err(Into::into)
    }

    pub fn reprocess_raw_records_if_needed(&self) -> Result<Option<BTreeMap<String, i64>>> {
        self.reprocess_if_needed_with(None)
    }

    /// Same as [`Self::reprocess_raw_records_if_needed`], but takes the cross-process write lock once **per batch**
    /// and releases it between batches.
    ///
    /// The full replay at startup runs for more than ten minutes. Before, it held the write lock the whole time (even though it committed every 64 rows):
    /// during that time, changing settings, logging life events or resetting the ledger all failed after 5 seconds with "another write operation is in progress".
    /// Now short writes can get in between batches. Callers must not take the write lock outside.
    pub fn reprocess_raw_records_if_needed_batched(
        &self,
        data_dir: &Path,
    ) -> Result<Option<BTreeMap<String, i64>>> {
        self.reprocess_if_needed_with(Some(data_dir))
    }

    fn reprocess_if_needed_with(
        &self,
        batch_lock: Option<&Path>,
    ) -> Result<Option<BTreeMap<String, i64>>> {
        let Some(plan) = self.pending_replay_plan()? else {
            return Ok(None);
        };
        // The revision is unchanged and only some payloads lack the stamp (a crash before normalization, or an exit cut the last run short): only process
        // those. Before, one missing stamp caused a full replay.
        let options = ReplayOptions {
            unstamped_only: plan.stored_revision.as_deref() == Some(NORMALIZER_REVISION),
            batch_lock,
        };
        let counts = if plan.streams.is_empty() {
            self.reprocess_raw_records_with(None, options)?
        } else {
            let streams: Vec<&str> = plan.streams.iter().map(String::as_str).collect();
            self.reprocess_raw_records_with(Some(&streams), options)?
        };
        // Local replay has its own timeline. It never rewrites the cloud sync time: "is my data
        // fresh" and "when did you last reach the cloud" are two different questions.
        self.record_local_replay(false)?;
        Ok(Some(counts))
    }

    pub fn reprocess_raw_records(&self) -> Result<BTreeMap<String, i64>> {
        self.reprocess_raw_records_for_stream(None)
    }

    /// Replays `raw_records`. When `stream_filter` is `None`, it replays all of them.
    ///
    /// The filter is a set of stream names, not one name: a revision often changes the normalization
    /// rules of more than one stream (v18 changed workouts and sleep together). With only one name, the second stream is
    /// either missed or forces a fall back to a full replay.
    pub(super) fn reprocess_raw_records_for_stream(
        &self,
        stream_filter: Option<&[&str]>,
    ) -> Result<BTreeMap<String, i64>> {
        self.reprocess_raw_records_with(stream_filter, ReplayOptions::default())
    }

    fn reprocess_raw_records_with(
        &self,
        stream_filter: Option<&[&str]>,
        options: ReplayOptions<'_>,
    ) -> Result<BTreeMap<String, i64>> {
        let _replay_guard = ReplayGuard::enter();
        // When only processing payloads without a stamp, skip those already stamped with this revision.
        let stamp_filter = if options.unstamped_only {
            " AND NOT EXISTS (SELECT 1 FROM raw_normalization n
                              WHERE n.raw_record_id = r.id AND n.revision = ?1)"
        } else {
            ""
        };
        // First fetch only the id and the two short fields that normalization needs. Read each payload one by one in the loop.
        // Before, the payload went into the Vec too. That loaded every payload in the database
        // into memory before the replay started, and this code must run on NAS boxes and memory-limited containers with an 842 MB
        // database.
        let plan: Vec<(i64, String, String)> = if let Some(streams) = stream_filter {
            // The number of parameters follows the number of streams. A hand-built IN list is the place where this kind of code most often leaves a SQL
            // injection hole, even though all values here are compile-time constants.
            // ?1 is the current revision: raw rows already quarantined by this revision do not enter the plan. Otherwise every
            // startup scans them again, and since they cannot get a stamp, they replay forever.
            let placeholders = (2..=streams.len() + 1)
                .map(|index| format!("?{index}"))
                .collect::<Vec<_>>()
                .join(", ");
            let mut stmt = self.conn.prepare(&format!(
                "SELECT r.id, r.stream, r.source_key
                 FROM raw_records r
                 WHERE NOT EXISTS (
                     SELECT 1 FROM raw_quarantine q
                     WHERE q.raw_record_id = r.id AND q.revision = ?1
                 )
                   AND r.stream IN ({placeholders}){stamp_filter}
                 ORDER BY r.fetched_at, r.id"
            ))?;
            let mut bind: Vec<&str> = Vec::with_capacity(streams.len() + 1);
            bind.push(NORMALIZER_REVISION);
            bind.extend(streams.iter().copied());
            let rows = stmt.query_map(rusqlite::params_from_iter(bind.iter()), |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?;
            rows.collect::<std::result::Result<Vec<_>, _>>()?
        } else {
            // Replay in the order of the last fetch from the cloud: when one day has values in several payloads, the last one written
            // wins, and it must be the most recently fetched. Ordered by id, a payload inserted long ago and later updated in place
            // would come first and be overwritten by an older snapshot (it heals only on the next sync).
            let mut stmt = self.conn.prepare(&format!(
                "SELECT r.id, r.stream, r.source_key
                 FROM raw_records r
                 WHERE NOT EXISTS (
                     SELECT 1 FROM raw_quarantine q
                     WHERE q.raw_record_id = r.id AND q.revision = ?1
                 ){stamp_filter}
                 ORDER BY r.fetched_at, r.id"
            ))?;
            let rows = stmt.query_map(params![NORMALIZER_REVISION], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?;
            rows.collect::<std::result::Result<Vec<_>, _>>()?
        };

        let mut counts = BTreeMap::<String, i64>::new();
        let mut band_heart_rate = 0i64;
        let mut failures = 0i64;
        // One transaction per batch. Batch boundaries fall between payloads, so "delete the derived rows of this payload,
        // then insert again with the new rules" always stays in one transaction. A failure halfway never leaves a record that was cleared
        // but not rebuilt. A single decode / parse / normalization failure goes to the quarantine table and the loop continues.
        // It must not abort the whole run with `?`. The quarantine INSERT commits together with the derived rows of this batch.
        for batch in plan.chunks(REPLAY_BATCH_RECORDS) {
            // Stop when the exit flag is raised: all transactions before this batch are committed, the revision does not
            // advance, and the next startup resumes from the break point. Return Ok, not Err: this is not
            // a failure and must not be logged as "replay failed".
            if background_write_abort_requested() {
                return Ok(counts);
            }
            // Lock per batch: if the lock is not available, someone else is writing (CLI reparse, restore, backup). This run stops here,
            // the revision does not advance, and the next startup resumes from the break point. Same handling as an exit request.
            let batch_guard = match options.batch_lock {
                Some(dir) => match write_lock::acquire_with_timeout(
                    dir,
                    write_lock::WritePurpose::Reprocess,
                    std::time::Duration::from_secs(30),
                ) {
                    Ok(guard) => Some(guard),
                    Err(error) => {
                        tracing::warn!("Replay gave up the write lock and could not take it back, will continue at next startup: {error}");
                        return Ok(counts);
                    }
                },
                None => None,
            };
            let transaction = ReplayBatch::begin(&self.conn)?;
            for (id, stream, source_key) in batch {
                // The payload may have been cleaned up after this replay started. Skip it, it is not an error.
                let Some((stored_payload, payload_zip)) = self.raw_payload(*id)? else {
                    continue;
                };
                let encoded_payload = match decode_raw_payload(stored_payload, payload_zip) {
                    Ok(value) => value,
                    Err(error) => {
                        failures += 1;
                        self.insert_raw_quarantine(*id, stream, source_key, &error, None)?;
                        continue;
                    }
                };
                let payload: serde_json::Value = match serde_json::from_str(&encoded_payload) {
                    Ok(value) => value,
                    Err(error) => {
                        failures += 1;
                        self.insert_raw_quarantine(
                            *id,
                            stream,
                            source_key,
                            &ZeppBridgeError::ParseError(error.to_string()),
                            None,
                        )?;
                        continue;
                    }
                };
                // `normalize_and_persist_raw` first clears the old derived rows of this payload, then inserts them again one by one
                // with the new rules. That can be several INSERTs. It is not one
                // statement, so if it fails halfway, the clears/inserts already run must not stay in this batch
                // transaction and commit with it. Otherwise the result is "deleted but not fully rebuilt". An inner
                // savepoint isolates the writes of this one payload. On failure, only this payload rolls back,
                // and the other payloads already processed in this batch are not affected.
                self.conn.execute("SAVEPOINT reprocess_record", [])?;
                match self.normalize_and_persist_raw(*id, stream, source_key, &payload) {
                    Ok(result) => {
                        self.conn.execute("RELEASE reprocess_record", [])?;
                        self.clear_raw_quarantine(*id)?;
                        *counts.entry(stream.clone()).or_default() += result.primary_records;
                        band_heart_rate += result.band_heart_rate_records;
                    }
                    Err(error) => {
                        self.conn.execute("ROLLBACK TO reprocess_record", [])?;
                        self.conn.execute("RELEASE reprocess_record", [])?;
                        failures += 1;
                        self.insert_raw_quarantine(
                            *id,
                            stream,
                            source_key,
                            &error,
                            Some(&payload),
                        )?;
                    }
                }
            }
            transaction.commit()?;
            if let Some(guard) = batch_guard {
                drop(guard);
                // After releasing, wait a little: waiters check only every 120ms. Without this pause, the next batch
                // always gets the lock first.
                std::thread::sleep(std::time::Duration::from_millis(150));
            }
        }
        if band_heart_rate > 0 {
            counts.insert("heart_rate".to_string(), band_heart_rate);
        }

        // For streams with their own table, report the current total in the database (more useful than this run's delta).
        // Streams without their own table keep the count measured in this run: `wellness` output goes into
        // daily_metrics and metric_samples. Asking "how many rows does the wellness table have" only
        // gives 0, and reporting 0 to the user says this stream decoded nothing.
        for stream in counts.keys().cloned().collect::<Vec<_>>() {
            if let Some(total) = self.normalized_stream_count(&stream)? {
                counts.insert(stream.clone(), total);
            }
        }

        self.set_app_meta(REPLAY_LAST_FAILURES_KEY, &failures.to_string())?;
        // After one full pass, advance the revision. Failures are already in the quarantine table for this revision.
        //
        // Before, it did not advance when there were new failures. But the same rules give the same result on a second run: the second pass
        // skips the quarantined rows and must have 0 failures, so every revision bump replayed the full database twice
        // (the 3.0.3 log shows one pass at 21:46 and another at 22:00). Most "failures" are the cloud
        // returning empty data (for example, empty heart rate items for a day), and they stay empty no matter how often they replay.
        // An early stop (exit, giving up the write lock) already returned above and does not reach this point.
        // An empty database with 0 rows also gets the stamp, so the first sync does not cause a needless replay.
        self.conn.execute(
            "INSERT INTO app_meta(key, value, updated_at)
             VALUES('normalizer_revision', ?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
            params![NORMALIZER_REVISION, Utc::now().to_rfc3339()],
        )?;
        self.set_app_meta(LAST_LOCAL_REPROCESS_AT_KEY, &Utc::now().to_rfc3339())?;
        Ok(counts)
    }

    /// The stored form of a single raw payload (may be compressed).
    pub(super) fn raw_payload(
        &self,
        raw_record_id: i64,
    ) -> Result<Option<(String, Option<Vec<u8>>)>> {
        self.conn
            .query_row(
                "SELECT payload, payload_zip FROM raw_records WHERE id = ?1",
                [raw_record_id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<Vec<u8>>>(1)?)),
            )
            .optional()
            .map_err(Into::into)
    }

    /// `None` = this stream has no canonical table of its own, so it has no total that belongs only to it.
    pub(super) fn normalized_stream_count(&self, stream: &str) -> Result<Option<i64>> {
        let (query, parameter): (&str, Option<&str>) = match stream {
            "heart_rate" => (
                "SELECT COUNT(*) FROM metric_samples WHERE metric = ?1",
                Some("heart_rate"),
            ),
            "hrv" => (
                "SELECT COUNT(*) FROM metric_samples WHERE metric = ?1",
                Some("hrv"),
            ),
            // Count by stream, not by whole table: wellness also writes daily rows into daily_metrics,
            // and the v20 upgrade replays both streams. A whole-table COUNT(*) would add wellness
            // rows to the daily_summary count, and the two numbers the user sees would add up to more than what was really
            // written.
            "daily_summary" => (
                "SELECT COUNT(*) FROM daily_metrics d
                   JOIN raw_records r ON r.id = d.raw_record_id
                  WHERE r.stream = 'daily_summary'",
                None,
            ),
            "sleep" => ("SELECT COUNT(*) FROM sleep_sessions", None),
            "workouts" => ("SELECT COUNT(*) FROM workouts", None),
            "workout_detail" => ("SELECT COUNT(*) FROM workout_samples", None),
            // wellness has no table of its own. It writes into daily_metrics and metric_samples.
            // Without counting it, an upgrade that replays only wellness reports "0 rows" to the user, but
            // this stream is the whole point of that upgrade.
            "wellness" => (
                "SELECT (SELECT COUNT(*) FROM metric_samples s
                           JOIN raw_records r ON r.id = s.raw_record_id
                          WHERE r.stream = 'wellness')
                      + (SELECT COUNT(*) FROM daily_metrics d
                           JOIN raw_records r ON r.id = d.raw_record_id
                          WHERE r.stream = 'wellness')",
                None,
            ),
            // Weight also has no table of its own. It writes into metric_samples. Without counting it, an upgrade
            // that replays only weight reports "0 rows" to the user, but the whole point of that upgrade
            // is this stream.
            "weight" => (
                "SELECT COUNT(*) FROM metric_samples s
                   JOIN raw_records r ON r.id = s.raw_record_id
                  WHERE r.stream = 'weight'",
                None,
            ),
            _ => return Ok(None),
        };
        let total = if let Some(parameter) = parameter {
            self.conn.query_row(query, [parameter], |row| row.get(0))?
        } else {
            self.conn.query_row(query, [], |row| row.get(0))?
        };
        Ok(Some(total))
    }
}
