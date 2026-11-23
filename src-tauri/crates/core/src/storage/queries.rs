//! List and detail queries for sleep, workouts and the overview (split out of storage/mod.rs by domain, logic unchanged).

use super::*;

pub(super) fn stored_stage_minutes(minutes: Option<i32>) -> (i32, i64) {
    (minutes.unwrap_or(0), i64::from(minutes.is_some()))
}

/// `pub(crate)`: the ai_tasks sleep window export must turn "not measured" back into None by the same rule.
pub(crate) fn loaded_stage_minutes(minutes: i32, available: i64) -> Option<i32> {
    (available != 0).then_some(minutes)
}

/// Columns shared by the sleep list and detail (the order must match [`SleepRow::read`]).
const SLEEP_SESSION_COLUMNS: &str = "sleep_id, start_time, end_time, score, duration_minutes,
    deep_minutes, deep_available, light_minutes, light_available,
    rem_minutes, rem_available, awake_minutes, awake_available,
    source_scope, device_id, synced_at, wake_count";

/// Raw column values of one sleep row. Times and source stay as library strings: parsing happens in [`SleepRow::into_session`],
/// which returns this crate's `Result`. A bad time or bad source still reports `err.core.parse`, and does not become
/// `err.core.database` because it sits inside the `query_map` closure.
struct SleepRow {
    sleep_id: String,
    start: String,
    end: String,
    score: Option<i32>,
    duration_minutes: i32,
    deep: (i32, i64),
    light: (i32, i64),
    rem: (i32, i64),
    awake: (i32, i64),
    scope: String,
    device_id: Option<String>,
    synced_at: Option<String>,
    wake_count: Option<i32>,
}

impl SleepRow {
    fn read(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            sleep_id: row.get(0)?,
            start: row.get(1)?,
            end: row.get(2)?,
            score: row.get(3)?,
            duration_minutes: row.get(4)?,
            deep: (row.get(5)?, row.get(6)?),
            light: (row.get(7)?, row.get(8)?),
            rem: (row.get(9)?, row.get(10)?),
            awake: (row.get(11)?, row.get(12)?),
            scope: row.get(13)?,
            device_id: row.get(14)?,
            synced_at: row.get(15)?,
            wake_count: row.get(16)?,
        })
    }

    fn into_session(self, stages: Vec<SleepStageSlice>) -> Result<SleepSession> {
        Ok(SleepSession {
            start_time: parse_datetime(&self.start, "sleep.start_time")?,
            end_time: parse_datetime(&self.end, "sleep.end_time")?,
            sleep_id: self.sleep_id,
            score: self.score,
            duration_minutes: self.duration_minutes,
            deep_minutes: loaded_stage_minutes(self.deep.0, self.deep.1),
            light_minutes: loaded_stage_minutes(self.light.0, self.light.1),
            rem_minutes: loaded_stage_minutes(self.rem.0, self.rem.1),
            awake_minutes: loaded_stage_minutes(self.awake.0, self.awake.1),
            source_scope: parse_scope(&self.scope)?,
            device_id: self.device_id,
            synced_at: self
                .synced_at
                .as_deref()
                .map(|value| parse_datetime(value, "sleep.synced_at"))
                .transpose()?,
            time_in_bed_minutes: None,
            stages,
            wake_count: self.wake_count,
        })
    }
}

impl Database {
    /// `pub(crate)`: the detailed ai_tasks export attaches the real stage segments, with the same query.
    pub(crate) fn load_sleep_stages(&self, sleep_id: &str) -> Result<Vec<SleepStageSlice>> {
        let mut stmt = self.conn.prepare(
            "SELECT stage, start_time, end_time, raw_mode FROM sleep_stages
             WHERE sleep_id = ?1 ORDER BY start_time, id",
        )?;
        let rows = stmt.query_map([sleep_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<i64>>(3)?,
            ))
        })?;
        let mut stages = Vec::new();
        for row in rows {
            let (stage, start, end, raw_mode) = row?;
            stages.push(SleepStageSlice {
                stage,
                start_time: parse_datetime(&start, "sleep_stages.start_time")?,
                end_time: parse_datetime(&end, "sleep_stages.end_time")?,
                raw_mode,
            });
        }
        Ok(stages)
    }

    /// Nap stage segments for the day this night belongs to (only the official channel gives them). Empty list when there are none.
    pub fn get_sleep_nap_stages(&self, sleep_id: &str) -> Result<Vec<SleepStageSlice>> {
        let mut stmt = self.conn.prepare(
            "SELECT stage, start_time, end_time, raw_mode FROM sleep_nap_stages
             WHERE sleep_id = ?1 ORDER BY start_time, id",
        )?;
        let rows = stmt.query_map([sleep_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<i64>>(3)?,
            ))
        })?;
        let mut stages = Vec::new();
        for row in rows {
            let (stage, start, end, raw_mode) = row?;
            stages.push(SleepStageSlice {
                stage,
                start_time: parse_datetime(&start, "sleep_nap_stages.start_time")?,
                end_time: parse_datetime(&end, "sleep_nap_stages.end_time")?,
                raw_mode,
            });
        }
        Ok(stages)
    }

    /// Total number of sleep records on this machine.
    ///
    /// Paging needs it: without a total, the UI can only say "showing 500", not "2317 in total",
    /// and "where is the rest?" is exactly what users ask.
    pub fn count_sleep_sessions(&self) -> Result<i64> {
        self.conn
            .query_row("SELECT COUNT(*) FROM sleep_sessions_shown", [], |row| {
                row.get(0)
            })
            .map_err(Into::into)
    }

    pub fn get_recent_sleep_sessions(&self, limit: usize) -> Result<Vec<SleepSession>> {
        self.sleep_sessions_page(limit, 0)
    }

    /// One page of sleep records, newest first.
    ///
    /// `offset` is new here. The SQL used to have `LIMIT` and no `OFFSET`, so
    /// the 500 in the UI was a hard cap and not a page size: for a user who downloaded the full history, records after
    /// number 500 had no way in from the app (Reddit p6zxyo7).
    pub fn sleep_sessions_page(&self, limit: usize, offset: usize) -> Result<Vec<SleepSession>> {
        let limit = i64::try_from(limit).unwrap_or(i64::MAX).max(0);
        let offset = i64::try_from(offset).unwrap_or(i64::MAX).max(0);
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {SLEEP_SESSION_COLUMNS}
             FROM sleep_sessions_shown ORDER BY start_time DESC LIMIT ?1 OFFSET ?2"
        ))?;
        let rows = stmt.query_map([limit, offset], SleepRow::read)?;
        let mut sessions = Vec::new();
        for row in rows {
            // The list does not carry stage segments: only the detail page needs them.
            sessions.push(row?.into_session(Vec::new())?);
        }
        Ok(sessions)
    }

    pub fn get_sleep_detail(&self, sleep_id: &str) -> Result<Option<SleepSession>> {
        let row = self
            .conn
            .query_row(
                &format!(
                    "SELECT {SLEEP_SESSION_COLUMNS}
                     FROM sleep_sessions WHERE sleep_id = ?1 LIMIT 1"
                ),
                [sleep_id],
                SleepRow::read,
            )
            .optional()?;
        let Some(mut row) = row else {
            return Ok(None);
        };
        let stages = self.load_sleep_stages(&row.sleep_id)?;
        // The official sleep API gives no device (real payloads hold only start, end, stages and score), so device_id is empty in storage.
        // If the old channel's copy of the same night carries a device, use it: both sides are the same sleep recorded by the same watch.
        if row.device_id.is_none() && row.sleep_id.starts_with("official:") {
            if let Some((scope, device_id)) = self
                .conn
                .query_row(
                    "SELECT source_scope, device_id FROM sleep_sessions
                     WHERE COALESCE(provider, '') != 'official'
                       AND device_id IS NOT NULL AND device_id != ''
                       AND start_time < ?2 AND end_time > ?1
                     ORDER BY ABS(julianday(start_time) - julianday(?1)) LIMIT 1",
                    params![row.start, row.end],
                    |found| {
                        Ok((
                            found.get::<_, String>(0)?,
                            found.get::<_, Option<String>>(1)?,
                        ))
                    },
                )
                .optional()?
            {
                row.scope = scope;
                row.device_id = device_id;
            }
        }
        row.into_session(stages).map(Some)
    }

    /// Total number of workout records on this machine. See `count_sleep_sessions` for the reason.
    pub fn count_workouts(&self) -> Result<i64> {
        self.conn
            .query_row("SELECT COUNT(*) FROM workouts_visible", [], |row| {
                row.get(0)
            })
            .map_err(Into::into)
    }

    pub fn get_recent_workouts(&self, limit: usize) -> Result<Vec<Workout>> {
        self.workouts_page(limit, 0)
    }

    /// One page of workout records, newest first. See `sleep_sessions_page`.
    pub fn workouts_page(&self, limit: usize, offset: usize) -> Result<Vec<Workout>> {
        self.workouts_page_in(limit, offset, false)
    }

    pub(super) fn workouts_page_in(
        &self,
        limit: usize,
        offset: usize,
        hidden: bool,
    ) -> Result<Vec<Workout>> {
        let limit = i64::try_from(limit).unwrap_or(i64::MAX).max(0);
        let offset = i64::try_from(offset).unwrap_or(i64::MAX).max(0);
        let table = if hidden {
            "workouts_hidden"
        } else {
            "workouts_visible"
        };
        let mut stmt = self.conn.prepare(&format!(
            "SELECT workout_id, workout_type, start_time, end_time,
                    distance_meters, calories, avg_hr, max_hr,
                    training_load, vo2max, source_scope, device_id,
                    synced_at, gps_available, sample_count, zepp_type,
                    workout_type_source, workout_type_override, gear_id
             FROM {table} ORDER BY start_time DESC LIMIT ?1 OFFSET ?2",
        ))?;
        let rows = stmt.query_map([limit, offset], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<f64>>(4)?,
                row.get::<_, Option<i32>>(5)?,
                row.get::<_, Option<i32>>(6)?,
                row.get::<_, Option<i32>>(7)?,
                row.get::<_, Option<f64>>(8)?,
                row.get::<_, Option<f64>>(9)?,
                row.get::<_, String>(10)?,
                row.get::<_, Option<String>>(11)?,
                row.get::<_, Option<String>>(12)?,
                row.get::<_, i64>(13)?,
                row.get::<_, i64>(14)?,
                row.get::<_, Option<i32>>(15)?,
                row.get::<_, String>(16)?,
                row.get::<_, Option<String>>(17)?,
                row.get::<_, Option<String>>(18)?,
            ))
        })?;
        // Read the id aliases once, then apply them to each record in memory: the table is small, and this is harder to get wrong
        // than adding a JOIN to each of two large SELECTs.
        let code_labels = self.workout_code_label_map()?;
        let mut workouts = Vec::new();
        for row in rows {
            let (
                workout_id,
                workout_type,
                start,
                end,
                distance_meters,
                calories,
                avg_hr,
                max_hr,
                training_load,
                vo2max,
                scope,
                device_id,
                synced_at,
                gps_available,
                sample_count,
                zepp_type,
                type_source,
                user_override,
                gear_id,
            ) = row?;
            let effective_type = user_override
                .clone()
                .unwrap_or_else(|| workout_type.clone());
            let custom_label = zepp_type.and_then(|code| code_labels.get(&code).cloned());
            workouts.push(Workout {
                workout_id,
                workout_type: workout_type.clone(),
                normalized_type: workout_type,
                type_source,
                user_override,
                effective_type,
                custom_label,
                start_time: parse_datetime(&start, "workout.start_time")?,
                end_time: parse_datetime(&end, "workout.end_time")?,
                distance_meters,
                calories,
                avg_hr,
                max_hr,
                training_load,
                vo2max,
                // The list view does not read these: the screen shows only type, distance, duration and heart rate.
                // Widening the SELECT by thirteen columns plus a join for them costs every list
                // render. For these fields, use the single-workout detail query.
                min_hr: None,
                total_steps: None,
                moving_seconds: None,
                elevation_gain_m: None,
                elevation_loss_m: None,
                max_altitude_m: None,
                min_altitude_m: None,
                training_effect: None,
                anaerobic_training_effect: None,
                rpe: None,
                avg_cadence_spm: None,
                max_cadence_spm: None,
                avg_stride_cm: None,
                // The gear id is only one more column, so the list carries it: grouping by shoe (#143) reads it from the list.
                gear_id,
                hr_zones: Vec::new(),
                source_scope: parse_scope(&scope)?,
                device_id,
                synced_at: synced_at
                    .as_deref()
                    .map(|value| parse_datetime(value, "workout.synced_at"))
                    .transpose()?,
                gps_available: gps_available != 0,
                sample_count,
                zepp_source: None,
                zepp_type,
            });
        }
        Ok(workouts)
    }

    pub fn get_workout_detail(&self, workout_id: &str) -> Result<Option<Workout>> {
        let row = self
            .conn
            .query_row(
                &format!(
                    "SELECT {WORKOUT_DETAIL_COLUMNS} FROM workouts_visible WHERE workout_id = ?1 LIMIT 1"
                ),
                [workout_id],
                WorkoutDetailRow::read,
            )
            .optional()?;
        let Some(row) = row else {
            return Ok(None);
        };
        let hr_zones = self.workout_hr_zones(&row.workout_id)?;
        let route_points: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM route_points WHERE workout_id = ?1",
            [&row.workout_id],
            |row| row.get(0),
        )?;
        let stored_samples: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM workout_samples WHERE workout_id = ?1",
            [&row.workout_id],
            |row| row.get(0),
        )?;
        let custom_label = match row.zepp_type {
            Some(code) => self.workout_code_label_map()?.get(&code).cloned(),
            None => None,
        };
        row.into_workout(WorkoutDetailExtras {
            hr_zones,
            route_points,
            stored_samples,
            custom_label,
        })
        .map(Some)
    }

    pub fn get_health_overview(&self) -> Result<HealthOverview> {
        // Daily metrics use the device's local day: "today" also uses the device time zone (see contract::health_today).
        let today = crate::contract::health_today(self.device_time_zone()?.as_deref(), Utc::now());
        let latest_heart_rate = self.get_latest_heart_rate_sample()?;
        let current_hr = latest_heart_rate.as_ref().map(|(value, _)| *value);
        let latest_heart_rate_at = latest_heart_rate.map(|(_, timestamp)| timestamp);
        let resting_hr = self.latest_daily_i32("resting_hr")?;
        let hrv = self.latest_metric_f64("hrv")?;
        let (last_updated, coverage, source_scope) = self.overview_metadata()?;
        let last_sleep_score = self
            .get_recent_sleep_sessions(1)?
            .into_iter()
            .next()
            .and_then(|sleep| sleep.score);
        Ok(HealthOverview {
            current_hr,
            resting_hr,
            hrv,
            last_sleep_score,
            readiness: self.latest_daily_f64("readiness")?,
            bio_charge: self.latest_daily_f64("bio_charge")?,
            hybrid_charge: self.latest_daily_f64("hybrid_charge")?,
            training_load: self.latest_daily_f64("training_load")?,
            vo2max: self.latest_daily_f64("vo2max")?,
            steps_today: self.latest_daily_i32_for_date("steps", today)?,
            active_calories_today: self
                .latest_daily_i32_for_date("active_calories", today)?
                .or(self.latest_daily_i32_for_date("calories", today)?),
            latest_heart_rate_at,
            last_updated,
            coverage,
            source_scope,
        })
    }

    pub(super) fn overview_metadata(
        &self,
    ) -> Result<(Option<String>, Option<Coverage>, Option<String>)> {
        let last_updated = self.get_app_meta(LAST_CLOUD_SYNC_AT_KEY)?;
        // Earliest/latest local coverage day uses the same path as local_coverage: take
        // MIN/MAX on the raw columns, then convert, so indexes apply. The old code called date() on every row of four tables, then aggregated.
        let (start, end) = self.coverage_day_bounds_in(true)?;
        // Streams = each metric in the sample table + one each for the other three tables. The old code did a UNION of every
        // row of the four tables, then COUNT(DISTINCT), about 90 ms on a real library. Now metric names use a
        // skip scan, and the other three tables only ask "is there any row".
        let mut streams: BTreeSet<String> = self.sample_metric_names()?.into_iter().collect();
        for (stream, table) in [
            ("daily_summary", "daily_metrics"),
            ("sleep", "sleep_sessions"),
            ("workouts", "workouts_visible"),
        ] {
            let present: bool = self.conn.query_row(
                &format!("SELECT EXISTS(SELECT 1 FROM {table})"),
                [],
                |row| row.get(0),
            )?;
            if present {
                streams.insert(stream.to_string());
            }
        }
        let stream_count = streams.len() as i64;
        // Source: we only need to know whether there are 0 kinds, 1 kind (which one) or "mixed". Check the three small tables first.
        // If two kinds are already seen, the sample table is not needed. Otherwise find just one sample "different from the sources seen".
        let mut scopes = BTreeSet::<String>::new();
        for table in ["daily_metrics", "sleep_sessions", "workouts_visible"] {
            let mut statement = self.conn.prepare(&format!(
                "SELECT DISTINCT source_scope FROM {table} WHERE source_scope IS NOT NULL"
            ))?;
            for scope in statement.query_map([], |row| row.get::<_, String>(0))? {
                scopes.insert(scope?);
            }
        }
        if scopes.len() < 2 {
            let other: Option<String> = match scopes.iter().next() {
                Some(seen) => self.conn.query_row(
                    "SELECT source_scope FROM metric_samples
                     WHERE source_scope IS NOT NULL AND source_scope <> ?1 LIMIT 1",
                    [seen],
                    |row| row.get(0),
                ),
                None => self.conn.query_row(
                    "SELECT source_scope FROM metric_samples
                     WHERE source_scope IS NOT NULL LIMIT 1",
                    [],
                    |row| row.get(0),
                ),
            }
            .optional()?;
            if let Some(other) = other {
                scopes.insert(other);
                if scopes.len() < 2 {
                    // The first three tables are empty and the sample table shows one kind: confirm the sample table has no second kind.
                    let seen = scopes.iter().next().cloned().unwrap_or_default();
                    let second: Option<String> = self
                        .conn
                        .query_row(
                            "SELECT source_scope FROM metric_samples
                             WHERE source_scope IS NOT NULL AND source_scope <> ?1 LIMIT 1",
                            [&seen],
                            |row| row.get(0),
                        )
                        .optional()?;
                    scopes.extend(second);
                }
            }
        }
        let scope_count = scopes.len() as i64;
        let only_scope = scopes.iter().next().cloned();
        let coverage = match (start, end) {
            (Some(start), Some(end)) => {
                let start_date = NaiveDate::parse_from_str(&start, "%Y-%m-%d")
                    .map_err(|error| ZeppBridgeError::ParseError(error.to_string()))?;
                let end_date = NaiveDate::parse_from_str(&end, "%Y-%m-%d")
                    .map_err(|error| ZeppBridgeError::ParseError(error.to_string()))?;
                Some(Coverage {
                    start,
                    end,
                    days: (end_date - start_date).num_days() + 1,
                    streams: stream_count,
                })
            }
            _ => None,
        };
        let source_scope = match scope_count {
            0 => None,
            1 => only_scope,
            _ => Some("mixed".to_string()),
        };
        Ok((last_updated, coverage, source_scope))
    }
}
