//! Assembles the task bundle: prompt, task document, anchor workouts (split out of ai_tasks/export.rs, logic unchanged).

use super::workout_extras::{climbs_json, comparison_json, strip_heart_rate};
use super::*;

impl Database {
    /// Shared builder for preview/prepare: coverage facts, attachment status, warnings, output document.
    ///
    /// The caller stats `attachments` first and passes them in. The prepare blocked check
    /// and the bundle use the same attachment status, so they do not each stat again.
    pub(super) fn build_ai_task_bundle(
        &self,
        task: &AiTask,
        anchors: &[AnchorWorkout],
        attachments: Vec<AiTaskAttachmentStatus>,
    ) -> Result<AiTaskBundle> {
        // Load each enabled window category once: the same window result first builds the coverage
        // facts, then goes into the document context day rows. Two query passes become one.
        let mut coverage = Vec::new();
        let mut sections: Vec<(AiTaskCategory, Vec<WindowGather>)> = Vec::new();
        for range in &task.categories {
            if !range.enabled || !range.category.is_windowed() {
                continue;
            }
            let windows = self.gather_category_windows(task, range, anchors)?;
            coverage.extend(window_coverage_rows(range.category, &windows));
            sections.push((range.category, windows));
        }
        let briefs: Vec<AiTaskWorkoutBrief> = anchors
            .iter()
            .map(|anchor| AiTaskWorkoutBrief {
                workout_id: anchor.workout.workout_id.clone(),
                workout_type: anchor.workout.effective_type.clone(),
                start_time: anchor.workout.start_time.to_rfc3339(),
                end_time: anchor.workout.end_time.to_rfc3339(),
                start_date: anchor.local_start_day.to_string(),
                distance_meters: anchor.workout.distance_meters,
                calories: anchor.workout.calories,
                avg_hr: anchor.workout.avg_hr,
                max_hr: anchor.workout.max_hr,
            })
            .collect();
        let warnings = task_warnings(task, &coverage, &attachments);
        let document =
            self.build_task_document(task, anchors, &coverage, &attachments, sections)?;
        Ok(AiTaskBundle {
            document,
            briefs,
            coverage,
            attachments,
            warnings,
        })
    }

    /// Prompt assembly: task instructions + analysis direction (template) + user question + coverage notes. Each part can be empty.
    ///
    /// Task instructions go first: with no question and no template, the prompt must still say clearly
    /// what the AI must do (before, only "do not guess missing data" was left, so the AI could only ask back). When the user
    /// edited the final prompt (`override_text`), that full text replaces the first three parts. Coverage notes are still appended.
    ///
    /// The template is the general direction and the question is this time's focus. Both are **kept**, not one or the other.
    /// The direction part uses the frontend-localized `parts.direction` first. When not passed (CLI and other old callers)
    /// it falls back to the template's own `prompt_template`. The backend does not produce copy.
    pub(super) fn assemble_task_prompt(
        &self,
        task: &AiTask,
        coverage_note: &str,
        parts: &AiTaskPromptParts<'_>,
    ) -> Result<String> {
        let join = |sections: &[String]| {
            sections
                .iter()
                .filter(|part| !part.is_empty())
                .cloned()
                .collect::<Vec<_>>()
                .join("\n\n")
        };
        if let Some(text) = parts
            .override_text
            .map(str::trim)
            .filter(|text| !text.is_empty())
        {
            return Ok(join(&[
                sanitize_export_text(text),
                coverage_note.trim().to_string(),
            ]));
        }
        let direction = match parts
            .direction
            .map(str::trim)
            .filter(|text| !text.is_empty())
        {
            Some(text) => text.to_string(),
            None => match task.template_id.as_deref() {
                Some(template_id) => self
                    .get_ai_task_template(template_id)?
                    .map(|template| template.prompt_template.trim().to_string())
                    .unwrap_or_default(),
                None => String::new(),
            },
        };
        // User-written text (own prompt / user template prompt_template) goes through path cleaning
        // before output, because this text goes into prompt.txt for an external AI.
        let sections = [
            parts.brief.map(str::trim).unwrap_or_default().to_string(),
            sanitize_export_text(&direction),
            sanitize_export_text(task.prompt.trim()),
            coverage_note.trim().to_string(),
        ];
        Ok(join(&sections))
    }

    /// Full document for `health-context.json`. It is clean when built: fields are picked one by one,
    /// and `device_id`/paths/identity keys never appear in the document.
    ///
    /// `sections` is the data per window category already loaded when the bundle was built. The document and
    /// coverage share the same query result and do not query again.
    pub(super) fn build_task_document(
        &self,
        task: &AiTask,
        anchors: &[AnchorWorkout],
        coverage: &[AiTaskCoverage],
        attachments: &[AiTaskAttachmentStatus],
        sections: Vec<(AiTaskCategory, Vec<WindowGather>)>,
    ) -> Result<Value> {
        let mut document = Map::new();
        document.insert("schema".into(), json!("zeppbridge.ai_task"));
        document.insert("schema_version".into(), json!(AI_TASK_SCHEMA_VERSION));
        document.insert("generated_at".into(), json!(Utc::now().to_rfc3339()));

        // Task metadata: the prompt does not go into JSON. It is in prompt.txt / prompt_text.
        let mut task_meta = Map::new();
        task_meta.insert("id".into(), json!(effective_task_id(task)));
        task_meta.insert("title".into(), json!(task.title));
        task_meta.insert(
            "detail_level".into(),
            serde_json::to_value(task.detail_level)?,
        );
        task_meta.insert("template_id".into(), json!(task.template_id));
        task_meta.insert(
            "personal_note".into(),
            json!(sanitize_export_text(&task.personal_note)),
        );
        task_meta.insert(
            "include_precise_gps".into(),
            json!(task.include_precise_gps),
        );
        // Window anchor: `workouts` = look back from each workout start day. `today` = no linked workout,
        // so the analysis covers the last N days up to today.
        task_meta.insert(
            "window_anchor".into(),
            json!(if anchors.is_empty() {
                "today"
            } else {
                "workouts"
            }),
        );
        document.insert("task".into(), Value::Object(task_meta));

        // Anchor workouts: depth follows detail_level. With the "Workouts" category off, anchors only set the date window,
        // and not even id / type / start day is given: `workouts` is empty,
        // day rows have empty `linked_workout_ids`, and coverage rows have `workout_id` cleared to the merged view.
        let workouts_shared = task
            .categories
            .iter()
            .any(|range| range.category == AiTaskCategory::Workout && range.enabled);
        let mut workouts = Vec::with_capacity(anchors.len());
        if workouts_shared {
            for anchor in anchors {
                workouts.push(self.anchor_workout_json(&anchor.workout, task)?);
            }
        }
        document.insert("workouts".into(), Value::Array(workouts));

        // Context: one group of day entries per enabled window category, (category,date) already deduplicated.
        // An empty-window category (all windows empty, no rows) still produces a `days: []` section, same as the old version.
        let mut context = Vec::new();
        for (category, gathers) in sections {
            let days = merge_window_gathers(category, gathers);
            let mut day_entries = Vec::with_capacity(days.len());
            for (date, acc) in days {
                let mut entry = Map::new();
                entry.insert("date".into(), json!(date));
                let linked: Vec<String> = if workouts_shared {
                    acc.linked.into_iter().collect()
                } else {
                    Vec::new()
                };
                entry.insert("linked_workout_ids".into(), json!(linked));
                if !acc.metrics.is_empty() {
                    entry.insert("metrics".into(), Value::Array(acc.metrics));
                }
                if !acc.sleeps.is_empty() {
                    entry.insert("sleeps".into(), Value::Array(acc.sleeps));
                }
                if !acc.workouts.is_empty() {
                    entry.insert("workouts".into(), Value::Array(acc.workouts));
                }
                day_entries.push(Value::Object(entry));
            }
            let mut section = Map::new();
            section.insert("category".into(), serde_json::to_value(category)?);
            section.insert("days".into(), Value::Array(day_entries));
            context.push(Value::Object(section));
        }
        document.insert("context".into(), Value::Array(context));

        if workouts_shared {
            document.insert("coverage".into(), serde_json::to_value(coverage)?);
        } else {
            document.insert(
                "coverage".into(),
                serde_json::to_value(coverage_without_anchors(coverage))?,
            );
        }
        if task
            .categories
            .iter()
            .any(|r| r.category == AiTaskCategory::Body && r.enabled)
        {
            let windows: Vec<_> = coverage
                .iter()
                .filter(|r| r.category == AiTaskCategory::Body)
                .collect();
            let food: Vec<_> = self
                .food_entries(1825)?
                .into_iter()
                .filter(|e| {
                    windows
                        .iter()
                        .any(|w| e.date >= w.start_date && e.date <= w.end_date)
                })
                .collect();
            if !food.is_empty() {
                document.insert("food".into(), serde_json::to_value(food)?);
            }
        }
        if task
            .categories
            .iter()
            .any(|r| r.category == AiTaskCategory::Workout && r.enabled)
        {
            let mut comparisons = BTreeMap::new();
            for window in coverage
                .iter()
                .filter(|r| r.category == AiTaskCategory::Workout)
            {
                let from = NaiveDate::parse_from_str(&window.start_date, "%Y-%m-%d");
                let to = NaiveDate::parse_from_str(&window.end_date, "%Y-%m-%d");
                if let (Ok(from), Ok(to)) = (from, to) {
                    let rows = self.plan_adherence(from, to)?;
                    if rows.iter().any(|r| r.planned.is_some()) {
                        for row in rows.into_iter().filter(|r| r.verdict != "none") {
                            comparisons.insert(row.date.clone(), row);
                        }
                    }
                }
            }
            if !comparisons.is_empty() {
                document.insert(
                    "plan_adherence".into(),
                    serde_json::to_value(comparisons.into_values().collect::<Vec<_>>())?,
                );
            }
        }
        // Coach profile: long-term facts the user confirmed. Not health data, so category switches do not apply.
        let coach_notes: Vec<Value> = self
            .accepted_coach_notes()?
            .into_iter()
            .map(|note| {
                json!({
                    "text": note.text,
                    "source": note.source,
                    "source_name": note.source_name,
                    "since": note.decided_at.unwrap_or(note.created_at).chars().take(10).collect::<String>(),
                })
            })
            .collect();
        if !coach_notes.is_empty() {
            document.insert("coach_notes".into(), Value::Array(coach_notes));
        }
        // History: the user's own PBs / injuries / races / shoes / eras. Like the coach profile, category switches do not apply.
        let journal = self.journal_for_ai()?;
        if !journal.is_empty() {
            document.insert("journal".into(), serde_json::to_value(&journal)?);
        }
        if self.is_demo_library()? {
            document.insert("simulated_data".into(), json!(true));
        }

        // Attachments list only the output form (display_name/kind/byte_len).
        let public_refs: Vec<AiTaskAttachmentPublicRef> = task
            .attachments
            .iter()
            .map(AiTaskAttachmentPublicRef::from)
            .collect();
        let _ = attachments; // Status is already in the preview/prepare result. The document holds only PublicRef.
        document.insert("attachments".into(), serde_json::to_value(public_refs)?);

        // Document-level unit table: fixed field units for anchor workouts and sleep entries. Metric units are
        // inline on each metric entry.
        let mut units = Map::new();
        for (field, unit) in [
            ("distance_meters", "m"),
            ("moving_seconds", "s"),
            ("calories", "kcal"),
            ("avg_hr", "bpm"),
            ("max_hr", "bpm"),
            ("min_hr", "bpm"),
            ("training_load", "load"),
            ("vo2max", "ml/kg/min"),
            ("training_effect", "score"),
            ("anaerobic_training_effect", "score"),
            ("rpe", "score"),
            ("total_steps", "steps"),
            ("avg_cadence_spm", "spm"),
            ("max_cadence_spm", "spm"),
            ("avg_stride_cm", "cm"),
            ("elevation_gain_m", "m"),
            ("elevation_loss_m", "m"),
            ("max_altitude_m", "m"),
            ("min_altitude_m", "m"),
            ("duration_minutes", "min"),
            ("deep_minutes", "min"),
            ("light_minutes", "min"),
            ("rem_minutes", "min"),
            ("awake_minutes", "min"),
            ("wake_count", "count"),
            ("pace", "min/km"),
            ("speed", "m/s"),
            ("cadence", "spm"),
            ("power_watts", "W"),
            ("altitude_m", "m"),
            ("latitude", "deg"),
            ("longitude", "deg"),
        ] {
            units.insert(field.to_string(), json!(unit));
        }
        document.insert("units".into(), Value::Object(units));

        Ok(Value::Object(document))
    }

    /// Output object for an anchor workout. Fields are picked from [`Workout`] one by one. Internal columns such as `device_id`,
    /// `synced_at` and `zepp_*` stay out. Called only when the "Workouts" category is on
    /// (when it is off, no anchor field goes out, see [`Self::build_task_document`]).
    pub(super) fn anchor_workout_json(&self, workout: &Workout, task: &AiTask) -> Result<Value> {
        let excluded = task
            .categories
            .iter()
            .find(|range| range.category == AiTaskCategory::Workout && range.enabled)
            .map(|range| range.excluded_metrics.as_slice())
            .unwrap_or_default();
        let mut object = self.anchor_workout_full(workout, task)?;
        strip_excluded_workout_fields(&mut object, excluded);
        Ok(Value::Object(object))
    }

    fn anchor_workout_full(&self, workout: &Workout, task: &AiTask) -> Result<Map<String, Value>> {
        let mut object = Map::new();
        object.insert("workout_id".into(), json!(workout.workout_id));
        object.insert("workout_type".into(), json!(workout.effective_type));
        object.insert("start_time".into(), json!(workout.start_time.to_rfc3339()));
        object.insert("end_time".into(), json!(workout.end_time.to_rfc3339()));
        if let Some(value) = workout.distance_meters {
            object.insert("distance_meters".into(), json!(value));
        }
        if let Some(value) = workout.calories {
            object.insert("calories".into(), json!(value));
        }
        if let Some(value) = workout.avg_hr {
            object.insert("avg_hr".into(), json!(value));
        }
        if let Some(value) = workout.max_hr {
            object.insert("max_hr".into(), json!(value));
        }

        if task.detail_level == AiTaskDetailLevel::Summary {
            return Ok(object);
        }

        // From standard: full summary columns.
        for (key, value) in [
            ("min_hr", workout.min_hr.map(|v| json!(v))),
            ("total_steps", workout.total_steps.map(|v| json!(v))),
            ("moving_seconds", workout.moving_seconds.map(|v| json!(v))),
            (
                "elevation_gain_m",
                workout.elevation_gain_m.map(|v| json!(v)),
            ),
            (
                "elevation_loss_m",
                workout.elevation_loss_m.map(|v| json!(v)),
            ),
            ("max_altitude_m", workout.max_altitude_m.map(|v| json!(v))),
            ("min_altitude_m", workout.min_altitude_m.map(|v| json!(v))),
            ("training_load", workout.training_load.map(|v| json!(v))),
            ("vo2max", workout.vo2max.map(|v| json!(v))),
            ("training_effect", workout.training_effect.map(|v| json!(v))),
            (
                "anaerobic_training_effect",
                workout.anaerobic_training_effect.map(|v| json!(v)),
            ),
            ("rpe", workout.rpe.map(|v| json!(v))),
            ("avg_cadence_spm", workout.avg_cadence_spm.map(|v| json!(v))),
            ("max_cadence_spm", workout.max_cadence_spm.map(|v| json!(v))),
            ("avg_stride_cm", workout.avg_stride_cm.map(|v| json!(v))),
        ] {
            if let Some(value) = value {
                object.insert(key.into(), value);
            }
        }
        if !workout.hr_zones.is_empty() {
            object.insert("hr_zones".into(), serde_json::to_value(&workout.hr_zones)?);
        }
        // The user's own RPE / soreness, separate from the watch `rpe`.
        if let Some((rpe, soreness)) = self.workout_feel_map()?.remove(&workout.workout_id) {
            if let Some(rpe) = rpe {
                object.insert("user_rpe".into(), json!(rpe));
            }
            if !soreness.is_empty() {
                object.insert("soreness".into(), json!(soreness));
            }
        }
        object.insert("sample_count".into(), json!(workout.sample_count));
        object.insert("gps_available".into(), json!(workout.gps_available));
        object.insert("source_scope".into(), json!(workout.source_scope.as_str()));
        // The comparison items with recent runs of the same kind, the two halves (from the workout page insight card), and climb segments (from the climb card).
        if let Some(comparison) = comparison_json(&self.workout_insight(&workout.workout_id)?)? {
            object.insert("comparison".into(), comparison);
        }
        let series = self.get_workout_series(&workout.workout_id)?;
        if let Some(climbs) = &series.climbs {
            object.insert("climbs".into(), climbs_json(climbs)?);
        }
        if let Some(strength) = &series.strength {
            object.insert("strength".into(), serde_json::to_value(strength)?);
        }

        if task.detail_level == AiTaskDetailLevel::Detailed {
            let mut series_json = Map::new();
            series_json.insert("samples".into(), serde_json::to_value(&series.samples)?);
            series_json.insert("pauses".into(), serde_json::to_value(&series.pauses)?);
            series_json.insert("splits".into(), serde_json::to_value(&series.splits)?);
            series_json.insert("laps".into(), serde_json::to_value(&series.laps)?);
            series_json.insert("summary".into(), serde_json::to_value(&series.summary)?);
            if task.include_precise_gps {
                series_json.insert("route".into(), serde_json::to_value(&series.route)?);
            }
            object.insert("series".into(), Value::Object(series_json));
        }
        Ok(object)
    }
}

/// Fields the task excludes in the "Workouts" category are also left out of anchor workouts (export side of R01).
/// When heart rate fields are excluded, heart rate zones and heart rate in the per-point curve go too, because they give back
/// the average / max heart rate.
fn strip_excluded_workout_fields(object: &mut Map<String, Value>, excluded: &[String]) {
    for field in excluded {
        object.remove(field);
    }
    let calories_hidden = excluded.iter().any(|field| field == "calories");
    if calories_hidden {
        if let Some(sets) = object
            .get_mut("strength")
            .and_then(|s| s.get_mut("sets"))
            .and_then(Value::as_array_mut)
        {
            for set in sets {
                if let Some(set) = set.as_object_mut() {
                    set.remove("calories_kcal");
                }
            }
        }
    }
    let heart_rate_hidden = excluded
        .iter()
        .any(|field| matches!(field.as_str(), "avg_hr" | "max_hr" | "min_hr"));
    if !heart_rate_hidden {
        return;
    }
    object.remove("hr_zones");
    strip_heart_rate(object);
    if let Some(sets) = object
        .get_mut("strength")
        .and_then(|s| s.get_mut("sets"))
        .and_then(Value::as_array_mut)
    {
        for set in sets {
            if let Some(set) = set.as_object_mut() {
                set.remove("avg_hr");
            }
        }
    }
    if let Some(samples) = object
        .get_mut("series")
        .and_then(|series| series.get_mut("samples"))
        .and_then(Value::as_array_mut)
    {
        for sample in samples {
            if let Some(sample) = sample.as_object_mut() {
                sample.remove("heart_rate");
            }
        }
    }
    if let Some(summary) = object
        .get_mut("series")
        .and_then(|series| series.get_mut("summary"))
        .and_then(Value::as_object_mut)
    {
        summary.retain(|key, _| !key.contains("hr") && !key.contains("heart"));
    }
}

/// Coverage rows exported when the "Workouts" category is off: no anchor workout id. When a category has several anchor windows,
/// a merged view row with `workout_id: null` exists, so keep only it. With one window, clear that row's id.
fn coverage_without_anchors(coverage: &[AiTaskCoverage]) -> Vec<AiTaskCoverage> {
    let merged: BTreeSet<AiTaskCategory> = coverage
        .iter()
        .filter(|row| row.workout_id.is_none())
        .map(|row| row.category)
        .collect();
    coverage
        .iter()
        .filter(|row| row.workout_id.is_none() || !merged.contains(&row.category))
        .map(|row| AiTaskCoverage {
            workout_id: None,
            ..row.clone()
        })
        .collect()
}
