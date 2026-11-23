//! Collects data in time windows by category: workout day rows, sleep day rows, stages (split out of ai_tasks/export.rs, logic unchanged).

use super::*;

impl Database {
    /// One pass over all anchor windows for one window category. Each window produces one
    /// [`WindowGather`]. The coverage days/sources and the document day rows
    /// accumulate here together, so the two sides do not each query again.
    pub(super) fn gather_category_windows(
        &self,
        task: &AiTask,
        range: &AiTaskCategoryRange,
        anchors: &[AnchorWorkout],
    ) -> Result<Vec<WindowGather>> {
        let mut gathers = Vec::new();
        let excluded = &range.excluded_metrics;
        let is_excluded = |name: &str| excluded.iter().any(|metric| metric == name);
        let field_units = category_units(range.category);
        // Picked days: the window spans first to last day. Days not picked are filtered out after the full window loads (rows, coverage and per-metric day counts together).
        let picked: Option<BTreeSet<String>> =
            (!range.picked_days.is_empty()).then(|| range.picked_days.iter().cloned().collect());
        for (workout_id, start, end) in category_windows(range, anchors, Local::now().date_naive())
        {
            let start_text = start.to_string();
            let end_text = end.to_string();
            let mut gather = WindowGather {
                workout_id,
                picked: picked.clone(),
                start,
                end,
                covered_days: BTreeSet::new(),
                sources: BTreeSet::new(),
                metric_days: BTreeMap::new(),
                rows: Vec::new(),
            };
            // Workout/sleep: one row is one record, and the metrics are its fields. Count days per field.
            // Excluded fields are removed from the output object (the record itself stays).
            let push_record = |gather: &mut WindowGather,
                               day: String,
                               key: String,
                               source_scope: String,
                               mut row: Value| {
                if let Value::Object(object) = &mut row {
                    for field in field_units.keys() {
                        if object.contains_key(field) {
                            gather
                                .metric_days
                                .entry(field.clone())
                                .or_default()
                                .insert(day.clone());
                        }
                        if is_excluded(field) {
                            object.remove(field);
                        }
                    }
                    // When stage minutes are excluded, the stage timeline also stays out, because the segments give back the minutes.
                    // Same table as MCP `access::project_sleep_fields`.
                    if crate::access::SLEEP_STAGE_FIELDS
                        .iter()
                        .any(|field| is_excluded(field))
                    {
                        object.remove("stages");
                    }
                }
                gather.covered_days.insert(day.clone());
                gather.sources.insert(source_scope);
                gather.rows.push((day, key, row));
            };
            match range.category {
                AiTaskCategory::Workout => {
                    for (day, row_id, source_scope, row) in
                        self.workout_day_rows(&start_text, &end_text)?
                    {
                        push_record(&mut gather, day, format!("w:{row_id}"), source_scope, row);
                    }
                }
                AiTaskCategory::Sleep => {
                    for (day, row_id, source_scope, row) in self.sleep_day_rows(
                        &start_text,
                        &end_text,
                        task.detail_level == AiTaskDetailLevel::Detailed,
                    )? {
                        push_record(&mut gather, day, format!("s:{row_id}"), source_scope, row);
                    }
                }
                _ => {
                    // Metric category output rows have no source_scope. Coverage still uses
                    // the days+sources query of `category_window_days`, with the same form and result as
                    // the old version. Document rows still take points by spec.
                    let (days, sources) = self.category_window_days(
                        range.category,
                        &start_text,
                        &end_text,
                        excluded,
                    )?;
                    gather.covered_days = days;
                    gather.sources = sources;
                    for spec in category_metric_specs(range.category) {
                        let points = match spec.source {
                            MetricSource::Daily(spread) => self.daily_metric_points(
                                spec.metric,
                                spread,
                                &start_text,
                                &end_text,
                            )?,
                            MetricSource::Samples => {
                                self.sample_metric_points(spec.metric, &start_text, &end_text)?
                            }
                            MetricSource::SleepScores => {
                                self.sleep_score_points(&start_text, &end_text)?
                            }
                        };
                        for point in points {
                            gather
                                .metric_days
                                .entry(spec.metric.to_string())
                                .or_default()
                                .insert(point.date.clone());
                            if is_excluded(spec.metric) {
                                continue;
                            }
                            let mut metric = Map::new();
                            metric.insert("metric".into(), json!(spec.metric));
                            metric.insert("unit".into(), json!(spec.unit));
                            metric.insert("value".into(), json!(point.value));
                            if let Some(min) = point.min {
                                metric.insert("min".into(), json!(min));
                            }
                            if let Some(max) = point.max {
                                metric.insert("max".into(), json!(max));
                            }
                            if let Some(samples) = point.samples {
                                metric.insert("samples".into(), json!(samples));
                            }
                            gather.rows.push((
                                point.date,
                                spec.metric.to_string(),
                                Value::Object(metric),
                            ));
                        }
                    }
                }
            }
            if let Some(picked) = &picked {
                gather.rows.retain(|(day, _, _)| picked.contains(day));
                gather.covered_days.retain(|day| picked.contains(day));
                for days in gather.metric_days.values_mut() {
                    days.retain(|day| picked.contains(day));
                }
                gather.metric_days.retain(|_, days| !days.is_empty());
            }
            gathers.push(gather);
        }
        Ok(gathers)
    }

    /// Workout day entries in the window. Returns `(local day, workout_id, source_scope, output
    /// object)`. `workout_id` is separate so overlapping windows can deduplicate. `source_scope` goes into both
    /// the coverage sources and the output object. `workout_type` uses the effective value,
    /// where the user correction wins.
    pub(super) fn workout_day_rows(
        &self,
        start: &str,
        end: &str,
    ) -> Result<Vec<(String, String, String, Value)>> {
        let mut stmt = self.conn.prepare(
            "SELECT workout_id, COALESCE(workout_type_override, workout_type),
                    start_time, end_time, distance_meters, moving_seconds,
                    calories, avg_hr, max_hr, min_hr, training_load, vo2max,
                    source_scope, date(start_time,'localtime')
             FROM workouts_visible
             WHERE date(start_time,'localtime') BETWEEN ?1 AND ?2
             ORDER BY start_time",
        )?;
        let rows = stmt.query_map(params![start, end], |row| {
            let workout_id: String = row.get(0)?;
            let source_scope: String = row.get(12)?;
            let mut object = Map::new();
            object.insert("workout_id".into(), json!(workout_id));
            object.insert("workout_type".into(), json!(row.get::<_, String>(1)?));
            object.insert("start_time".into(), json!(row.get::<_, String>(2)?));
            object.insert("end_time".into(), json!(row.get::<_, String>(3)?));
            for (index, key) in [
                (4, "distance_meters"),
                (5, "moving_seconds"),
                (6, "calories"),
                (7, "avg_hr"),
                (8, "max_hr"),
                (9, "min_hr"),
                (10, "training_load"),
                (11, "vo2max"),
            ] {
                if let Some(value) = row.get::<_, Option<f64>>(index)? {
                    object.insert(key.into(), json!(value));
                }
            }
            object.insert("source_scope".into(), json!(source_scope.clone()));
            Ok((
                row.get::<_, String>(13)?,
                workout_id,
                source_scope,
                Value::Object(object),
            ))
        })?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    /// Sleep day entries in the window (owned by the wake-up day). Returns `(owning day, sleep_id,
    /// source_scope, output object)`. `device_id` does not go into the document.
    pub(super) fn sleep_day_rows(
        &self,
        start: &str,
        end: &str,
        include_stages: bool,
    ) -> Result<Vec<(String, String, String, Value)>> {
        let mut stmt = self.conn.prepare(
            "SELECT sleep_id, start_time, end_time, score, duration_minutes,
                    deep_minutes, deep_available, light_minutes, light_available,
                    rem_minutes, rem_available, awake_minutes, awake_available,
                    source_scope, wake_count, date(end_time,'localtime')
             FROM sleep_sessions_shown
             WHERE date(end_time,'localtime') BETWEEN ?1 AND ?2
             ORDER BY end_time",
        )?;
        let mapped = stmt.query_map(params![start, end], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<i32>>(3)?,
                row.get::<_, i32>(4)?,
                row.get::<_, i32>(5)?,
                row.get::<_, i64>(6)?,
                row.get::<_, i32>(7)?,
                row.get::<_, i64>(8)?,
                row.get::<_, i32>(9)?,
                row.get::<_, i64>(10)?,
                row.get::<_, i32>(11)?,
                row.get::<_, i64>(12)?,
                row.get::<_, String>(13)?,
                row.get::<_, Option<i32>>(14)?,
                row.get::<_, String>(15)?,
            ))
        })?;
        let mut raw_rows = Vec::new();
        for row in mapped {
            raw_rows.push(row?);
        }
        // Detailed stage segments load in one IN query. A per-session `load_sleep_stages`
        // is a classic N+1 (one query per night in the window).
        let stage_map = if include_stages {
            self.load_sleep_stages_batch(
                &raw_rows.iter().map(|row| row.0.clone()).collect::<Vec<_>>(),
            )?
        } else {
            BTreeMap::new()
        };

        let mut rows: Vec<(String, String, String, Value)> = Vec::new();
        for (
            sleep_id,
            start_time,
            end_time,
            score,
            duration_minutes,
            deep_minutes,
            deep_available,
            light_minutes,
            light_available,
            rem_minutes,
            rem_available,
            awake_minutes,
            awake_available,
            source_scope,
            wake_count,
            day,
        ) in raw_rows
        {
            let mut object = Map::new();
            object.insert("sleep_id".into(), json!(sleep_id));
            object.insert("start_time".into(), json!(start_time));
            object.insert("end_time".into(), json!(end_time));
            if let Some(value) = score {
                object.insert("score".into(), json!(value));
            }
            object.insert("duration_minutes".into(), json!(duration_minutes));
            for (key, value) in [
                (
                    "deep_minutes",
                    loaded_stage_minutes(deep_minutes, deep_available),
                ),
                (
                    "light_minutes",
                    loaded_stage_minutes(light_minutes, light_available),
                ),
                (
                    "rem_minutes",
                    loaded_stage_minutes(rem_minutes, rem_available),
                ),
                (
                    "awake_minutes",
                    loaded_stage_minutes(awake_minutes, awake_available),
                ),
            ] {
                if let Some(value) = value {
                    object.insert(key.into(), json!(value));
                }
            }
            if let Some(value) = wake_count {
                object.insert("wake_count".into(), json!(value));
            }
            object.insert("source_scope".into(), json!(source_scope));
            if include_stages {
                if let Some(stages) = stage_map.get(&sleep_id) {
                    if !stages.is_empty() {
                        object.insert("stages".into(), serde_json::to_value(stages)?);
                    }
                }
            }
            rows.push((day, sleep_id, source_scope, Value::Object(object)));
        }
        Ok(rows)
    }

    /// Batched version of `load_sleep_stages`: load once with `sleep_id IN (...)`, then bucket by
    /// sleep_id. The order in each bucket matches the single version (`ORDER BY start_time, id`).
    pub(super) fn load_sleep_stages_batch(
        &self,
        sleep_ids: &[String],
    ) -> Result<BTreeMap<String, Vec<SleepStageSlice>>> {
        let mut map: BTreeMap<String, Vec<SleepStageSlice>> = BTreeMap::new();
        if sleep_ids.is_empty() {
            return Ok(map);
        }
        let placeholders = sleep_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let mut stmt = self.conn.prepare(&format!(
            "SELECT sleep_id, stage, start_time, end_time, raw_mode
             FROM sleep_stages WHERE sleep_id IN ({placeholders})
             ORDER BY sleep_id, start_time, id"
        ))?;
        let rows = stmt.query_map(params_from_iter(sleep_ids.iter()), |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<i64>>(4)?,
            ))
        })?;
        for row in rows {
            let (sleep_id, stage, start, end, raw_mode) = row?;
            map.entry(sleep_id).or_default().push(SleepStageSlice {
                stage,
                start_time: parse_rfc3339_utc(&start, "sleep_stages.start_time")?,
                end_time: parse_rfc3339_utc(&end, "sleep_stages.end_time")?,
                raw_mode,
            });
        }
        Ok(map)
    }
}
