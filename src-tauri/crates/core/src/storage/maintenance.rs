//! Data status, retention cleanup, raw payload compression and quarantine (split out of storage/mod.rs by domain, logic unchanged).

use super::*;

impl Database {
    pub fn list_data_status(&self) -> Result<Vec<DataStatus>> {
        let mut stmt = self.conn.prepare(
            "SELECT stream, status, last_sync, records_written, capability,
                    needs_reauth, message FROM sync_state ORDER BY stream",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, Option<String>>(6)?,
            ))
        })?;
        let mut statuses = Vec::new();
        for row in rows {
            let (stream, status, last_sync, records_written, capability, needs_reauth, message) =
                row?;
            statuses.push(DataStatus {
                stream,
                status,
                last_sync: last_sync
                    .as_deref()
                    .map(|value| parse_datetime(value, "sync_state.last_sync"))
                    .transpose()?,
                records_written,
                capability,
                needs_reauth: needs_reauth != 0,
                message,
            });
        }
        Ok(statuses)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update_sync_state_details(
        &self,
        stream: &str,
        cursor: Option<&str>,
        status: &str,
        error: Option<&str>,
        needs_reauth: bool,
        records_written: i64,
        capability: CapabilityStatus,
        message: Option<String>,
    ) -> Result<()> {
        if self.sync_state_is_quiet() {
            return Ok(());
        }
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO sync_state
                (stream, last_sync, cursor, status, error, needs_reauth,
                 records_written, capability, message, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?2)
             ON CONFLICT(stream) DO UPDATE SET
                last_sync = excluded.last_sync,
                cursor = excluded.cursor,
                status = excluded.status,
                error = excluded.error,
                needs_reauth = excluded.needs_reauth,
                records_written = excluded.records_written,
                capability = excluded.capability,
                message = excluded.message,
                updated_at = excluded.updated_at",
            params![
                stream,
                now,
                cursor,
                status,
                error,
                if needs_reauth { 1 } else { 0 },
                records_written,
                capability.as_str(),
                message,
            ],
        )?;
        Ok(())
    }

    pub fn get_sync_state(&self, stream: &str) -> Result<Option<SyncStateInfo>> {
        let row = self
            .conn
            .query_row(
                "SELECT stream, last_sync, cursor, status, error, needs_reauth,
                        records_written, capability, message, updated_at
                 FROM sync_state WHERE stream = ?1",
                [stream],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, Option<String>>(4)?,
                        row.get::<_, i64>(5)?,
                        row.get::<_, i64>(6)?,
                        row.get::<_, String>(7)?,
                        row.get::<_, Option<String>>(8)?,
                        row.get::<_, String>(9)?,
                    ))
                },
            )
            .optional()?;
        row.map(
            |(
                stream,
                last_sync,
                cursor,
                status,
                error,
                needs_reauth,
                records_written,
                capability,
                message,
                updated_at,
            )| {
                Ok(SyncStateInfo {
                    stream,
                    last_sync: last_sync
                        .as_deref()
                        .map(|value| parse_datetime(value, "sync_state.last_sync"))
                        .transpose()?,
                    cursor,
                    status,
                    error,
                    needs_reauth: needs_reauth != 0,
                    records_written,
                    capability,
                    message,
                    updated_at: parse_datetime(&updated_at, "sync_state.updated_at")?,
                })
            },
        )
        .transpose()
    }

    /// Compresses historical payloads that are not yet compressed, and returns the byte counts before and after.
    ///
    /// First it converts the full-window daily event payloads saved before 3.0 into per-day payloads (see `event_windows`),
    /// which are the largest part of old databases. Then it compresses the plain-text payloads not yet compressed.
    ///
    /// New payloads are compressed when they enter the database. This method only handles the backlog saved **before this version**.
    /// It is a one-time maintenance action and does not run on the sync path: an old database can hold thousands of payloads
    /// and gigabytes of data. One pass reads and writes all of it, and a normal sync must not do this on the side.
    ///
    /// Safety rule: **decompress and compare first, and store only when it is identical**. Raw payloads are the only
    /// source for replay. A corrupt compression permanently loses that payload, so it is better not to compress it.
    pub fn compact_raw_payloads(&self) -> Result<RawPayloadCompaction> {
        let _guard = CompactionGuard::enter();
        let mut report = RawPayloadCompaction::default();
        self.consolidate_event_windows(&mut report)?;
        let (pruned, pruned_bytes) = self.prune_superseded_page_raws()?;
        report.compacted += pruned;
        report.bytes_before += pruned_bytes;
        let mut last_id: i64 = 0;
        loop {
            // Same exit signal as replay: stop at a batch boundary and give back the write lock quickly.
            if background_write_abort_requested() {
                break;
            }
            let pending: Vec<(i64, String)> = {
                let mut stmt = self.conn.prepare(
                    "SELECT id, payload FROM raw_records
                     WHERE id > ?1
                       AND (payload_zip IS NULL OR LENGTH(payload_zip) = 0)
                       AND LENGTH(payload) > ?2
                     ORDER BY id
                     LIMIT ?3",
                )?;
                let rows = stmt.query_map(
                    params![
                        last_id,
                        MIN_COMPRESSIBLE_PAYLOAD_BYTES,
                        COMPACTION_BATCH_RECORDS as i64
                    ],
                    |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
                )?;
                rows.collect::<std::result::Result<Vec<_>, _>>()?
            };
            if pending.is_empty() {
                break;
            }
            last_id = pending.last().map(|(id, _)| *id).unwrap_or(last_id);
            let transaction = self.conn.unchecked_transaction()?;
            for (id, payload) in pending {
                let original = payload.len() as u64;
                let Ok(compressed) = compress_payload(&payload) else {
                    report.skipped += 1;
                    continue;
                };
                // If compression does not make it smaller, skip it and take no risk.
                if compressed.len() as u64 >= original {
                    report.skipped += 1;
                    continue;
                }
                match decompress_payload(&compressed) {
                    Ok(round_tripped) if round_tripped == payload => {}
                    _ => {
                        report.skipped += 1;
                        continue;
                    }
                }
                transaction.execute(
                    "UPDATE raw_records SET payload = '', payload_zip = ?2 WHERE id = ?1",
                    params![id, compressed],
                )?;
                report.compacted += 1;
                report.bytes_before += original;
                report.bytes_after += compressed.len() as u64;
            }
            transaction.commit()?;
        }

        // Compression frees pages **inside the database**: without VACUUM, the file on disk
        // does not shrink by a single byte, and the user sees no change. VACUUM rebuilds the whole file and
        // needs about as much temporary space again, so only run it when something was compressed. A failure does not
        // fail the whole job: the data is already compressed, only the file did not shrink.
        // Also skip it on an exit stop: on a large database VACUUM is the longest step, and the only goal now
        // is to let the process finish quickly.
        if report.compacted > 0 && !background_write_abort_requested() {
            if let Err(error) = self.conn.execute_batch("VACUUM") {
                tracing::warn!("VACUUM after compression failed, disk usage will not go down for now: {error}");
            }
            let _ = self.bump_payload_stats_generation();
            let _ = self.persist_raw_payload_stats();
        }
        Ok(report)
    }

    /// How many historical payloads are **worth compressing**. Used to decide whether to run one pass in the background at startup.
    ///
    /// The threshold must be the same as in [`compact_raw_payloads`]. Otherwise this loop occurs:
    /// the count says "10 left to compress" -> the UI shows "compressing" -> the compress function finds these 10
    /// get larger when compressed and skips them all -> the next startup counts the same 10 as pending again.
    /// This really happened: ten `{"items":[]}` rows (12-byte empty responses) made the banner flash at every
    /// startup.
    pub fn pending_raw_payload_count(&self) -> Result<i64> {
        let uncompressed: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM raw_records
             WHERE (payload_zip IS NULL OR LENGTH(payload_zip) = 0)
               AND LENGTH(payload) > ?1",
            [MIN_COMPRESSIBLE_PAYLOAD_BYTES],
            |row| row.get(0),
        )?;
        Ok(uncompressed + self.pending_event_window_count()?)
    }

    /// Deletes superseded heart rate page / workout history payloads: no derived row points to them and they are not quarantined.
    ///
    /// The keys of these two streams used to include the timestamp of each sync (`heart_rate_page:<start>:<end>`,
    /// `sport_history:<kind>:<start>:<cursor>`). Hundreds were added each day and never cleaned up: in a real database
    /// none of 896 heart rate pages had a sample pointing to them, and 777 of 786 workout history payloads had no workout pointing to them.
    /// The new keys are stable (heart rate by UTC day, workout history by content). After new payloads take over
    /// their rows, the old ones are only a load on replay.
    ///
    /// It only checks "does any derived row point to it": quarantined (failed to parse) payloads stay, because a replay after a parser fix
    /// still needs them. Payloads with rows pointing to them stay, because they are the only source of those rows. Runs after every sync (not affected by
    /// the archive switch, since it deletes nobody's data), and also during startup compression. Returns (count, bytes).
    pub fn prune_superseded_page_raws(&self) -> Result<(u64, u64)> {
        const BATCH: i64 = 500;
        let mut deleted = 0u64;
        let mut bytes = 0u64;
        loop {
            if background_write_abort_requested() {
                break;
            }
            let ids: Vec<(i64, i64)> = {
                let mut stmt = self.conn.prepare(
                    "SELECT r.id, LENGTH(r.payload) + COALESCE(LENGTH(r.payload_zip), 0)
                     FROM raw_records r
                     WHERE ((r.stream = 'heart_rate' AND r.source_key GLOB 'heart_rate_page:*')
                         OR (r.stream = 'workouts' AND r.source_key GLOB 'sport_history:*'))
                       AND NOT EXISTS (SELECT 1 FROM raw_quarantine q WHERE q.raw_record_id = r.id)
                       AND NOT EXISTS (SELECT 1 FROM metric_samples m WHERE m.raw_record_id = r.id)
                       AND NOT EXISTS (SELECT 1 FROM daily_metrics d WHERE d.raw_record_id = r.id)
                       AND NOT EXISTS (SELECT 1 FROM sleep_sessions s WHERE s.raw_record_id = r.id)
                       AND NOT EXISTS (SELECT 1 FROM workouts w WHERE w.raw_record_id = r.id)
                     ORDER BY r.id
                     LIMIT ?1",
                )?;
                let rows = stmt.query_map([BATCH], |row| Ok((row.get(0)?, row.get(1)?)))?;
                rows.collect::<std::result::Result<Vec<_>, _>>()?
            };
            if ids.is_empty() {
                break;
            }
            let transaction = self.conn.unchecked_transaction()?;
            for (id, size) in &ids {
                deleted +=
                    transaction.execute("DELETE FROM raw_records WHERE id = ?1", [id])? as u64;
                bytes += (*size).max(0) as u64;
            }
            transaction.commit()?;
            if (ids.len() as i64) < BATCH {
                break;
            }
        }
        Ok((deleted, bytes))
    }

    pub fn cleanup_old_data(&self, days: i64) -> Result<()> {
        if !(1..=365).contains(&days) {
            return Err(ZeppBridgeError::ConfigError(
                "retention days must be in 1..=365".into(),
            ));
        }
        let cutoff_timestamp = (Utc::now() - Duration::days(days)).to_rfc3339();
        // daily_metrics.date is the device local calendar day. Never compare it with the UTC "today".
        let today = crate::contract::health_today(self.device_time_zone()?.as_deref(), Utc::now());
        let cutoff_date = (today - Duration::days(days))
            .format("%Y-%m-%d")
            .to_string();
        let changes_before = self.conn.total_changes();
        self.conn.execute_batch("BEGIN IMMEDIATE;")?;
        let deleted = (|| -> Result<()> {
            self.conn.execute(
                "DELETE FROM metric_samples WHERE timestamp < ?1 AND import_batch_id IS NULL",
                [&cutoff_timestamp],
            )?;
            self.conn.execute(
                "DELETE FROM daily_metrics WHERE date < ?1 AND import_batch_id IS NULL",
                [&cutoff_date],
            )?;
            self.conn.execute(
                "DELETE FROM sleep_sessions WHERE start_time < ?1 AND import_batch_id IS NULL",
                [&cutoff_timestamp],
            )?;
            self.conn.execute(
                "DELETE FROM workout_hr_zones
                 WHERE workout_id IN (SELECT workout_id FROM workouts WHERE start_time < ?1) AND workout_id NOT IN (SELECT workout_id FROM workout_imports)",
                [&cutoff_timestamp],
            )?;
            self.conn.execute(
                "DELETE FROM workout_strength_sets
                 WHERE workout_id IN (SELECT workout_id FROM workouts WHERE start_time < ?1) AND workout_id NOT IN (SELECT workout_id FROM workout_imports)",
                [&cutoff_timestamp],
            )?;
            self.conn.execute(
                "DELETE FROM workouts WHERE start_time < ?1 AND workout_id NOT IN (SELECT workout_id FROM workout_imports)",
                [&cutoff_timestamp],
            )?;
            self.conn.execute(
                "DELETE FROM workout_samples WHERE timestamp < ?1 AND workout_id NOT IN (SELECT workout_id FROM workout_imports)",
                [&cutoff_timestamp],
            )?;
            self.conn.execute(
                "DELETE FROM route_points WHERE timestamp < ?1 AND workout_id NOT IN (SELECT workout_id FROM workout_imports)",
                [&cutoff_timestamp],
            )?;
            self.conn.execute(
                "DELETE FROM workout_pauses WHERE start_time < ?1 AND workout_id NOT IN (SELECT workout_id FROM workout_imports)",
                [&cutoff_timestamp],
            )?;
            self.conn.execute(
                "DELETE FROM workout_laps WHERE start_time < ?1 AND workout_id NOT IN (SELECT workout_id FROM workout_imports)",
                [&cutoff_timestamp],
            )?;
            self.conn.execute(
                "DELETE FROM workout_splits WHERE start_time < ?1 AND workout_id NOT IN (SELECT workout_id FROM workout_imports)",
                [&cutoff_timestamp],
            )?;
            // Raw responses are retained from their fetch time, not their query
            // window start. A 30-day request naturally starts near the retention
            // cutoff and must not be deleted seconds after it is fetched.
            // A payload expires by either of two times. Delete it when either applies and no derived row points to it:
            // - fetched before the retention period (the old rule);
            // - the time range the payload covers is **entirely** before the retention period. With only the fetch time, payloads from several years ago
            //   that were backfilled recently stay after their derived rows are deleted. The next revision replay brings the deleted history
            //   back exactly, and the next sync deletes it again. A payload whose window crosses the retention boundary stays, because it still owns rows inside the period.
            self.conn.execute(
                "DELETE FROM raw_records
                 WHERE (fetched_at < ?1 OR COALESCE(end_utc, start_utc) < ?1)
                   AND NOT EXISTS (SELECT 1 FROM metric_samples m WHERE m.raw_record_id = raw_records.id)
                   AND NOT EXISTS (SELECT 1 FROM daily_metrics d WHERE d.raw_record_id = raw_records.id)
                   AND NOT EXISTS (SELECT 1 FROM sleep_sessions s WHERE s.raw_record_id = raw_records.id)
                   AND NOT EXISTS (SELECT 1 FROM workouts w WHERE w.raw_record_id = raw_records.id)",
                [&cutoff_timestamp],
            )?;
            // The coverage ledger changes too: a month that was cleared (even in part) can no longer say "persisted". Before, the ledger
            // stayed persisted, so backfill skipped these months as done, and the "complete local copy" was false.
            // Months the cloud never had (empty_from_cloud) lost nothing, so leave them.
            self.conn.execute(
                "UPDATE coverage_ledger
                 SET status = 'pending', records = 0, persisted_at = NULL, error = NULL,
                     updated_at = ?2
                 WHERE status IN ('persisted', 'partial') AND chunk_start < ?1",
                params![cutoff_date, Utc::now().to_rfc3339()],
            )?;
            Ok(())
        })();
        match deleted {
            Ok(()) => {
                if self.conn.is_autocommit() {
                    return Err(ZeppBridgeError::DataUnavailable(
                        "Cleanup was interrupted. No changes were written.".into(),
                    ));
                }
                self.conn.execute_batch("COMMIT;")?;
                if self.conn.total_changes() != changes_before {
                    note_content_change();
                }
            }
            Err(error) => {
                if !self.conn.is_autocommit() {
                    let _ = self.conn.execute_batch("ROLLBACK;");
                }
                return Err(error);
            }
        }
        if let Err(error) = self
            .conn
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE); PRAGMA incremental_vacuum;")
        {
            tracing::warn!("WAL checkpoint after cleanup failed, the data is already deleted: {error}");
        }
        let _ = self.bump_payload_stats_generation();
        let _ = self.persist_raw_payload_stats();
        Ok(())
    }

    /// `payload` is this payload itself (`None` when it cannot be decoded). When the cloud explicitly returned an empty collection, record it as
    /// `empty_response`, and data health reports it separately from "cannot understand" (#143). The rule is the same as the one sync
    /// uses to decide "the cloud has nothing" (`sync/persist.rs`).
    pub(super) fn insert_raw_quarantine(
        &self,
        raw_record_id: i64,
        stream: &str,
        source_key: &str,
        error: &ZeppBridgeError,
        payload: Option<&serde_json::Value>,
    ) -> Result<()> {
        let empty_response = error.is_unavailable()
            && payload.is_some_and(crate::normalizer::payload_is_explicitly_empty);
        self.conn.execute(
            "INSERT INTO raw_quarantine(
                 raw_record_id, stream, source_key, error, revision, quarantined_at, empty_response
             )
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(raw_record_id) DO UPDATE SET
                stream = excluded.stream,
                source_key = excluded.source_key,
                error = excluded.error,
                revision = excluded.revision,
                quarantined_at = excluded.quarantined_at,
                empty_response = excluded.empty_response",
            params![
                raw_record_id,
                stream,
                source_key,
                error.to_string(),
                NORMALIZER_REVISION,
                Utc::now().to_rfc3339(),
                empty_response,
            ],
        )?;
        Ok(())
    }

    pub(super) fn clear_raw_quarantine(&self, raw_record_id: i64) -> Result<()> {
        self.conn.execute(
            "DELETE FROM raw_quarantine WHERE raw_record_id = ?1",
            [raw_record_id],
        )?;
        Ok(())
    }

    /// Commit raw first, then normalize in a separate transaction.
    ///
    /// A normalization failure rolls back the derived rows and writes raw to the quarantine table, but a payload already fetched from the cloud must stay
    /// in `raw_records`. Before, both were in one transaction, and a failure did a ROLLBACK of the raw row too.
    pub fn persist_fetched_record(&self, record: &RawRecord) -> Result<(i64, NormalizationCounts)> {
        let serialized = SerializedPayload::of(record)?;
        if let Some((raw_id, records_written)) = self.touch_unchanged_raw(record, &serialized)? {
            return Ok((
                raw_id,
                NormalizationCounts {
                    primary_records: records_written,
                    ..NormalizationCounts::default()
                },
            ));
        }
        // The cloud again returned "nothing" in the same form: the conclusion stays. Do not store it again or quarantine it again.
        if let Some(detail) = self.touch_unchanged_empty_raw(record, &serialized)? {
            return Err(ZeppBridgeError::DataUnavailable(detail));
        }
        let raw_id = self.insert_serialized_raw(record, &serialized, &Utc::now().to_rfc3339())?;
        let normalized = (|| {
            let transaction = ReplayBatch::begin(&self.conn)?;
            let counts = self.normalize_and_persist_raw(
                raw_id,
                &record.stream,
                &record.source_key,
                &record.payload,
            )?;
            self.clear_raw_quarantine(raw_id)?;
            transaction.commit()?;
            Ok(counts)
        })();
        match normalized {
            Ok(counts) => Ok((raw_id, counts)),
            Err(error) => {
                let _ = self.insert_raw_quarantine(
                    raw_id,
                    &record.stream,
                    &record.source_key,
                    &error,
                    Some(&record.payload),
                );
                Err(error)
            }
        }
    }

    #[cfg(test)]
    pub fn count_metric_samples(&self) -> Result<i64> {
        self.conn
            .query_row("SELECT COUNT(*) FROM metric_samples", [], |row| row.get(0))
            .map_err(Into::into)
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub fn count_raw_records(&self) -> Result<i64> {
        self.conn
            .query_row("SELECT COUNT(*) FROM raw_records", [], |row| row.get(0))
            .map_err(Into::into)
    }
}
