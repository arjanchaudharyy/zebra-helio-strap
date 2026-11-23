//! The user replaces a workout with a corrected `.fit` file (#140).
//!
//! Like `corrections.rs`, this is **layer 3: the user's local corrections**. The cloud raw payload does not change,
//! nothing goes back to the cloud (the Zepp open API has no endpoint to import workouts), and replay does not touch it:
//!
//! * Series: every cloud detail stored (sync, replay) goes through [`Database::apply_cloud_series`].
//!   With a correction it writes the series read from the correction file. The cloud copy stays only in `raw_records`;
//! * Summary: each time a cloud summary is written to `workouts`, [`Database::reapply_fit_override_summary`] runs.
//!   It first saves the new cloud values in `original_*`, then writes the corrected values over them. Undo puts them back unchanged.
//!
//! So the UI, CLI, MCP and exports all read the corrected version, and no output needs its own check.

use chrono::{DateTime, Utc};
use rusqlite::{params, params_from_iter, types::Value as SqlValue, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::payload::decode_raw_payload;
use super::util::{parse_datetime, workout_id_from_detail_key};
use super::Database;
use crate::decoder::{decode_workout_detail, DecodedWorkout};
use crate::export_fit::{decode_fit_file, ImportedFit};
use crate::models::error::{Result, ZeppBridgeError};
use crate::normalizer::Normalizer;

/// Maximum difference in seconds between the start of the correction file and the original record. The user said a correction keeps the original start;
/// 5 minutes of margin covers how different tools define "start". More than that is not the same workout.
pub const FIT_START_TOLERANCE_SECONDS: i64 = 5 * 60;

/// Correction info that the UI and MCP see. It does not include the file itself.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkoutFitOverride {
    pub workout_id: String,
    pub file_name: Option<String>,
    pub sha256: String,
    pub byte_len: i64,
    pub imported_at: String,
    pub distance_m: Option<f64>,
    pub moving_seconds: Option<i64>,
    /// Distance from the cloud. Undo returns to this value.
    pub original_distance_m: Option<f64>,
    pub original_moving_seconds: Option<i64>,
}

/// Result of an import. The caller maps refusals to `err.workout_fit.*` codes.
#[derive(Debug, Clone, PartialEq)]
pub enum FitOverrideOutcome {
    Applied(WorkoutFitOverride),
    WorkoutMissing,
    StartMismatch {
        workout_start: DateTime<Utc>,
        fit_start: DateTime<Utc>,
    },
}

impl Database {
    /// Replace `workout_id` with a correction file that is already parsed. The caller must hold the write lock.
    pub fn import_workout_fit(
        &self,
        workout_id: &str,
        file_name: Option<&str>,
        bytes: &[u8],
        imported: &ImportedFit,
    ) -> Result<FitOverrideOutcome> {
        let workout_start: Option<String> = self
            .conn
            .query_row(
                "SELECT start_time FROM workouts WHERE workout_id = ?1",
                [workout_id],
                |row| row.get(0),
            )
            .optional()?;
        let Some(workout_start) = workout_start else {
            return Ok(FitOverrideOutcome::WorkoutMissing);
        };
        let workout_start = parse_datetime(&workout_start, "workouts.start_time")?;
        let fit_start = imported.decoded.start_time;
        if (fit_start - workout_start).num_seconds().abs() > FIT_START_TOLERANCE_SECONDS {
            return Ok(FitOverrideOutcome::StartMismatch {
                workout_start,
                fit_start,
            });
        }
        let summary = &imported.summary;
        let fit_end = summary.end_time.unwrap_or(imported.decoded.end_time);
        let tx = self.conn.unchecked_transaction()?;
        let original_series = if self.workout_fit_override(workout_id)?.is_none() {
            Some(serde_json::to_string(
                &self.snapshot_fit_original(workout_id)?,
            )?)
        } else {
            None
        };
        // Save the original values on the first import. A second import (with a different correction) does not overwrite them.
        tx.execute(
            "INSERT INTO workout_fit_overrides
                (workout_id, file_name, sha256, byte_len, fit, imported_at,
                 fit_start_time, fit_end_time, fit_distance_m, fit_moving_seconds,
                 fit_avg_hr, fit_max_hr,
                 original_end_time, original_distance_m, original_moving_seconds,
                 original_avg_hr, original_max_hr, original_series_json)
             SELECT ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12,
                    end_time, distance_meters, moving_seconds, avg_hr, max_hr, ?13
               FROM workouts WHERE workout_id = ?1
             ON CONFLICT(workout_id) DO UPDATE SET
                file_name = excluded.file_name,
                sha256 = excluded.sha256,
                byte_len = excluded.byte_len,
                fit = excluded.fit,
                imported_at = excluded.imported_at,
                fit_start_time = excluded.fit_start_time,
                fit_end_time = excluded.fit_end_time,
                fit_distance_m = excluded.fit_distance_m,
                fit_moving_seconds = excluded.fit_moving_seconds,
                fit_avg_hr = excluded.fit_avg_hr,
                fit_max_hr = excluded.fit_max_hr",
            params![
                workout_id,
                file_name.map(|name| name.chars().take(200).collect::<String>()),
                hex::encode(Sha256::digest(bytes)),
                bytes.len() as i64,
                bytes,
                Utc::now().to_rfc3339(),
                fit_start.to_rfc3339(),
                fit_end.to_rfc3339(),
                summary.distance_m,
                summary.moving_seconds,
                summary.avg_hr,
                summary.max_hr,
                original_series,
            ],
        )?;
        self.overlay_fit_summary(workout_id)?;
        self.replace_workout_series(workout_id, &imported.decoded)?;
        let outcome = match self.workout_fit_override(workout_id)? {
            Some(info) => FitOverrideOutcome::Applied(info),
            None => FitOverrideOutcome::WorkoutMissing,
        };
        tx.commit()?;
        Ok(outcome)
    }

    /// Undo the correction: the summary goes back to the cloud values, and the series is decoded again from the latest local cloud detail.
    /// Returns `false` when there was no correction.
    pub fn remove_workout_fit(&self, workout_id: &str) -> Result<bool> {
        let tx = self.conn.unchecked_transaction()?;
        let original: Option<Option<String>> = self
            .conn
            .query_row(
                "SELECT original_series_json FROM workout_fit_overrides WHERE workout_id = ?1",
                [workout_id],
                |row| row.get(0),
            )
            .optional()?;
        let Some(original) = original else {
            return Ok(false);
        };
        let original = original
            .as_deref()
            .map(serde_json::from_str::<FitOriginal>)
            .transpose()?;

        self.conn.execute(
            "UPDATE workouts SET
                end_time = COALESCE(o.original_end_time, workouts.end_time),
                distance_meters = o.original_distance_m,
                moving_seconds = o.original_moving_seconds,
                avg_hr = o.original_avg_hr,
                max_hr = o.original_max_hr
             FROM workout_fit_overrides o
             WHERE o.workout_id = workouts.workout_id AND workouts.workout_id = ?1",
            [workout_id],
        )?;
        if let Some(decoded) = self.decode_cloud_detail(workout_id)? {
            self.replace_workout_series(workout_id, &decoded)?;
        } else if let Some(original) = original {
            self.restore_fit_original(workout_id, original)?;
        } else {
            return Err(ZeppBridgeError::WorkoutFitOriginalMissing);
        }
        self.conn.execute(
            "DELETE FROM workout_fit_overrides WHERE workout_id = ?1",
            [workout_id],
        )?;
        tx.commit()?;
        Ok(true)
    }

    fn snapshot_fit_original(&self, workout_id: &str) -> Result<FitOriginal> {
        let (gps_available, sample_count) = self.conn.query_row(
            "SELECT gps_available, sample_count FROM workouts WHERE workout_id = ?1",
            [workout_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let mut rows = std::collections::BTreeMap::new();
        for (table, columns) in SERIES_COLUMNS {
            let mut statement = self.conn.prepare(&format!(
                "SELECT {columns} FROM {table} WHERE workout_id = ?1 ORDER BY rowid"
            ))?;
            let count = statement.column_count();
            let values = statement
                .query_map([workout_id], |row| {
                    (0..count)
                        .map(|index| row.get::<_, SqlValue>(index).map(FitOriginalValue::from))
                        .collect::<rusqlite::Result<Vec<_>>>()
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            rows.insert(table.to_owned(), values);
        }
        Ok(FitOriginal {
            gps_available,
            sample_count,
            rows,
        })
    }

    fn restore_fit_original(&self, workout_id: &str, original: FitOriginal) -> Result<()> {
        for (table, columns) in SERIES_COLUMNS {
            let rows = original
                .rows
                .get(table)
                .ok_or(ZeppBridgeError::WorkoutFitOriginalMissing)?;
            let values = columns
                .split(',')
                .map(|_| "?")
                .collect::<Vec<_>>()
                .join(", ");
            self.conn.execute(
                &format!("DELETE FROM {table} WHERE workout_id = ?1"),
                [workout_id],
            )?;
            let mut insert = self.conn.prepare(&format!(
                "INSERT INTO {table} (workout_id, {columns}) VALUES (?, {values})"
            ))?;
            for row in rows {
                insert.execute(params_from_iter(
                    std::iter::once(SqlValue::Text(workout_id.to_owned()))
                        .chain(row.iter().cloned().map(SqlValue::from)),
                ))?;
            }
        }
        self.conn.execute(
            "UPDATE workouts SET gps_available = ?2, sample_count = ?3 WHERE workout_id = ?1",
            params![workout_id, original.gps_available, original.sample_count],
        )?;
        Ok(())
    }

    pub fn workout_fit_override(&self, workout_id: &str) -> Result<Option<WorkoutFitOverride>> {
        self.conn
            .query_row(
                &format!("{OVERRIDE_SELECT} WHERE workout_id = ?1"),
                [workout_id],
                override_from_row,
            )
            .optional()
            .map_err(Into::into)
    }

    /// All local corrections, sorted by import time. The CLI `fit-override --list` uses it (#143:
    /// corrections exist only on this machine, so a user with several computers must first know which ones this one has).
    pub fn list_workout_fit_overrides(&self) -> Result<Vec<WorkoutFitOverride>> {
        let mut stmt = self
            .conn
            .prepare(&format!("{OVERRIDE_SELECT} ORDER BY imported_at"))?;
        let rows = stmt.query_map([], override_from_row)?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    /// The only entry for storing cloud detail: with a correction, it writes the series from the correction file.
    pub(super) fn apply_cloud_series(
        &self,
        workout_id: &str,
        cloud: &DecodedWorkout,
    ) -> Result<()> {
        match self.fit_override_series(workout_id)? {
            Some(corrected) => self.replace_workout_series(workout_id, &corrected),
            None => self.replace_workout_series(workout_id, cloud),
        }
    }

    /// Call right after a cloud summary is written to `workouts`: with a correction, save the new cloud values as originals,
    /// then write the corrected values back. Without a correction it does nothing.
    pub(super) fn reapply_fit_override_summary(&self, workout_id: &str) -> Result<()> {
        let updated = self.conn.execute(
            "UPDATE workout_fit_overrides SET
                original_end_time = w.end_time,
                original_distance_m = w.distance_meters,
                original_moving_seconds = w.moving_seconds,
                original_avg_hr = w.avg_hr,
                original_max_hr = w.max_hr
             FROM workouts w
             WHERE w.workout_id = workout_fit_overrides.workout_id
               AND workout_fit_overrides.workout_id = ?1",
            [workout_id],
        )?;
        if updated > 0 {
            self.overlay_fit_summary(workout_id)?;
        }
        Ok(())
    }

    /// Write the corrected values into `workouts`. Fields the correction file does not give keep the cloud value.
    fn overlay_fit_summary(&self, workout_id: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE workouts SET
                end_time = o.fit_end_time,
                distance_meters = COALESCE(o.fit_distance_m, workouts.distance_meters),
                moving_seconds = COALESCE(o.fit_moving_seconds, workouts.moving_seconds),
                avg_hr = COALESCE(o.fit_avg_hr, workouts.avg_hr),
                max_hr = COALESCE(o.fit_max_hr, workouts.max_hr)
             FROM workout_fit_overrides o
             WHERE o.workout_id = workouts.workout_id AND workouts.workout_id = ?1",
            [workout_id],
        )?;
        Ok(())
    }

    fn fit_override_series(&self, workout_id: &str) -> Result<Option<DecodedWorkout>> {
        let bytes: Option<Vec<u8>> = self
            .conn
            .query_row(
                "SELECT fit FROM workout_fit_overrides WHERE workout_id = ?1",
                [workout_id],
                |row| row.get(0),
            )
            .optional()?;
        // It parsed when it was stored, so it parses again. If it truly does not parse (the library was damaged from outside), fall back
        // to the cloud series, so one bad file does not block a whole sync.
        Ok(bytes
            .and_then(|bytes| decode_fit_file(&bytes).ok())
            .map(|imported| imported.decoded))
    }

    /// Local cloud detail, decoded with the same parameters as when it was stored. When both channels have one, use the advanced channel (more fields,
    /// same priority as at storage, see `has_legacy_workout_detail`). Inside one channel, use the latest.
    fn decode_cloud_detail(&self, workout_id: &str) -> Result<Option<DecodedWorkout>> {
        let legacy_prefix = format!("workout_detail:{workout_id}:");
        let official_key = format!(
            "{}sport_detail:{workout_id}",
            crate::official::fetch::SOURCE_PREFIX
        );
        let row: Option<(i64, String)> = self
            .conn
            .query_row(
                "SELECT r.id, r.source_key FROM raw_records r
                  WHERE r.stream = 'workout_detail'
                    AND (substr(r.source_key, 1, length(?1)) = ?1 OR r.source_key = ?2)
                    AND NOT EXISTS (SELECT 1 FROM raw_quarantine q WHERE q.raw_record_id = r.id)
                  ORDER BY (substr(r.source_key, 1, length(?1)) = ?1) DESC, r.fetched_at DESC, r.id DESC
                  LIMIT 1",
                params![legacy_prefix, official_key],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((raw_id, source_key)) = row else {
            return Ok(None);
        };
        let Some((stored, zip)) = self.raw_payload(raw_id)? else {
            return Ok(None);
        };
        let payload: serde_json::Value = serde_json::from_str(&decode_raw_payload(stored, zip)?)?;
        let legacy = workout_id_from_detail_key(&source_key).is_some();
        let payload = if legacy {
            payload
        } else {
            Normalizer::official_detail_as_legacy(&payload)
        };
        let summary_end = self.workout_end_time(workout_id)?;
        let summary_distance = self.workout_distance_meters(workout_id)?;
        let mut decoded = decode_workout_detail(&payload, summary_end, summary_distance).ok();
        if let (false, Some(decoded)) = (legacy, decoded.as_mut()) {
            super::official::drop_unreconciled_distance(decoded, summary_distance);
        }
        Ok(decoded)
    }
}

#[derive(Serialize, Deserialize)]
struct FitOriginal {
    gps_available: bool,
    sample_count: i64,
    rows: std::collections::BTreeMap<String, Vec<Vec<FitOriginalValue>>>,
}

// Preserve SQLite REAL bits when they pass through the serialized backup.
#[derive(Clone, Serialize, Deserialize)]
enum FitOriginalValue {
    Null,
    Integer(i64),
    Real(u64),
    Text(String),
    Blob(Vec<u8>),
}

impl From<SqlValue> for FitOriginalValue {
    fn from(value: SqlValue) -> Self {
        match value {
            SqlValue::Null => Self::Null,
            SqlValue::Integer(value) => Self::Integer(value),
            SqlValue::Real(value) => Self::Real(value.to_bits()),
            SqlValue::Text(value) => Self::Text(value),
            SqlValue::Blob(value) => Self::Blob(value),
        }
    }
}

impl From<FitOriginalValue> for SqlValue {
    fn from(value: FitOriginalValue) -> Self {
        match value {
            FitOriginalValue::Null => Self::Null,
            FitOriginalValue::Integer(value) => Self::Integer(value),
            FitOriginalValue::Real(value) => Self::Real(f64::from_bits(value)),
            FitOriginalValue::Text(value) => Self::Text(value),
            FitOriginalValue::Blob(value) => Self::Blob(value),
        }
    }
}

// Preserve raw stored values so restoring the samples does not convert pace twice.
const SERIES_COLUMNS: [(&str, &str); 6] = [
    ("workout_samples", "timestamp, heart_rate, pace, speed, cadence, altitude, stride, power_watts, ground_contact_ms, vertical_oscillation_mm, vertical_ratio_pct, equivalent_pace_s, distance_m"),
    ("route_points", "timestamp, latitude, longitude, altitude, segment_start"),
    ("workout_pauses", "start_time, end_time, kind"),
    ("workout_splits", "split_index, start_time, end_time, distance_m, duration_seconds, pace_min_per_km, avg_hr, max_hr, elevation_gain_m, elevation_loss_m, partial"),
    ("workout_laps", "lap_index, start_time, end_time, distance_m, duration_seconds, avg_hr, max_hr"),
    ("workout_strength_sets", "set_index, action_code, exercise_key, muscle_group, reps, weight_value, weight_unit, weight_kg, duration_ms, avg_hr, calories_kcal"),
];

const OVERRIDE_SELECT: &str = "SELECT workout_id, file_name, sha256, byte_len, imported_at,
        fit_distance_m, fit_moving_seconds,
        original_distance_m, original_moving_seconds
 FROM workout_fit_overrides";

fn override_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<WorkoutFitOverride> {
    Ok(WorkoutFitOverride {
        workout_id: row.get(0)?,
        file_name: row.get(1)?,
        sha256: row.get(2)?,
        byte_len: row.get(3)?,
        imported_at: row.get(4)?,
        distance_m: row.get(5)?,
        moving_seconds: row.get(6)?,
        original_distance_m: row.get(7)?,
        original_moving_seconds: row.get(8)?,
    })
}
