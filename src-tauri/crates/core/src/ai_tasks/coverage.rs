//! P4 window coverage: each anchor workout opens its own window from its local start day.
//! The export layer dedupes `(category, date)` across overlapping windows. This file only gives totals in the merged row.
//!
//! Time rules (beta1.md P4, same as contract.rs):
//! - Workout anchor day = `date(start_time,'localtime')`. The Rust equivalent is
//!   `start_time.with_timezone(&Local).date_naive()`.
//! - Sleep belongs to the wake-up day, that is `date(end_time,'localtime')`.
//! - Window = `[anchor day - days_before, anchor day]`. With `include_workout_day=false`
//!   the right end is the day before the anchor. `days_before=0` without the day itself gives an empty window, with no output.

use super::export::WindowGather;
use super::model::{AiTaskCategory, AiTaskCategoryRange, AiTaskCoverage};
use super::AiTaskError;
use crate::models::error::{Result, ZeppBridgeError};
use crate::models::{HeartRateZoneBucket, Workout};
use crate::storage::{
    local_day_range_utc_bounds, series_metric_spec, Database, MetricSource, WorkoutDetailExtras,
    WorkoutDetailRow, WORKOUT_DETAIL_COLUMNS,
};
use chrono::{DateTime, Duration, Local, NaiveDate, Utc};
use rusqlite::{params, params_from_iter};
use std::collections::{BTreeMap, BTreeSet};

/// Anchor workouts of a task: the parsed form of `workout_ids`.
///
/// `local_start_day` is the window anchor of this workout. P4 requires one window per workout,
/// so we cannot first merge into a "task-level time range" and then split it.
pub(crate) struct AnchorWorkout {
    pub workout: Workout,
    pub local_start_day: NaiveDate,
}

/// A second private storage parser next to `date(start_time,'localtime')`.
/// `storage::parse_datetime` is private to its module, so this copies the same implementation
/// (RFC3339 -> UTC) with the same error shape.
pub(crate) fn parse_rfc3339_utc(value: &str, field: &str) -> Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|error| ZeppBridgeError::ParseError(format!("{field} is invalid: {error}")))
}

impl Database {
    /// `workout_ids` -> anchors (strict). If any id is not found on this machine, return
    /// `err.ai_task.workout_not_found` with the full missing list in params.
    pub(crate) fn ai_task_anchors(&self, workout_ids: &[String]) -> Result<Vec<AnchorWorkout>> {
        let (anchors, missing) = self.resolve_ai_task_anchors(workout_ids)?;
        if !missing.is_empty() {
            return Err(AiTaskError::workouts_not_found(missing));
        }
        Ok(anchors)
    }

    fn resolve_ai_task_anchors(
        &self,
        workout_ids: &[String],
    ) -> Result<(Vec<AnchorWorkout>, Vec<String>)> {
        let found = self.workout_details_batch(workout_ids)?;
        let mut anchors = Vec::with_capacity(workout_ids.len());
        let mut missing = Vec::new();
        // Anchor order follows the input. The workouts order in preview/export is the user's pick order.
        // get + clone, not remove: when one id repeats, this matches the old per-id query
        // (each lookup returns Some) and the second one does not fall into missing.
        for id in workout_ids {
            match found.get(id) {
                Some(workout) => {
                    // Rust equivalent of `date(start_time,'localtime')` (P4).
                    let local_start_day = workout.start_time.with_timezone(&Local).date_naive();
                    anchors.push(AnchorWorkout {
                        workout: workout.clone(),
                        local_start_day,
                    });
                }
                None => missing.push(id.clone()),
            }
        }
        Ok((anchors, missing))
    }

    /// Batch version of `get_workout_detail`. Calling per id is N+1 with about 5 queries each
    /// (main row, HR zones, route point count, sample count, numbered name table). This sends one
    /// `IN` query for each and maps rows back by id. Row assembly matches `get_workout_detail` field by field.
    /// The two must change together: when you change one, change the other too.
    fn workout_details_batch(&self, workout_ids: &[String]) -> Result<BTreeMap<String, Workout>> {
        let mut details = BTreeMap::new();
        if workout_ids.is_empty() {
            return Ok(details);
        }
        let placeholders = workout_ids
            .iter()
            .map(|_| "?")
            .collect::<Vec<_>>()
            .join(",");
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {WORKOUT_DETAIL_COLUMNS} FROM workouts_visible WHERE workout_id IN ({placeholders})"
        ))?;
        let rows = stmt.query_map(params_from_iter(workout_ids.iter()), WorkoutDetailRow::read)?;
        let mut flat_rows = Vec::new();
        for row in rows {
            flat_rows.push(row?);
        }

        // HR zones: fetch with one IN query, bucket by workout_id and keep zone_index order.
        let mut zones: BTreeMap<String, Vec<HeartRateZoneBucket>> = BTreeMap::new();
        {
            let mut stmt = self.conn.prepare(&format!(
                "SELECT workout_id, zone_index, upper_bound_bpm, seconds
                 FROM workout_hr_zones WHERE workout_id IN ({placeholders})
                 ORDER BY workout_id, zone_index"
            ))?;
            let zone_rows = stmt.query_map(params_from_iter(workout_ids.iter()), |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    HeartRateZoneBucket {
                        index: row.get(1)?,
                        upper_bound_bpm: row.get(2)?,
                        seconds: row.get(3)?,
                    },
                ))
            })?;
            for row in zone_rows {
                let (workout_id, bucket) = row?;
                zones.entry(workout_id).or_default().push(bucket);
            }
        }
        let route_counts = self.count_by_workout(
            &format!(
                "SELECT workout_id, COUNT(*) FROM route_points
                 WHERE workout_id IN ({placeholders}) GROUP BY workout_id"
            ),
            workout_ids,
        )?;
        let sample_counts = self.count_by_workout(
            &format!(
                "SELECT workout_id, COUNT(*) FROM workout_samples
                 WHERE workout_id IN ({placeholders}) GROUP BY workout_id"
            ),
            workout_ids,
        )?;
        let code_labels = self.workout_code_label_map()?;

        for row in flat_rows {
            let workout_id = row.workout_id.clone();
            let extras = WorkoutDetailExtras {
                hr_zones: zones.remove(&workout_id).unwrap_or_default(),
                route_points: route_counts.get(&workout_id).copied().unwrap_or(0),
                stored_samples: sample_counts.get(&workout_id).copied().unwrap_or(0),
                custom_label: row
                    .zepp_type
                    .and_then(|code| code_labels.get(&code).cloned()),
            };
            details.insert(workout_id, row.into_workout(extras)?);
        }
        Ok(details)
    }

    /// Small helper that counts with `SELECT workout_id, COUNT(*) ... GROUP BY workout_id`.
    fn count_by_workout(&self, sql: &str, workout_ids: &[String]) -> Result<BTreeMap<String, i64>> {
        let mut stmt = self.conn.prepare(sql)?;
        let rows = stmt.query_map(params_from_iter(workout_ids.iter()), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;
        let mut counts = BTreeMap::new();
        for row in rows {
            let (workout_id, count) = row?;
            counts.insert(workout_id, count);
        }
        Ok(counts)
    }
}

/// Date window of one task category at one anchor (local days, both ends inclusive). An empty window gives `None`.
///
/// Export (`category_windows`) and MCP task grants (`access::shared_tasks::expand_task`) share this one rule
/// (code review R12: before, a task with no workouts exported a 15-day window but MCP got zero days).
/// `anchor_day` is `None` when the task has no linked workouts. Then "the last N days" is the subject and the window ends today.
/// `include_workout_day` has no meaning here. Today always counts.
pub(crate) fn task_window(
    days_before: i64,
    include_workout_day: bool,
    anchor_day: Option<NaiveDate>,
    today: NaiveDate,
) -> Option<(NaiveDate, NaiveDate)> {
    let days_before = days_before.max(0);
    let Some(day) = anchor_day else {
        return Some((today - Duration::days(days_before), today));
    };
    let end = if include_workout_day {
        day
    } else {
        day - Duration::days(1)
    };
    let start = day - Duration::days(days_before);
    (start <= end).then_some((start, end))
}

/// Window set of one enabled window category across anchors:
/// `(workout_id, start, end)`, local days, both ends inclusive. Empty windows give no output.
///
/// A task with no linked workouts has one window: `[today - days_before, today]`,
/// with `workout_id` as `None`. "The last N days" is the subject itself.
/// `include_workout_day` has no meaning here. Today always counts.
pub(crate) fn category_windows(
    range: &AiTaskCategoryRange,
    anchors: &[AnchorWorkout],
    today: NaiveDate,
) -> Vec<(Option<String>, NaiveDate, NaiveDate)> {
    // Picked days: one window from the earliest to the latest picked day, not tied to workout anchors (gather filters rows by the set).
    let picked: Vec<NaiveDate> = range
        .picked_days
        .iter()
        .filter_map(|day| NaiveDate::parse_from_str(day, "%Y-%m-%d").ok())
        .collect();
    if let (Some(start), Some(end)) = (picked.iter().min(), picked.iter().max()) {
        return vec![(None, *start, *end)];
    }
    if anchors.is_empty() {
        return task_window(range.days_before, range.include_workout_day, None, today)
            .map(|(start, end)| (None, start, end))
            .into_iter()
            .collect();
    }
    anchors
        .iter()
        .filter_map(|anchor| {
            let (start, end) = task_window(
                range.days_before,
                range.include_workout_day,
                Some(anchor.local_start_day),
                today,
            )?;
            Some((Some(anchor.workout.workout_id.clone()), start, end))
        })
        .collect()
}

/// The metric list that one "metric type" category queries.
///
/// Units and sources come from `series_metric_spec` (the only registry of `SERIES_METRICS`).
/// `heart_rate` is not in that table (it uses the special `heart_rate_series` rules),
/// so it is registered here explicitly as `metric_samples`/`bpm`.
pub(crate) struct CategoryMetricSpec {
    pub metric: &'static str,
    pub source: MetricSource,
    pub unit: &'static str,
}

pub(crate) fn category_metric_specs(category: AiTaskCategory) -> Vec<CategoryMetricSpec> {
    const HEART_RATE_SPEC: CategoryMetricSpec = CategoryMetricSpec {
        metric: "heart_rate",
        source: MetricSource::Samples,
        unit: "bpm",
    };
    let names: &[&str] = match category {
        AiTaskCategory::Recovery => &[
            "resting_hr",
            "readiness",
            "physical_readiness",
            "mental_readiness",
            "hybrid_charge",
            "physical_charge",
            "mental_charge",
            "stress",
            "respiratory_rate",
            "sleep_hrv",
            "sleep_rhr",
            "hrv_baseline",
            "rhr_baseline",
            "ahi_baseline",
            "spo2_odi",
            "spo2_night_score",
            "spo2_measured_minutes",
            "hrv",
            "hrv_rmssd",
            "spo2",
        ],
        // Category split (1A A10): all-day HR has only point samples. Resting HR is its own category.
        AiTaskCategory::HeartRate => &["heart_rate"],
        AiTaskCategory::RestingHr => &["resting_hr"],
        AiTaskCategory::Training => &[
            "training_load",
            "vo2max",
            "lactate_threshold_hr",
            "lactate_threshold_pace",
            "pai_daily",
            "pai_total",
            "steps",
            "active_calories",
            "active_minutes",
            // Resting energy: basal metabolism from a body scale. With activity energy beside it, the AI can estimate a day's total burn.
            // The body category has it too. For accounts with no scale this item is "not provided". We do not derive it from other numbers.
            "bmr",
        ],
        AiTaskCategory::Body => &[
            "blood_pressure_systolic",
            "blood_pressure_diastolic",
            "blood_pressure_pulse",
            "weight",
            "bmi",
            "height",
            "body_fat_rate",
            "body_water_rate",
            "muscle_mass",
            "bone_mass",
            "protein_rate",
            "visceral_fat",
            "bmr",
            "body_balance_score",
        ],
        _ => return Vec::new(),
    };
    names
        .iter()
        .filter_map(|name| {
            if *name == "heart_rate" {
                return Some(HEART_RATE_SPEC);
            }
            series_metric_spec(name).map(|(source, unit)| CategoryMetricSpec {
                metric: name,
                source,
                unit,
            })
        })
        .collect()
}

/// Fixed column units for `AiTaskCoverage.units` (workout / sleep do not query the metric table;
/// their fields are fixed).
fn fixed_category_units(category: AiTaskCategory) -> BTreeMap<String, String> {
    let entries: &[(&str, &str)] = match category {
        AiTaskCategory::Workout => &[
            ("distance_meters", "m"),
            ("moving_seconds", "s"),
            ("calories", "kcal"),
            ("avg_hr", "bpm"),
            ("max_hr", "bpm"),
            ("min_hr", "bpm"),
            ("training_load", "load"),
            ("vo2max", "ml/kg/min"),
            ("total_steps", "steps"),
            ("elevation_gain_m", "m"),
            ("elevation_loss_m", "m"),
        ],
        AiTaskCategory::Sleep => &[
            ("duration_minutes", "min"),
            ("score", "score"),
            ("deep_minutes", "min"),
            ("light_minutes", "min"),
            ("rem_minutes", "min"),
            ("awake_minutes", "min"),
            ("wake_count", "count"),
        ],
        _ => &[],
    };
    entries
        .iter()
        .map(|(name, unit)| (name.to_string(), unit.to_string()))
        .collect()
}

/// Metric -> unit map for what a category queries. workout/sleep use fixed column units.
pub(crate) fn category_units(category: AiTaskCategory) -> BTreeMap<String, String> {
    let specs = category_metric_specs(category);
    if specs.is_empty() {
        return fixed_category_units(category);
    }
    specs
        .iter()
        .map(|spec| (spec.metric.to_string(), spec.unit.to_string()))
        .collect()
}

impl Database {
    /// Set of "local days with data" in one window, plus the set of matched `source_scope` values.
    ///
    /// Dedup grain is the P4 `(category, date)`: a day is covered if it has any data of that category
    /// (any **non-excluded** metric, any source).
    pub(crate) fn category_window_days(
        &self,
        category: AiTaskCategory,
        start: &str,
        end: &str,
        excluded: &[String],
    ) -> Result<(BTreeSet<String>, BTreeSet<String>)> {
        let mut days = BTreeSet::new();
        let mut sources = BTreeSet::new();
        match category {
            AiTaskCategory::Workout => {
                let mut stmt = self.conn.prepare(
                    "SELECT date(start_time,'localtime'), source_scope FROM workouts_visible
                     WHERE date(start_time,'localtime') BETWEEN ?1 AND ?2",
                )?;
                let rows = stmt.query_map(params![start, end], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })?;
                for row in rows {
                    let (day, source) = row?;
                    days.insert(day);
                    sources.insert(source);
                }
            }
            AiTaskCategory::Sleep => {
                // Sleep belongs to the wake-up day (same as the insight contract).
                let mut stmt = self.conn.prepare(
                    "SELECT date(end_time,'localtime'), source_scope FROM sleep_sessions_shown
                     WHERE date(end_time,'localtime') BETWEEN ?1 AND ?2",
                )?;
                let rows = stmt.query_map(params![start, end], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })?;
                for row in rows {
                    let (day, source) = row?;
                    days.insert(day);
                    sources.insert(source);
                }
            }
            _ => {
                for spec in category_metric_specs(category) {
                    if excluded.iter().any(|metric| metric == spec.metric) {
                        continue;
                    }
                    match spec.source {
                        MetricSource::Daily(_) => {
                            let mut stmt = self.conn.prepare(
                                "SELECT date, source_scope FROM daily_metrics
                                 WHERE metric = ?1 AND date BETWEEN ?2 AND ?3",
                            )?;
                            let rows = stmt.query_map(params![spec.metric, start, end], |row| {
                                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                            })?;
                            for row in rows {
                                let (day, source) = row?;
                                days.insert(day);
                                sources.insert(source);
                            }
                        }
                        MetricSource::Samples => {
                            // First use padded UTC bounds to hit the `metric_samples(metric, timestamp)`
                            // index, then fold into local days. Same form as sample_metric_points.
                            let bounds = local_day_range_utc_bounds(start, end);
                            let (lower, upper) = match &bounds {
                                Some((lower, upper)) => {
                                    (Some(lower.as_str()), Some(upper.as_str()))
                                }
                                None => (None, None),
                            };
                            let mut stmt = self.conn.prepare(
                                "SELECT date(timestamp,'localtime'), source_scope
                                 FROM metric_samples
                                 WHERE metric = ?1
                                   AND (?4 IS NULL OR timestamp >= ?4)
                                   AND (?5 IS NULL OR timestamp < ?5)
                                   AND date(timestamp,'localtime') BETWEEN ?2 AND ?3",
                            )?;
                            let rows = stmt.query_map(
                                params![spec.metric, start, end, lower, upper],
                                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                            )?;
                            for row in rows {
                                let (day, source) = row?;
                                days.insert(day);
                                sources.insert(source);
                            }
                        }
                        MetricSource::SleepScores => {
                            let mut stmt = self.conn.prepare(
                                "SELECT date(end_time,'localtime'), source_scope FROM sleep_sessions_shown
                                 WHERE score IS NOT NULL AND date(end_time,'localtime') BETWEEN ?1 AND ?2",
                            )?;
                            let rows = stmt.query_map(params![start, end], |row| {
                                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                            })?;
                            for row in rows {
                                let (day, source) = row?;
                                days.insert(day);
                                sources.insert(source);
                            }
                        }
                    }
                }
            }
        }
        Ok((days, sources))
    }
}

/// P2 coverage facts: one row per window, plus one merged `workout_id=null` row when there are several windows
/// (union day count of the windows + data days after date dedup).
///
/// This sends no queries of its own. `gathers` is the result of the same fetch the export build uses, so
/// coverage and the document's days/sources come from the same data (H4 merge).
/// Empty `gathers` gives an empty result. This matches the old rule "a category with all-empty windows gives no row".
pub(crate) fn window_coverage_rows(
    category: AiTaskCategory,
    gathers: &[WindowGather],
) -> Vec<AiTaskCoverage> {
    let units = category_units(category);
    let mut rows = Vec::new();
    let mut union_days = BTreeSet::new();
    let mut union_covered = BTreeSet::new();
    let mut union_sources = BTreeSet::new();
    let mut union_metric_days: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut merged_start: Option<NaiveDate> = None;
    let mut merged_end: Option<NaiveDate> = None;
    let day_counts = |days: &BTreeMap<String, BTreeSet<String>>| -> BTreeMap<String, i64> {
        days.iter()
            .map(|(metric, set)| (metric.clone(), set.len() as i64))
            .collect()
    };

    for gather in gathers {
        match &gather.picked {
            Some(picked) => union_days.extend(picked.iter().cloned()),
            None => union_days.extend(day_set(gather.start, gather.end)),
        }
        union_covered.extend(gather.covered_days.iter().cloned());
        union_sources.extend(gather.sources.iter().cloned());
        for (metric, days) in &gather.metric_days {
            union_metric_days
                .entry(metric.clone())
                .or_default()
                .extend(days.iter().cloned());
        }
        merged_start = Some(merged_start.map_or(gather.start, |d: NaiveDate| d.min(gather.start)));
        merged_end = Some(merged_end.map_or(gather.end, |d: NaiveDate| d.max(gather.end)));
        rows.push(AiTaskCoverage {
            category,
            workout_id: gather.workout_id.clone(),
            start_date: gather.start.to_string(),
            end_date: gather.end.to_string(),
            // Picked days: the range is the picked days, not the first-to-last span.
            days_in_range: gather
                .picked
                .as_ref()
                .map_or((gather.end - gather.start).num_days() + 1, |picked| {
                    picked.len() as i64
                }),
            days_with_data: gather.covered_days.len() as i64,
            covered_dates: gather.covered_days.iter().cloned().collect(),
            sources: gather.sources.iter().cloned().collect(),
            units: units.clone(),
            metric_days: day_counts(&gather.metric_days),
            missing: gather.covered_days.is_empty(),
            picked_dates: gather
                .picked
                .as_ref()
                .map(|picked| picked.iter().cloned().collect())
                .unwrap_or_default(),
        });
    }

    if gathers.len() > 1 {
        if let (Some(start), Some(end)) = (merged_start, merged_end) {
            rows.push(AiTaskCoverage {
                category,
                workout_id: None,
                start_date: start.to_string(),
                end_date: end.to_string(),
                // Union coverage days. Windows can have gaps, so the span is not the day count.
                days_in_range: union_days.len() as i64,
                days_with_data: union_covered.len() as i64,
                covered_dates: union_covered.iter().cloned().collect(),
                sources: union_sources.into_iter().collect(),
                units,
                metric_days: day_counts(&union_metric_days),
                missing: union_covered.is_empty(),
                picked_dates: Vec::new(),
            });
        }
    }
    rows
}

/// `YYYY-MM-DD` for each day in the closed range `[start, end]`.
fn day_set(start: NaiveDate, end: NaiveDate) -> BTreeSet<String> {
    let mut days = BTreeSet::new();
    let mut day = start;
    while day <= end {
        days.insert(day.to_string());
        day += Duration::days(1);
    }
    days
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::SourceScope;
    use chrono::TimeZone;

    fn at(y: i32, m: u32, d: u32, h: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, m, d, h, 0, 0).unwrap()
    }

    /// Three invariants of the batch anchor path: anchor order follows the input, missing ids go into missing,
    /// and each Workout matches `get_workout_detail` in all fields (including hr_zones /
    /// gps_available and sample_count derived from route and sample counts).
    #[test]
    fn anchors_batch_matches_get_workout_detail_and_keeps_input_order() {
        let db = Database::in_memory().unwrap();
        let start_a = at(2026, 9, 10, 10);
        let start_b = at(2026, 9, 12, 6);
        db.insert_workout(&Workout {
            workout_id: "w-a".into(),
            workout_type: "running".into(),
            normalized_type: "running".into(),
            type_source: "string_field".into(),
            effective_type: "running".into(),
            start_time: start_a,
            end_time: start_a + Duration::hours(1),
            distance_meters: Some(10_000.0),
            calories: Some(600),
            avg_hr: Some(140),
            max_hr: Some(170),
            hr_zones: vec![
                HeartRateZoneBucket {
                    index: 0,
                    upper_bound_bpm: 120,
                    seconds: 300,
                },
                HeartRateZoneBucket {
                    index: 1,
                    upper_bound_bpm: 150,
                    seconds: 600,
                },
            ],
            source_scope: SourceScope::UserFused,
            ..Default::default()
        })
        .unwrap();
        db.insert_workout(&Workout {
            workout_id: "w-b".into(),
            workout_type: "walking".into(),
            normalized_type: "walking".into(),
            type_source: "string_field".into(),
            effective_type: "walking".into(),
            start_time: start_b,
            end_time: start_b + Duration::minutes(40),
            source_scope: SourceScope::Device,
            ..Default::default()
        })
        .unwrap();
        // Write raw rows directly so the batch branch's COUNT(route_points)/COUNT(workout_samples)
        // really count something. They feed gps_available and sample_count.
        db.conn
            .execute(
                "INSERT INTO route_points (workout_id, timestamp, latitude, longitude, altitude)
                 VALUES ('w-b', '2026-09-12T06:01:00+00:00', 31.0, 121.0, NULL)",
                [],
            )
            .unwrap();
        db.conn
            .execute(
                "INSERT INTO workout_samples (workout_id, timestamp) VALUES ('w-b', '2026-09-12T06:01:00+00:00')",
                [],
            )
            .unwrap();

        // The order is the reverse of insertion on purpose, with one id that does not exist mixed in.
        let (anchors, missing) = db
            .resolve_ai_task_anchors(&["w-b".to_string(), "w-a".to_string(), "ghost".to_string()])
            .unwrap();
        assert_eq!(missing, vec!["ghost".to_string()]);
        let ids: Vec<&str> = anchors
            .iter()
            .map(|anchor| anchor.workout.workout_id.as_str())
            .collect();
        assert_eq!(ids, ["w-b", "w-a"], "anchor order must follow the input");

        for anchor in &anchors {
            let detail = db
                .get_workout_detail(&anchor.workout.workout_id)
                .unwrap()
                .unwrap();
            assert_eq!(
                serde_json::to_value(&anchor.workout).unwrap(),
                serde_json::to_value(&detail).unwrap(),
                "{}: batch row must match get_workout_detail in all fields",
                anchor.workout.workout_id
            );
            assert_eq!(
                anchor.local_start_day,
                anchor.workout.start_time.with_timezone(&Local).date_naive()
            );
        }
        // Guard against a false pass where both sides are wrong: the count subqueries really reached the fields.
        assert!(anchors[0].workout.gps_available);
        assert!(anchors[0].workout.sample_count >= 1);
        assert_eq!(anchors[1].workout.hr_zones.len(), 2);
    }

    /// A bad time gives a parse error (err.core.parse) on both paths. It is not swallowed and no fake time is filled in.
    #[test]
    fn corrupt_start_time_is_a_parse_error_on_both_paths() {
        let db = Database::in_memory().unwrap();
        let start = at(2026, 9, 10, 10);
        db.insert_workout(&Workout {
            workout_id: "w-bad".into(),
            workout_type: "running".into(),
            normalized_type: "running".into(),
            type_source: "string_field".into(),
            effective_type: "running".into(),
            start_time: start,
            end_time: start + Duration::hours(1),
            source_scope: SourceScope::Device,
            ..Default::default()
        })
        .unwrap();
        db.conn
            .execute(
                "UPDATE workouts SET start_time = 'not-a-time' WHERE workout_id = 'w-bad'",
                [],
            )
            .unwrap();
        assert!(matches!(
            db.get_workout_detail("w-bad"),
            Err(ZeppBridgeError::ParseError(_))
        ));
        assert!(matches!(
            db.resolve_ai_task_anchors(&["w-bad".to_string()]),
            Err(ZeppBridgeError::ParseError(_))
        ));
    }
}
