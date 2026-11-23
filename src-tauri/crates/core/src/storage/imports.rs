//! Storing, listing and undoing whole batches of imported workouts (batch 7; file reading is in `crate::import`).
//!
//! One import is one batch (`import_batches`). For each imported workout, `workout_imports` records which batch
//! and which source it came from. The workout row itself goes into `workouts` (`raw_record_id` is empty, so replay does not touch it). Series use the same
//! `replace_workout_series` as cloud detail.
//!
//! Dedup: when this machine already has a workout that starts within two minutes and overlaps in time (synced from the cloud, or from an earlier import batch), do not
//! import it. Apple Health usually already has the copy that Zepp synced there. Duplicates inside one batch also keep only one.

use chrono::{DateTime, Utc};
use rusqlite::params;
use serde::Serialize;

use super::Database;
use crate::import::{ImportSource, ParsedActivity, ParsedFile};
use crate::models::error::Result;
use crate::models::{SourceScope, Workout};

/// Start times within this many seconds count as the same workout.
pub const DUPLICATE_WINDOW_SECONDS: i64 = 120;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ImportReport {
    /// No batch when nothing was added (all duplicates or unreadable).
    pub batch_id: Option<i64>,
    pub added: usize,
    pub duplicates: usize,
    pub unreadable: usize,
    /// The number of health data rows added (samples + daily values + sleep, only from unstrap backups; rows already on this machine do not count).
    pub health_added: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ImportBatch {
    pub id: i64,
    pub source: String,
    pub file_name: Option<String>,
    pub imported_at: String,
    pub added: i64,
    pub duplicates: i64,
    pub unreadable: i64,
    pub health_added: i64,
    /// An undone batch keeps one row (when it was undone). Its workouts are already deleted.
    pub undone_at: Option<String>,
    /// The start and end dates of the workouts in this batch (those still in the database).
    pub first_start: Option<String>,
    pub last_start: Option<String>,
}

impl Database {
    /// Store one parsed file as a batch. The caller takes the write lock.
    pub fn import_activities(
        &self,
        source: ImportSource,
        file_name: Option<&str>,
        parsed: ParsedFile,
        now: DateTime<Utc>,
    ) -> Result<ImportReport> {
        let health = parsed.health.unwrap_or_default();
        let mut activities = parsed.activities;
        activities.sort_by_key(|activity| activity.start_time);
        let mut fresh: Vec<ParsedActivity> = Vec::new();
        let mut duplicates = 0;
        for activity in activities {
            let in_batch = fresh
                .iter()
                .rev()
                .take_while(|previous| {
                    (activity.start_time - previous.start_time)
                        .num_seconds()
                        .abs()
                        <= DUPLICATE_WINDOW_SECONDS
                })
                .any(|previous| {
                    activity.start_time < previous.end_time
                        && previous.start_time < activity.end_time
                });
            if in_batch || self.has_workout_near(activity.start_time, activity.end_time)? {
                duplicates += 1;
            } else {
                fresh.push(activity);
            }
        }
        let nothing = || ImportReport {
            batch_id: None,
            added: 0,
            duplicates,
            unreadable: parsed.unreadable,
            health_added: 0,
        };
        if fresh.is_empty() && health.is_empty() {
            return Ok(nothing());
        }

        self.conn.execute_batch("BEGIN IMMEDIATE;")?;
        let written = (|| -> Result<(i64, usize)> {
            self.conn.execute(
                "INSERT INTO import_batches(source, file_name, imported_at, added, duplicates, unreadable)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    source.as_str(),
                    file_name,
                    now.to_rfc3339(),
                    fresh.len() as i64,
                    duplicates as i64,
                    parsed.unreadable as i64
                ],
            )?;
            let batch_id = self.conn.last_insert_rowid();
            for (index, activity) in fresh.iter().enumerate() {
                let workout_id = format!("import-{batch_id}-{}", index + 1);
                self.insert_workout(&imported_workout(&workout_id, activity, now))?;
                if let Some(decoded) = &activity.decoded {
                    self.replace_workout_series(&workout_id, decoded)?;
                }
                self.conn.execute(
                    "INSERT INTO workout_imports(workout_id, batch_id, source, name) VALUES(?1, ?2, ?3, ?4)",
                    params![workout_id, batch_id, source.as_str(), activity.name],
                )?;
            }
            let health = self.insert_imported_health(batch_id, &health, now)?;
            self.conn.execute(
                "UPDATE import_batches SET health_added = ?2 WHERE id = ?1",
                params![batch_id, health.total() as i64],
            )?;
            Ok((batch_id, health.total()))
        })();
        match written {
            // All workouts are duplicates and all health data is already on this machine: nothing was added, so do not leave an empty batch.
            Ok((_, 0)) if fresh.is_empty() => {
                self.conn.execute_batch("ROLLBACK;")?;
                Ok(nothing())
            }
            Ok((batch_id, health_added)) => {
                self.conn.execute_batch("COMMIT;")?;
                Ok(ImportReport {
                    batch_id: Some(batch_id),
                    added: fresh.len(),
                    duplicates,
                    unreadable: parsed.unreadable,
                    health_added,
                })
            }
            Err(error) => {
                let _ = self.conn.execute_batch("ROLLBACK;");
                Err(error)
            }
        }
    }

    fn has_workout_near(&self, start: DateTime<Utc>, end: DateTime<Utc>) -> Result<bool> {
        // start_time is stored as RFC 3339. Comparing with julianday works even when time zone formats differ.
        Ok(self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM workouts
                WHERE ABS(julianday(start_time) - julianday(?1)) * 86400 <= ?2
                AND julianday(end_time) > julianday(?1)
                AND julianday(start_time) < julianday(?3))",
            params![
                start.to_rfc3339(),
                DUPLICATE_WINDOW_SECONDS,
                end.to_rfc3339()
            ],
            |row| row.get(0),
        )?)
    }

    /// Undo a batch: delete its workouts, their series and the health data it added. The batch keeps one row "when it was undone".
    /// Returns how many workouts were deleted.
    pub fn undo_import_batch(&self, batch_id: i64, now: DateTime<Utc>) -> Result<usize> {
        self.conn.execute_batch("BEGIN IMMEDIATE;")?;
        let done = (|| -> Result<usize> {
            let ids: Vec<String> = {
                let mut stmt = self
                    .conn
                    .prepare("SELECT workout_id FROM workout_imports WHERE batch_id = ?1")?;
                let rows = stmt.query_map([batch_id], |row| row.get(0))?;
                rows.collect::<std::result::Result<_, _>>()?
            };
            for id in &ids {
                for table in [
                    "workout_samples",
                    "route_points",
                    "workout_pauses",
                    "workout_splits",
                    "workout_laps",
                    "workout_strength_sets",
                    "workout_hr_zones",
                    "workout_local_state",
                    "workout_annotations",
                    "workout_feedback",
                    "workout_fit_overrides",
                    "workout_detail_requests",
                    "workout_detail_fetch_attempts",
                    "workouts",
                ] {
                    self.conn
                        .execute(&format!("DELETE FROM {table} WHERE workout_id = ?1"), [id])?;
                }
            }
            self.conn.execute(
                "DELETE FROM workout_imports WHERE batch_id = ?1",
                [batch_id],
            )?;
            self.delete_imported_health(batch_id)?;
            self.conn.execute(
                "UPDATE import_batches SET undone_at = ?2 WHERE id = ?1 AND undone_at IS NULL",
                params![batch_id, now.to_rfc3339()],
            )?;
            Ok(ids.len())
        })();
        match done {
            Ok(count) => {
                self.conn.execute_batch("COMMIT;")?;
                Ok(count)
            }
            Err(error) => {
                let _ = self.conn.execute_batch("ROLLBACK;");
                Err(error)
            }
        }
    }

    /// All import batches, newest first.
    pub fn import_batches(&self) -> Result<Vec<ImportBatch>> {
        let mut stmt = self.conn.prepare(
            "SELECT b.id, b.source, b.file_name, b.imported_at, b.added, b.duplicates, b.unreadable,
                    b.health_added, b.undone_at, MIN(w.start_time), MAX(w.start_time)
             FROM import_batches b
             LEFT JOIN workout_imports i ON i.batch_id = b.id
             LEFT JOIN workouts w ON w.workout_id = i.workout_id
             GROUP BY b.id ORDER BY b.id DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(ImportBatch {
                id: row.get(0)?,
                source: row.get(1)?,
                file_name: row.get(2)?,
                imported_at: row.get(3)?,
                added: row.get(4)?,
                duplicates: row.get(5)?,
                unreadable: row.get(6)?,
                health_added: row.get(7)?,
                undone_at: row.get(8)?,
                first_start: row.get(9)?,
                last_start: row.get(10)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// Where each imported workout came from (`workout_id` -> `gpx` / `fit` / `strava` / `apple_health`). Used to show the source in the list.
    pub fn workout_import_sources(&self) -> Result<Vec<(String, String)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT workout_id, source FROM workout_imports")?;
        let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }
}

fn imported_workout(workout_id: &str, activity: &ParsedActivity, now: DateTime<Utc>) -> Workout {
    let route = activity
        .decoded
        .as_ref()
        .map_or(0, |decoded| decoded.route.len());
    let samples = activity
        .decoded
        .as_ref()
        .map_or(0, |decoded| decoded.samples.len());
    Workout {
        workout_id: workout_id.to_string(),
        workout_type: activity.sport.clone(),
        normalized_type: activity.sport.clone(),
        type_source: "imported".to_string(),
        user_override: None,
        effective_type: activity.sport.clone(),
        custom_label: None,
        start_time: activity.start_time,
        end_time: activity.end_time,
        distance_meters: activity.distance_m,
        calories: activity.calories,
        avg_hr: activity.avg_hr,
        max_hr: activity.max_hr,
        training_load: None,
        vo2max: None,
        min_hr: None,
        total_steps: None,
        moving_seconds: activity.moving_seconds,
        elevation_gain_m: activity.elevation_gain_m,
        elevation_loss_m: None,
        max_altitude_m: None,
        min_altitude_m: None,
        training_effect: None,
        anaerobic_training_effect: None,
        rpe: None,
        avg_cadence_spm: None,
        max_cadence_spm: None,
        avg_stride_cm: None,
        gear_id: None,
        hr_zones: Vec::new(),
        source_scope: SourceScope::Device,
        device_id: None,
        // The import time, not the cloud fetch time: imported workouts never went through the cloud.
        synced_at: Some(now),
        gps_available: route > 0,
        sample_count: samples as i64,
        zepp_source: None,
        zepp_type: None,
    }
}
