//! Persisting official open platform payloads.
//!
//! The entry is `normalize_and_persist_raw` in `ingest.rs`: payloads whose `source_key` starts with `official:`
//! go here. The write works like the legacy channel: first clear the rows this payload produced before, then upsert by natural key.
//! The only difference: after the write, the `provider` of these rows (and of the payload itself) is set to `official`.
//!
//! When both sides have the same night, both are stored: the official sleep `sleep_id` is `official:<start seconds>`, which does not
//! collide with the legacy `band:...` key. The UI reads the `sleep_sessions_shown` view and shows only the official one per night (v34).
//! Heart rate, daily steps, workouts, PAI and weight are fetched only when there is no legacy channel (`sync/official.rs`), so they never
//! overwrite legacy rows with the same key.

use super::*;

/// Source name of official details in the failure count table. The legacy channel uses the workout's own `zepp_source`, so they do not collide.
const OFFICIAL_DETAIL_SOURCE: &str = "official";
use crate::official::fetch::{OfficialKind, PROVIDER};

impl Database {
    pub(super) fn normalize_official_raw(
        &self,
        raw_record_id: i64,
        source_key: &str,
        payload: &serde_json::Value,
    ) -> Result<NormalizationCounts> {
        let kind = OfficialKind::from_source_key(source_key).ok_or_else(|| {
            ZeppBridgeError::ConfigError(format!("Unrecognized official payload: {source_key}"))
        })?;
        let mut counts = NormalizationCounts::default();
        match kind {
            OfficialKind::Sleep => {
                let nights = Normalizer::normalize_official_sleep(payload);
                self.conn.execute(
                    "DELETE FROM sleep_sessions WHERE raw_record_id = ?1",
                    [raw_record_id],
                )?;
                for night in &nights {
                    self.insert_sleep_session_with_raw(&night.session, Some(raw_record_id))?;
                    self.conn.execute(
                        "UPDATE sleep_sessions SET rem_seconds = ?2, nap_total_seconds = ?3 WHERE sleep_id = ?1",
                        params![night.session.sleep_id, night.rem_seconds, night.nap_total_seconds],
                    )?;
                    self.replace_sleep_nap_stages(&night.session.sleep_id, &night.nap_stages)?;
                }
                counts.primary_records = nights.len() as i64;
            }
            OfficialKind::HeartRate | OfficialKind::ActivityHourly => {
                let rows = if kind == OfficialKind::HeartRate {
                    Normalizer::normalize_official_heart_rate(payload)
                } else {
                    Normalizer::normalize_official_activity_hourly(payload)
                };
                self.conn.execute(
                    "DELETE FROM metric_samples WHERE raw_record_id = ?1",
                    [raw_record_id],
                )?;
                for row in &rows {
                    self.insert_metric_sample_with_raw(row, Some(raw_record_id))?;
                }
                counts.primary_records = rows.len() as i64;
            }
            OfficialKind::ActivityDaily => {
                let rows = Normalizer::normalize_official_activity_daily(payload);
                self.conn.execute(
                    "DELETE FROM daily_metrics WHERE raw_record_id = ?1",
                    [raw_record_id],
                )?;
                for row in &rows {
                    self.insert_daily_metric_with_raw(row, Some(raw_record_id))?;
                }
                counts.primary_records = rows.len() as i64;
            }
            OfficialKind::Pai => {
                let batch = Normalizer::normalize_wellness("wellness:pai", payload);
                self.conn.execute(
                    "DELETE FROM daily_metrics WHERE raw_record_id = ?1",
                    [raw_record_id],
                )?;
                for row in &batch.daily_metrics {
                    self.insert_daily_metric_with_raw(row, Some(raw_record_id))?;
                }
                counts.primary_records = batch.daily_metrics.len() as i64;
            }
            OfficialKind::Body => {
                let batch =
                    Normalizer::normalize_weight(&Normalizer::official_body_as_legacy(payload));
                self.conn.execute(
                    "DELETE FROM metric_samples WHERE raw_record_id = ?1",
                    [raw_record_id],
                )?;
                for row in &batch.metric_samples {
                    self.insert_metric_sample_with_raw(row, Some(raw_record_id))?;
                }
                counts.primary_records = batch.metric_samples.len() as i64;
            }
            OfficialKind::Sports => {
                let legacy = Normalizer::official_sports_as_legacy(payload);
                let rows =
                    Normalizer::normalize_workouts_with_device_context(&legacy, None, |id| {
                        self.workout_device_catalog_id(id)
                    })?;
                let strength_summaries = Normalizer::extract_workout_strength_summaries(&legacy);
                let keep: Vec<String> = rows.iter().map(|row| row.workout_id.clone()).collect();
                // Like the legacy channel, delete only rows "no longer produced": delete-then-insert would cascade and remove per-second series and tracks.
                self.clear_workouts_for_raw_except(raw_record_id, &keep)?;
                for row in &rows {
                    self.insert_workout_with_raw(row, Some(raw_record_id))?;
                    if let Some(summary) = strength_summaries.get(&row.workout_id) {
                        self.upsert_workout_strength_summary(&row.workout_id, summary)?;
                    }
                }
                counts.primary_records = rows.len() as i64;
            }
            OfficialKind::SportDetail => {
                let workout_id = source_key
                    .rsplit(':')
                    .next()
                    .filter(|id| !id.is_empty())
                    .ok_or_else(|| {
                        ZeppBridgeError::ConfigError("Invalid source_key for official workout details".into())
                    })?
                    .to_string();
                if !self.workout_exists(&workout_id)? {
                    return Err(ZeppBridgeError::DataUnavailable(
                        "The workout summary for these details does not exist yet".into(),
                    ));
                }
                // The advanced channel already has details for this workout: it wins, and this official copy no longer replaces the whole per-second series (#143).
                // The payload still counts as processed (raw_normalization is recorded below), so it is not replayed again and again.
                if self.has_legacy_workout_detail(&workout_id)? {
                    counts.primary_records = 0;
                } else {
                    let summary_end = self.workout_end_time(&workout_id)?;
                    let summary_distance = self.workout_distance_meters(&workout_id)?;
                    let mut decoded = decode_workout_detail(
                        &Normalizer::official_detail_as_legacy(payload),
                        summary_end,
                        summary_distance,
                    )?;
                    drop_unreconciled_distance(&mut decoded, summary_distance);
                    // For workouts where the user imported a corrected .fit, cloud details stay only in raw_records (#140).
                    self.apply_cloud_series(&workout_id, &decoded)?;
                    counts.primary_records = (decoded.samples.len()
                        + decoded.route.len()
                        + decoded.pauses.len()
                        + decoded.strength_sets.len())
                        as i64;
                }
            }
        }
        self.mark_official_rows(raw_record_id)?;
        self.conn.execute(
            "INSERT INTO raw_normalization(raw_record_id, revision, records_written)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(raw_record_id) DO UPDATE SET
                revision = excluded.revision, records_written = excluded.records_written",
            params![raw_record_id, NORMALIZER_REVISION, counts.primary_records],
        )?;
        Ok(counts)
    }

    /// Marks the source on the payload and the rows it produced. Normalization writes the shared columns; the source column is filled here.
    fn mark_official_rows(&self, raw_record_id: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE raw_records SET provider = ?2, channel = 'official_pull',
                received_at = COALESCE(received_at, fetched_at), parsed_at = ?3
             WHERE id = ?1",
            params![raw_record_id, PROVIDER, Utc::now().to_rfc3339()],
        )?;
        for table in [
            "sleep_sessions",
            "metric_samples",
            "daily_metrics",
            "workouts",
        ] {
            self.conn.execute(
                &format!("UPDATE {table} SET provider = ?2 WHERE raw_record_id = ?1"),
                params![raw_record_id, PROVIDER],
            )?;
        }
        Ok(())
    }

    /// The official workouts that have no per-second series yet (details are fetched next). Looks only at official rows:
    /// legacy workouts are filled by the legacy channel's own detail queue.
    /// Official workouts with no per-second details yet. Workouts with [`MAX_WORKOUT_DETAIL_ATTEMPTS`] failures in a row
    /// (or that the official API says have none) are set aside, and tried once more after [`WORKOUT_DETAIL_ATTEMPT_DECAY`].
    /// This uses the same failure count table and decay as the legacy channel (source recorded as `official`).
    pub fn official_workouts_missing_detail(&self, limit: usize) -> Result<Vec<(String, String)>> {
        let decay_before = (Utc::now() - WORKOUT_DETAIL_ATTEMPT_DECAY).to_rfc3339();
        let mut stmt = self.conn.prepare(
            "SELECT w.workout_id, w.start_time FROM workouts w
             LEFT JOIN workout_detail_fetch_attempts a
               ON a.workout_id = w.workout_id AND a.source = ?2
             WHERE w.provider = 'official'
               AND NOT EXISTS (SELECT 1 FROM workout_samples s WHERE s.workout_id = w.workout_id)
               AND NOT EXISTS (SELECT 1 FROM route_points r WHERE r.workout_id = w.workout_id)
               AND NOT EXISTS (
                   SELECT 1 FROM raw_records r
                   WHERE r.stream = 'workout_detail' AND r.source_key = 'official:sport_detail:' || w.workout_id)
               AND (a.attempts IS NULL OR a.attempts < ?3 OR a.updated_at < ?4)
             ORDER BY COALESCE(a.attempts, 0) ASC, w.start_time DESC LIMIT ?1",
        )?;
        let rows = stmt
            .query_map(
                rusqlite::params![
                    limit as i64,
                    OFFICIAL_DETAIL_SOURCE,
                    MAX_WORKOUT_DETAIL_ATTEMPTS,
                    decay_before
                ],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Records the result of one official detail fetch. Returns whether this workout is now set aside.
    /// `gone`: the official API states there is none (unavailable). No need to fail twice more; set it aside now.
    pub fn record_official_detail_result(
        &self,
        workout_id: &str,
        ok: bool,
        gone: bool,
    ) -> Result<bool> {
        self.record_workout_detail_fetch_result(workout_id, OFFICIAL_DETAIL_SOURCE, ok)?;
        if ok {
            return Ok(false);
        }
        if gone {
            self.conn.execute(
                "UPDATE workout_detail_fetch_attempts SET attempts = MAX(attempts, ?3)
                 WHERE workout_id = ?1 AND source = ?2",
                rusqlite::params![
                    workout_id,
                    OFFICIAL_DETAIL_SOURCE,
                    MAX_WORKOUT_DETAIL_ATTEMPTS
                ],
            )?;
        }
        let attempts: i64 = self.conn.query_row(
            "SELECT attempts FROM workout_detail_fetch_attempts WHERE workout_id = ?1 AND source = ?2",
            [workout_id, OFFICIAL_DETAIL_SOURCE],
            |row| row.get(0),
        )?;
        Ok(attempts >= MAX_WORKOUT_DETAIL_ATTEMPTS)
    }
}

/// Per-point distance in official details (`distanceInCentimeter`, decoded as `currentDistance`) is kept only if it matches the summary distance.
///
/// The unit of this series was seen only in synthetic samples (shape "seconds delta, cumulative cm", like `currentDistance` in the advanced channel),
/// and was not checked against real payloads. Reconciliation: if the last per-second distance differs from the summary distance by more than 3%, or there is no summary distance,
/// clear the whole per-second distance column. Better missing than invented, the same rule as the laps / kilo_pace reconciliation (#143).
pub(super) fn drop_unreconciled_distance(
    decoded: &mut DecodedWorkout,
    summary_distance_m: Option<f64>,
) {
    let last = decoded
        .samples
        .iter()
        .filter_map(|sample| sample.distance_m)
        .fold(None, |best: Option<f64>, value| {
            Some(best.map_or(value, |b| b.max(value)))
        });
    let Some(last) = last else { return };
    let agrees = summary_distance_m
        .filter(|total| total.is_finite() && *total > 0.0)
        .is_some_and(|total| ((last - total) / total).abs() <= 0.03);
    if !agrees {
        for sample in &mut decoded.samples {
            sample.distance_m = None;
        }
    }
}
