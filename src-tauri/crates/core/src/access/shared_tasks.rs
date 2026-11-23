//! Expand grant windows from shared tasks (split out of access.rs, same logic).

use super::*;

/// Read all tasks with `mcp_shared = 1` from the local database and expand them into grants.
///
/// No probe of `sqlite_master` / `table_info` first: SELECT directly. A missing table, missing columns
/// or any query error gives zero grants. This is the same fail-closed behavior as the old probe version, with two fewer
/// round trips. Every tool call runs this again, so the grant toggle on the task page takes effect on the next
/// MCP request at once, with no service restart.
pub fn shared_task_grants(db: &Database) -> Result<Vec<TaskGrant>> {
    let payloads: Vec<String> = (|| -> std::result::Result<Vec<String>, rusqlite::Error> {
        let mut stmt = db
            .conn
            .prepare("SELECT payload FROM ai_tasks WHERE mcp_shared = 1")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        rows.collect()
    })()
    .unwrap_or_default();

    let mut tasks = Vec::with_capacity(payloads.len());
    for payload in payloads {
        match serde_json::from_str::<SharedTaskPayload>(&payload) {
            Ok(task) => tasks.push(task),
            Err(error) => {
                eprintln!("zeppbridge-mcp: skipped a grant task that failed to parse: {error}");
            }
        }
    }

    // Fetch the local start day for the workout_ids of all tasks in one IN query. A primary-key query per id
    // is N+1 with many tasks and many workouts. Ids with no start day are still skipped on expansion.
    let workout_ids: Vec<String> = tasks
        .iter()
        .flat_map(|task| task.workout_ids.iter().cloned())
        .collect();
    let local_days = workout_local_days(db, &workout_ids)?;

    let mut grants = Vec::with_capacity(tasks.len());
    for task in tasks {
        grants.push(expand_task(task, &local_days, Local::now().date_naive()));
    }
    Ok(grants)
}

/// The minimal subset of `ai_tasks.payload` that the grant decision uses. Other fields (title, prompt,
/// attachments and so on) do not affect access and are not read.
#[derive(Debug, Deserialize)]
pub(super) struct SharedTaskPayload {
    #[serde(default)]
    pub(super) id: String,
    #[serde(default)]
    pub(super) workout_ids: Vec<String>,
    #[serde(default)]
    pub(super) categories: Vec<SharedCategoryRange>,
}

#[derive(Debug, Deserialize)]
pub(super) struct SharedCategoryRange {
    /// An `AiTaskCategory` string. An unknown category grants nothing, but does not break the whole task.
    pub(super) category: String,
    /// Missing means disabled: grants must fail closed.
    #[serde(default)]
    pub(super) enabled: bool,
    /// Same as the task model default: the daily window is 14 days back from the workout start day.
    #[serde(default = "default_days_before")]
    pub(super) days_before: i64,
    #[serde(default = "default_true")]
    pub(super) include_workout_day: bool,
    /// Metrics / fields the user dragged out in the relation graph. Export skips them, so MCP must skip them too (R01).
    #[serde(default)]
    pub(super) excluded_metrics: Vec<String>,
}

/// A task from before the category split (only one `heart_rate` row, covering all-day heart rate and resting heart rate): add a `resting_hr` row with the same settings.
/// The grant window is the same as before the split (same rule as `ai_tasks::upgrade_heart_rate_split`).
fn upgrade_heart_rate_split(categories: &mut Vec<SharedCategoryRange>) {
    if categories
        .iter()
        .any(|range| range.category == "resting_hr")
    {
        return;
    }
    let Some(at) = categories
        .iter()
        .position(|range| range.category == "heart_rate")
    else {
        return;
    };
    let legacy = &mut categories[at];
    let (resting, rest): (Vec<String>, Vec<String>) = legacy
        .excluded_metrics
        .drain(..)
        .partition(|metric| metric == "resting_hr");
    legacy.excluded_metrics = rest;
    let split = SharedCategoryRange {
        category: "resting_hr".into(),
        enabled: legacy.enabled,
        days_before: legacy.days_before,
        include_workout_day: legacy.include_workout_day,
        excluded_metrics: resting,
    };
    categories.insert(at, split);
}

pub(super) fn default_days_before() -> i64 {
    14
}

pub(super) fn default_true() -> bool {
    true
}

/// Expand one task's "workout_ids × enabled categories" into absolute date windows.
///
/// Each workout opens its own window: `[start day − days_before, start day]` (when `include_workout_day`
/// is false, the right end moves back one day). `local_days` is the start day table that `workout_local_days`
/// fetched in bulk. An id not in the table (record deleted / not synced) is skipped. A false category or an empty window
/// is skipped the same way. None of these is worth failing the whole grant.
///
/// A task with no linked workout ("last N days") has its window end at `today`, computed by the same function as export (R12).
/// A task with linked workouts where none can be found does not fall back to "today": export returns an error there, so this grants nothing.
///
/// A workout id has two roles (R01): it is always an **anchor** (it sets the date window).
/// It is also a **readable entity** only when the task enables the workout category. For a task with the workout
/// category off, MCP cannot read that workout itself.
pub(super) fn expand_task(
    mut task: SharedTaskPayload,
    local_days: &BTreeMap<String, NaiveDate>,
    today: NaiveDate,
) -> TaskGrant {
    upgrade_heart_rate_split(&mut task.categories);
    let mut windows = Vec::new();
    let anchor_days: Vec<Option<NaiveDate>> = if task.workout_ids.is_empty() {
        vec![None]
    } else {
        task.workout_ids
            .iter()
            .filter_map(|workout_id| local_days.get(workout_id).copied().map(Some))
            .collect()
    };
    for anchor_day in anchor_days {
        for range in &task.categories {
            if !range.enabled {
                continue;
            }
            let Some(category) = AccessCategory::parse(&range.category) else {
                continue;
            };
            if !category.is_windowed() {
                continue;
            }
            let Some((start, end)) = crate::ai_tasks::coverage::task_window(
                range.days_before,
                range.include_workout_day,
                anchor_day,
                today,
            ) else {
                continue;
            };
            if let Some(window) = GrantWindow::new(category, start, end) {
                windows.push(window.with_excluded(range.excluded_metrics.iter().cloned()));
            }
        }
    }
    let workout_range = task.categories.iter().find(|range| {
        range.enabled && AccessCategory::parse(&range.category) == Some(AccessCategory::Workout)
    });
    let (workout_ids, workout_excluded) = match workout_range {
        Some(range) => (
            task.workout_ids.clone(),
            range.excluded_metrics.iter().cloned().collect(),
        ),
        None => (Vec::new(), BTreeSet::new()),
    };
    TaskGrant {
        task_id: task.id,
        workout_ids,
        anchor_workout_ids: task.workout_ids,
        workout_excluded,
        windows,
    }
}

/// Bulk version of `workout_local_day`: fetch the local start day for the workout ids of all open tasks in one
/// `IN` query (P4: `date(start_time,'localtime')`), then match them to ids in memory.
/// Ids not in the table do not go into the map. The expansion side handles them as "not found, skip",
/// the same meaning as `None` from `.optional()` in the single version.
pub(super) fn workout_local_days(
    db: &Database,
    workout_ids: &[String],
) -> Result<BTreeMap<String, NaiveDate>> {
    let mut days = BTreeMap::new();
    if workout_ids.is_empty() {
        return Ok(days);
    }
    let placeholders = workout_ids
        .iter()
        .map(|_| "?")
        .collect::<Vec<_>>()
        .join(",");
    let mut stmt = db.conn.prepare(&format!(
        "SELECT workout_id, date(start_time, 'localtime') FROM workouts_visible
         WHERE workout_id IN ({placeholders})"
    ))?;
    let rows = stmt.query_map(params_from_iter(workout_ids.iter()), |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    for row in rows {
        let (workout_id, value) = row?;
        let day = NaiveDate::parse_from_str(&value, "%Y-%m-%d")
            .map_err(|error| ZeppBridgeError::ParseError(format!("Invalid workout start day: {error}")))?;
        days.insert(workout_id, day);
    }
    Ok(days)
}

/// The id of the latest night of sleep in the grant window (assigned by local wake-up day, same rule as the sleep list).
///
/// `get_sleep_detail` without an id in task scope is not "the latest night in the whole database", but
/// "the latest night in the grant window". A newer record outside the window must not be visible.
pub fn latest_sleep_in_windows(db: &Database, permit: &Permit) -> Result<Option<String>> {
    let mut stmt = db.conn.prepare(
        "SELECT sleep_id, date(end_time, 'localtime') FROM sleep_sessions_shown
         ORDER BY end_time DESC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    for row in rows {
        let (sleep_id, day) = row?;
        let Ok(day) = NaiveDate::parse_from_str(&day, "%Y-%m-%d") else {
            continue;
        };
        if permit.day_permitted(AccessCategory::Sleep, day) {
            return Ok(Some(sleep_id));
        }
    }
    Ok(None)
}

/* ------------------------------ Tests ------------------------------ */
