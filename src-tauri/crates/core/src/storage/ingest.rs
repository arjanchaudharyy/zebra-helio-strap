//! Store raw payloads and persist normalized rows (split from storage/mod.rs by domain, same logic).

use super::*;

/// A serialized payload and its checksum.
///
/// The checksum is always over the **uncompressed** JSON. Compression is a storage detail. It must not change the identity
/// of "what this payload is".
pub(super) struct SerializedPayload {
    pub(super) json: String,
    pub(super) hash: String,
}

impl SerializedPayload {
    pub(super) fn of(record: &RawRecord) -> Result<Self> {
        let json = serde_json::to_string(&record.payload)
            .map_err(|error| ZeppBridgeError::ParseError(error.to_string()))?;
        let mut hasher = Sha256::new();
        hasher.update(json.as_bytes());
        let hash = hex::encode(hasher.finalize());
        Ok(Self { json, hash })
    }
}

impl Database {
    /// Stored identity aliases link a historical workout to its own device.
    /// A user-assigned device model wins over an old identity display name.
    pub(super) fn workout_device_catalog_id(&self, device_id: &str) -> Result<Option<String>> {
        let profile = self.lookup_device_profile(device_id)?;
        let mut keys = vec![device_id];
        if let Some(profile) = profile.as_ref() {
            keys.extend(profile.device_id.as_deref());
            keys.extend(profile.serial.as_deref());
        }
        if let Some(assigned) = self.device_model_override(&keys)? {
            return Ok(Some(assigned.catalog_id));
        }
        Ok(profile.and_then(|profile| {
            crate::device_catalog::match_catalog(&crate::device_catalog::CatalogMatchInput {
                device_names: profile.name.as_deref().into_iter().collect(),
                ..Default::default()
            })
            .map(|matched| matched.entry.catalog_id.clone())
        }))
    }

    pub(super) fn ensure_table_columns(&self, table: &str, columns: &[(&str, &str)]) -> Result<()> {
        let mut stmt = self.conn.prepare(&format!("PRAGMA table_info({table})"))?;
        let existing = stmt
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        for (name, definition) in columns {
            if !existing.iter().any(|value| value == name) {
                self.conn.execute(
                    &format!("ALTER TABLE {table} ADD COLUMN {name} {definition}"),
                    [],
                )?;
            }
        }
        Ok(())
    }

    pub fn insert_raw_record(&self, record: &RawRecord) -> Result<i64> {
        let serialized = SerializedPayload::of(record)?;
        self.insert_serialized_raw(record, &serialized, &Utc::now().to_rfc3339())
    }

    /// `fetched_at` is the time this payload was **received from the cloud**. Usually that is now. When old payloads
    /// are regrouped into daily payloads, keep the time of the original. A local regroup must not pose as a cloud fetch.
    pub(super) fn insert_serialized_raw(
        &self,
        record: &RawRecord,
        serialized: &SerializedPayload,
        fetched_at: &str,
    ) -> Result<i64> {
        let SerializedPayload {
            json: payload,
            hash: payload_hash,
        } = serialized;
        // Content is identical: refresh only the metadata and `fetched_at`. Do not compress or rewrite the blob. SET must not
        // contain `payload_hash`: the `invalidate_raw_normalization` trigger fires on "UPDATE OF
        // payload_hash", and even with the same value it deletes the normalization and quarantine records.
        let refreshed = self
            .conn
            .prepare_cached(
                "UPDATE raw_records SET
                    source_scope = ?3, device_id = ?4, start_utc = ?5, end_utc = ?6,
                    fetched_at = ?7
                 WHERE stream = ?1 AND source_key = ?2 AND payload_hash = ?8",
            )?
            .execute(params![
                record.stream,
                record.source_key,
                record.source_scope.as_str(),
                record.device_id,
                record.start_utc.to_rfc3339(),
                record.end_utc.map(|value| value.to_rfc3339()),
                fetched_at,
                payload_hash,
            ])?;
        if refreshed > 0 {
            return self
                .conn
                .query_row(
                    "SELECT id FROM raw_records WHERE stream = ?1 AND source_key = ?2",
                    params![record.stream, record.source_key],
                    |row| row.get(0),
                )
                .map_err(Into::into);
        }
        let payload_zip = compress_payload(payload)?;
        self.conn.execute(
            "INSERT INTO raw_records
                (stream, source_key, source_scope, device_id, start_utc, end_utc,
                 payload, payload_zip, payload_hash, fetched_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, '', ?7, ?8, ?9)
             ON CONFLICT(stream, source_key) DO UPDATE SET
                source_scope = excluded.source_scope,
                device_id = excluded.device_id,
                start_utc = excluded.start_utc,
                end_utc = excluded.end_utc,
                payload = '',
                payload_zip = excluded.payload_zip,
                payload_hash = excluded.payload_hash,
                fetched_at = excluded.fetched_at",
            params![
                record.stream,
                record.source_key,
                record.source_scope.as_str(),
                record.device_id,
                record.start_utc.to_rfc3339(),
                record.end_utc.map(|value| value.to_rfc3339()),
                payload_zip,
                payload_hash,
                fetched_at,
            ],
        )?;
        self.conn
            .query_row(
                "SELECT id FROM raw_records WHERE stream = ?1 AND source_key = ?2",
                params![record.stream, record.source_key],
                |row| row.get(0),
            )
            .map_err(Into::into)
    }

    /// The same payload was fetched again: identical content, already normalized by the current parser, and not quarantined.
    ///
    /// Rewriting the blob, or deleting and reinserting the derived rows, would be wasted work. Auto sync fetches 30 days again every 15 minutes,
    /// and apart from today the other 29 days almost always come back unchanged. Only set `fetched_at` to now: the fact
    /// "last received from the cloud" must still be correct.
    ///
    /// Returns the primary count from the last normalization of this payload, so "wrote N rows" in the report
    /// does not drop to 0 because of the skip.
    ///
    /// `sleep` did not use this path before: its primary count (sleep sessions) is not the same as the total rows in
    /// `raw_normalization` (which also has band heart rate and extra daily metrics). The cost was that every sync deleted and reinserted
    /// 30 days of `band_data`. On a copy of a real library, one run was about 270,000 band heart rate rows (user report:
    /// "every sync rewrites about 27,000 rows"). Now the sleep session count comes directly from `raw_record_id`,
    /// and it is the same number that a new normalization reports as the primary count.
    pub(super) fn touch_unchanged_raw(
        &self,
        record: &RawRecord,
        serialized: &SerializedPayload,
    ) -> Result<Option<(i64, i64)>> {
        let unchanged = self
            .conn
            .prepare_cached(
                "SELECT r.id,
                        CASE WHEN r.stream = 'sleep'
                             THEN (SELECT COUNT(*) FROM sleep_sessions s WHERE s.raw_record_id = r.id)
                             ELSE n.records_written END
                 FROM raw_records r
                 JOIN raw_normalization n
                   ON n.raw_record_id = r.id AND n.revision = ?4
                 WHERE r.stream = ?1 AND r.source_key = ?2 AND r.payload_hash = ?3
                   AND NOT EXISTS (SELECT 1 FROM raw_quarantine q WHERE q.raw_record_id = r.id)",
            )?
            .query_row(
                params![
                    record.stream,
                    record.source_key,
                    serialized.hash,
                    NORMALIZER_REVISION
                ],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
            )
            .optional()?;
        if let Some((id, _)) = unchanged {
            self.conn
                .prepare_cached("UPDATE raw_records SET fetched_at = ?2 WHERE id = ?1")?
                .execute(params![id, Utc::now().to_rfc3339()])?;
        }
        Ok(unchanged)
    }

    /// The same **explicitly empty** payload was fetched again (heart rate `items` empty for a day). The current parser
    /// already decided "the cloud has nothing", and a new normalization gives the same result. Before, every sync stored it again,
    /// deleted the quarantine record and quarantined it again. Returns the earlier verdict text. The caller still reports "no data in the cloud".
    pub(super) fn touch_unchanged_empty_raw(
        &self,
        record: &RawRecord,
        serialized: &SerializedPayload,
    ) -> Result<Option<String>> {
        let unchanged = self
            .conn
            .prepare_cached(
                "SELECT r.id, q.error
                 FROM raw_records r
                 JOIN raw_quarantine q
                   ON q.raw_record_id = r.id AND q.revision = ?4 AND q.empty_response = 1
                 WHERE r.stream = ?1 AND r.source_key = ?2 AND r.payload_hash = ?3",
            )?
            .query_row(
                params![
                    record.stream,
                    record.source_key,
                    serialized.hash,
                    NORMALIZER_REVISION
                ],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?;
        let Some((id, error)) = unchanged else {
            return Ok(None);
        };
        self.conn
            .prepare_cached("UPDATE raw_records SET fetched_at = ?2 WHERE id = ?1")?
            .execute(params![id, Utc::now().to_rfc3339()])?;
        // The stored text is the Display ("Data unavailable: response items is empty"). Restore the original detail.
        let detail = error
            .strip_prefix("Data unavailable: ")
            .unwrap_or(&error)
            .to_string();
        Ok(Some(detail))
    }

    pub fn normalize_and_persist_raw(
        &self,
        raw_record_id: i64,
        stream: &str,
        source_key: &str,
        payload: &serde_json::Value,
    ) -> Result<NormalizationCounts> {
        note_content_change();
        // Payloads from the official open platform reuse the stream names of the old channel. The source_key prefix tells them apart (storage/official.rs).
        if source_key.starts_with(crate::official::fetch::SOURCE_PREFIX) {
            return self.normalize_official_raw(raw_record_id, source_key, payload);
        }
        let mut counts = NormalizationCounts::default();
        match stream {
            "heart_rate" => {
                let rows = Normalizer::normalize_heart_rate(payload)?;
                counts.primary_records = rows.len() as i64;
                self.clear_normalized_for_raw(raw_record_id, stream)?;
                for row in rows {
                    self.insert_metric_sample_with_raw(&row, Some(raw_record_id))?;
                }
            }
            "hrv" => {
                let rows = Normalizer::normalize_hrv(payload)?;
                counts.primary_records = rows.len() as i64;
                self.clear_normalized_for_raw(raw_record_id, stream)?;
                for row in rows {
                    self.insert_metric_sample_with_raw(&row, Some(raw_record_id))?;
                }
            }
            // Optional wellness streams. Their payload shapes are not verified
            // field by field yet, so normalization is best-effort and must
            // never fail. Raw is already committed before this runs; a later
            // persist error no longer deletes the response.
            "wellness" => {
                let batch = Normalizer::normalize_wellness(source_key, payload);
                if source_key.starts_with("wellness:blood_pressure:")
                    && batch.metric_samples.is_empty()
                    && !batch.diagnostics.is_empty()
                {
                    return Err(ZeppBridgeError::DataUnavailable(
                        batch.diagnostics.join("; "),
                    ));
                }
                counts.primary_records =
                    (batch.daily_metrics.len() + batch.metric_samples.len()) as i64;
                self.clear_normalized_for_raw(raw_record_id, "daily_summary")?;
                self.clear_normalized_for_raw(raw_record_id, "heart_rate")?;
                for row in batch.daily_metrics {
                    self.insert_daily_metric_with_raw(&row, Some(raw_record_id))?;
                }
                for row in batch.metric_samples {
                    self.insert_metric_sample_with_raw(&row, Some(raw_record_id))?;
                }
            }
            // Weight / body composition. Best effort, like wellness: the fields of `summary` change with
            // the record source. Unknown fields go only into diagnostics, and the stream does not fail.
            // The raw payload is committed first, so a normalization failure does not roll it back.
            "weight" => {
                let batch = Normalizer::normalize_weight(payload);
                counts.primary_records = batch.metric_samples.len() as i64;
                self.clear_normalized_for_raw(raw_record_id, stream)?;
                for row in batch.metric_samples {
                    self.insert_metric_sample_with_raw(&row, Some(raw_record_id))?;
                }
            }
            "daily_summary" => {
                let rows = Normalizer::normalize_daily_summary(payload)?;
                counts.primary_records = rows.len() as i64;
                self.clear_normalized_for_raw(raw_record_id, stream)?;
                for row in rows {
                    self.insert_daily_metric_with_raw(&row, Some(raw_record_id))?;
                }
            }
            "sleep" => {
                let zone = self.device_time_zone().ok().flatten();
                let band = Normalizer::normalize_band_data_in_zone(payload, zone.as_deref())?;
                if band.sleep_sessions.is_empty()
                    && band.heart_rate_samples.is_empty()
                    && band.daily_metrics.is_empty()
                {
                    let detail = if band.diagnostics.is_empty() {
                        "band_data has no recognizable records".to_string()
                    } else {
                        band.diagnostics.join("; ")
                    };
                    return Err(ZeppBridgeError::DataUnavailable(detail));
                }
                counts.primary_records = band.sleep_sessions.len() as i64;
                counts.band_heart_rate_records = band.heart_rate_samples.len() as i64;
                counts.supplemental_daily_records = band.daily_metrics.len() as i64;
                self.clear_normalized_for_raw(raw_record_id, stream)?;
                for row in band.sleep_sessions {
                    self.insert_sleep_session_with_raw(&row, Some(raw_record_id))?;
                }
                for row in band.heart_rate_samples {
                    self.insert_metric_sample_with_raw(&row, Some(raw_record_id))?;
                }
                for row in band.daily_metrics {
                    self.insert_daily_metric_with_raw(&row, Some(raw_record_id))?;
                }
                self.harvest_device_identities(payload)?;
            }
            "workouts" => {
                let sport = source_key
                    .strip_prefix("sport_history:")
                    .and_then(|value| value.split(':').next());
                let rows =
                    Normalizer::normalize_workouts_with_device_context(payload, sport, |id| {
                        self.workout_device_catalog_id(id)
                    })?;
                counts.primary_records = rows.len() as i64;
                // Compute the new rows first, then delete only the rows "this payload produced before but no longer produces".
                // Other streams "clear first, then insert". Workouts cannot do that. See
                // `clear_workouts_for_raw_except`.
                let strength_summaries = Normalizer::extract_workout_strength_summaries(payload);
                let keep: Vec<String> = rows.iter().map(|row| row.workout_id.clone()).collect();
                self.clear_workouts_for_raw_except(raw_record_id, &keep)?;
                for row in rows {
                    self.insert_workout_with_raw(&row, Some(raw_record_id))?;
                    if let Some(summary) = strength_summaries.get(&row.workout_id) {
                        self.upsert_workout_strength_summary(&row.workout_id, summary)?;
                    }
                }
                self.harvest_device_identities(payload)?;
            }
            "workout_detail" => {
                let workout_id = workout_id_from_detail_key(source_key).ok_or_else(|| {
                    ZeppBridgeError::ConfigError("workout_detail source_key is invalid".into())
                })?;
                if !self.workout_exists(&workout_id)? {
                    return Err(ZeppBridgeError::DataUnavailable(
                        "the workout summary for this detail does not exist yet".into(),
                    ));
                }
                let summary_end = self.workout_end_time(&workout_id)?;
                let summary_distance = self.workout_distance_meters(&workout_id)?;
                let decoded = decode_workout_detail(payload, summary_end, summary_distance)?;
                // For a workout where the user imported a corrected .fit, the cloud detail stays only in raw_records (#140).
                self.apply_cloud_series(&workout_id, &decoded)?;
                counts.primary_records = (decoded.samples.len()
                    + decoded.route.len()
                    + decoded.pauses.len()
                    + decoded.strength_sets.len()) as i64;
            }
            other => return Err(ZeppBridgeError::ConfigError(format!("unknown sync stream: {other}"))),
        }
        // Keep the processing result even when another raw payload later
        // replaces all canonical rows through their natural-key upserts.
        self.conn.execute(
            "INSERT INTO raw_normalization(raw_record_id, revision, records_written)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(raw_record_id) DO UPDATE SET
                revision = excluded.revision, records_written = excluded.records_written",
            params![
                raw_record_id,
                NORMALIZER_REVISION,
                counts.primary_records
                    + counts.band_heart_rate_records
                    + counts.supplemental_daily_records
            ],
        )?;
        Ok(counts)
    }

    pub(super) fn clear_normalized_for_raw(&self, raw_record_id: i64, stream: &str) -> Result<()> {
        match stream {
            "heart_rate" | "hrv" | "weight" => {
                self.conn.execute(
                    "DELETE FROM metric_samples WHERE raw_record_id = ?1",
                    [raw_record_id],
                )?;
            }
            "daily_summary" => {
                self.conn.execute(
                    "DELETE FROM daily_metrics WHERE raw_record_id = ?1",
                    [raw_record_id],
                )?;
            }
            "sleep" => {
                self.conn.execute(
                    "DELETE FROM metric_samples WHERE raw_record_id = ?1",
                    [raw_record_id],
                )?;
                self.conn.execute(
                    "DELETE FROM daily_metrics WHERE raw_record_id = ?1",
                    [raw_record_id],
                )?;
                self.conn.execute(
                    "DELETE FROM sleep_sessions WHERE raw_record_id = ?1",
                    [raw_record_id],
                )?;
            }
            // Workout cleanup is not here: it must first know which rows will be inserted again at once.
            // See `clear_workouts_for_raw_except`.
            "workouts" => {}
            "workout_detail" => {}
            _ => {}
        }
        Ok(())
    }

    /// Delete workout summary rows that this payload produced before but this normalization no longer produces.
    ///
    /// Why not "delete all, then insert" like other streams: `workout_samples`, `route_points`,
    /// `workout_pauses` and `workout_splits` all hang on `workouts` with `ON DELETE CASCADE`.
    /// Deleting a summary row that is about to be inserted again cascades and removes the per-second
    /// series and GPS track of that workout. Those come from `workout_detail`, and a partial replay
    /// does not replay that stream at all (v17 to v18 replayed only workouts and sleep). The result was
    /// an upgrade that reported success, and the user's track history was gone.
    ///
    /// The summary row itself does not need a delete first: `insert_workout_with_raw` is an upsert, and it reads the old row
    /// on purpose. `merge_workout_type` needs that row to correct old normalization results and
    /// keep the type the user changed. A delete first would break exactly that mechanism.
    pub(super) fn clear_workouts_for_raw_except(
        &self,
        raw_record_id: i64,
        keep: &[String],
    ) -> Result<()> {
        if keep.is_empty() {
            self.conn.execute(
                "DELETE FROM workouts WHERE raw_record_id = ?1",
                [raw_record_id],
            )?;
            return Ok(());
        }
        // The parameter count follows the number of workouts produced this time. Do not build the IN list by hand: these values come from
        // cloud payloads, not compile-time constants.
        let placeholders = (2..=keep.len() + 1)
            .map(|index| format!("?{index}"))
            .collect::<Vec<_>>()
            .join(", ");
        let mut parameters: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(keep.len() + 1);
        parameters.push(&raw_record_id);
        for workout_id in keep {
            parameters.push(workout_id);
        }
        self.conn.execute(
            &format!(
                "DELETE FROM workouts
                 WHERE raw_record_id = ?1 AND workout_id NOT IN ({placeholders})"
            ),
            parameters.as_slice(),
        )?;
        Ok(())
    }
}
