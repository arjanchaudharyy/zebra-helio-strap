use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Duration, Local, NaiveDate, Utc};

use rusqlite::params_from_iter;

use serde::{Deserialize, Serialize};

use crate::insight::{
    self, BaselineEntry, BaselineExclusion, Comparison, Confidence, InsightFact, WorkoutInsight,
};

use crate::models::error::{Result, ZeppBridgeError};

use crate::models::{MetricSeries, Workout};

use crate::storage::Database;

/* ------------------------------ Scope and categories ------------------------------ */

mod rescore;
mod shared_tasks;

pub use rescore::*;
pub use shared_tasks::*;

/// The read-only scope chosen when MCP starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessScope {
    /// Old behavior: the whole database read-only. Starting without `--scope` gives this. It is the only form compatible with old configs.
    FullReadOnly,
    /// Read only the data selected by tasks "shared with MCP". Grants are read again live on each request.
    TaskScoped,
}

impl AccessScope {
    /// `full-readonly` / `task`. Anything else is `None`, and the caller fails closed on it.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "full-readonly" => Some(Self::FullReadOnly),
            "task" => Some(Self::TaskScoped),
            _ => None,
        }
    }

    /// The string used for `scope.mode` in responses. It matches the argv value.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::FullReadOnly => "full-readonly",
            Self::TaskScoped => "task",
        }
    }

    pub fn is_task_scoped(&self) -> bool {
        matches!(self, Self::TaskScoped)
    }
}

/// The access-side mirror of task categories.
///
/// Fields map one to one to `AiTaskCategory` (the S1 task model). It is defined apart here because
/// the grant decision must not depend on the internal structure of the task store. However the task model changes, this sees only these eight values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AccessCategory {
    /// The workout record entity itself. Granted by workout_id, not by date window.
    Workout,
    Sleep,
    Recovery,
    HeartRate,
    Training,
    Body,
    /// Notes the user writes. No tool serves it yet, so the decision treats it as not satisfiable.
    PersonalNote,
    /// Attachment metadata. Same as above: the first batch only adds it to the enum, not to grant windows.
    Attachment,
    /// Resting heart rate (split out of `heart_rate` in 1A·A10. `heart_rate` now covers only all-day heart rate samples).
    RestingHr,
}

impl AccessCategory {
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "workout" => Self::Workout,
            "sleep" => Self::Sleep,
            "recovery" => Self::Recovery,
            "heart_rate" => Self::HeartRate,
            "training" => Self::Training,
            "body" => Self::Body,
            "personal_note" => Self::PersonalNote,
            "attachment" => Self::Attachment,
            "resting_hr" => Self::RestingHr,
            _ => return None,
        })
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Workout => "workout",
            Self::Sleep => "sleep",
            Self::Recovery => "recovery",
            Self::HeartRate => "heart_rate",
            Self::Training => "training",
            Self::Body => "body",
            Self::PersonalNote => "personal_note",
            Self::Attachment => "attachment",
            Self::RestingHr => "resting_hr",
        }
    }

    /// Health data categories windowed by local day. `Workout` is granted by id. `PersonalNote` /
    /// `Attachment` are task content, not time series. None of these produce a `GrantWindow`.
    pub fn is_windowed(&self) -> bool {
        matches!(
            self,
            Self::Sleep
                | Self::Recovery
                | Self::HeartRate
                | Self::RestingHr
                | Self::Training
                | Self::Body
        )
    }
}

/* ------------------------------ Grants and windows ------------------------------ */

/// A granted window of local days for one category (both ends included).
///
/// The serde shape `{category, start, end}` is the public contract for `permittedRanges`.
/// `NaiveDate` serializes as `YYYY-MM-DD`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GrantWindow {
    pub category: AccessCategory,
    #[serde(rename = "start")]
    pub start_date: NaiveDate,
    #[serde(rename = "end")]
    pub end_date: NaiveDate,
    /// Metric names / field names the task excludes in this category (`excluded_metrics`).
    /// The window allows "this category on these days, minus these names" (R01).
    /// Not part of the public `permittedRanges` shape.
    #[serde(skip)]
    pub excluded: BTreeSet<String>,
}

impl GrantWindow {
    /// An empty window (for example `include_workout_day=false` and `days_before=0`) is dropped,
    /// so a window that covers nothing does not stay in the grants.
    pub fn new(
        category: AccessCategory,
        start_date: NaiveDate,
        end_date: NaiveDate,
    ) -> Option<Self> {
        (start_date <= end_date).then_some(Self {
            category,
            start_date,
            end_date,
            excluded: BTreeSet::new(),
        })
    }

    /// Attach the names the task excludes in this window.
    pub fn with_excluded(mut self, excluded: impl IntoIterator<Item = String>) -> Self {
        self.excluded = excluded.into_iter().collect();
        self
    }

    pub fn contains(&self, day: NaiveDate) -> bool {
        self.start_date <= day && day <= self.end_date
    }

    /// Whether this window allows a given metric / field.
    pub fn allows(&self, name: &str) -> bool {
        !self.excluded.contains(name)
    }

    /// Intersect with `[start, end]`. No overlap gives empty.
    fn intersect(&self, start: NaiveDate, end: NaiveDate) -> Option<Self> {
        Self::new(
            self.category,
            self.start_date.max(start),
            self.end_date.min(end),
        )
        .map(|window| window.with_excluded(self.excluded.iter().cloned()))
    }
}

/// The grant one shared task expands into: workouts by id, health data by category window.
///
/// `windows` already holds absolute dates at load time. Rules such as `days_before` / `include_workout_day`
/// are expanded in `shared_task_grants()`, so the decision side never sees them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskGrant {
    pub task_id: String,
    /// Readable workout entities: the workouts the task chose, only when the task enables the workout category. Otherwise empty.
    pub workout_ids: Vec<String>,
    /// Anchor workouts used only to set date windows. An anchor is not a read grant (R01).
    pub anchor_workout_ids: Vec<String>,
    /// Field names excluded in the workout category (`distance_meters`, `avg_hr` and so on).
    pub workout_excluded: BTreeSet<String>,
    pub windows: Vec<GrantWindow>,
}

/// What one tool call wants. The MCP adapter builds it from the tool name and arguments.
#[derive(Debug, Clone, Default)]
pub struct DataRequest {
    /// Tool name, used only to name the call in denial reasons.
    pub tool: &'static str,
    /// The categories this request touches. Metrics map in through `metric_category()`.
    pub categories: Vec<AccessCategory>,
    /// Metrics the request names (`get_metric_series`). Each one must be not excluded in some grant window.
    pub metrics: Vec<String>,
    /// The local day range the request covers (the days of `get_metric_series` converted).
    pub date_range: Option<(NaiveDate, NaiveDate)>,
    /// Workout ids the request names. Empty = a listing request (`list_workouts`).
    pub workout_ids: Vec<String>,
    /// `get_sleep_detail` without an id: it wants "the latest night in the grant window", not the latest in the whole database.
    pub latest_sleep: bool,
    /// A request whose answer is a whole-database aggregate that cannot be cleanly trimmed to task scope (`get_data_health`).
    /// Task scope denies it fully. See R5.
    pub whole_library: bool,
}

/// Decision result: allowed.
///
/// The sets inside are the ceiling of what the executor may see: workouts outside `workout_ids` do not exist,
/// and dates outside `windows` do not exist.
#[derive(Debug, Clone, Default)]
pub struct Permit {
    /// The union of readable workout_ids of all shared tasks.
    pub workout_ids: BTreeSet<String>,
    /// Workout id -> fields still excluded. When several tasks allow the same workout, take the intersection:
    /// if any one task allows a field, it goes out (union grant). Empty sets are not stored.
    pub workout_excluded: BTreeMap<String, BTreeSet<String>>,
    /// Grant windows already trimmed to the request categories and date range.
    pub windows: Vec<GrantWindow>,
    /// The number of shared tasks this decision saw (source of `scope.grants`).
    pub grants: usize,
}

impl Permit {
    /// Placeholder for FullReadOnly. The executor branches on `scope` and does not read these empty sets.
    pub fn unrestricted() -> Self {
        Self::default()
    }

    /// Whether a local day of a category is granted.
    pub fn day_permitted(&self, category: AccessCategory, day: NaiveDate) -> bool {
        self.windows
            .iter()
            .any(|window| window.category == category && window.contains(day))
    }

    /// Whether a metric is granted on a local day: that day has a window of the same category and that window does not exclude it.
    pub fn metric_permitted(&self, category: AccessCategory, metric: &str, day: NaiveDate) -> bool {
        self.windows.iter().any(|window| {
            window.category == category && window.contains(day) && window.allows(metric)
        })
    }

    /// Whether the category has at least one grant window that does not exclude this metric.
    pub fn metric_has_window(&self, category: AccessCategory, metric: &str) -> bool {
        self.windows
            .iter()
            .any(|window| window.category == category && window.allows(metric))
    }

    /// The total number of days in the union of this category's grant windows (overlaps merged, no double count).
    pub fn permitted_day_count(&self, category: AccessCategory) -> i64 {
        self.day_count_where(|window| window.category == category)
    }

    /// The number of days a metric is actually allowed (windows that exclude it do not count).
    pub fn metric_day_count(&self, category: AccessCategory, metric: &str) -> i64 {
        self.day_count_where(|window| window.category == category && window.allows(metric))
    }

    fn day_count_where(&self, keep: impl Fn(&GrantWindow) -> bool) -> i64 {
        // Count days by date only: drop the exclusion sets, then merge. Overlapping days count once.
        let spans = merge_windows(
            self.windows
                .iter()
                .filter(|window| keep(window))
                .filter_map(|window| {
                    GrantWindow::new(window.category, window.start_date, window.end_date)
                })
                .collect(),
        );
        spans
            .iter()
            .map(|window| (window.end_date - window.start_date).num_days() + 1)
            .sum()
    }

    /// Sleep fields still excluded for one night (by local wake-up day): the intersection of the exclusion sets
    /// of the sleep windows that cover this day. A field any window allows goes out.
    pub fn sleep_excluded_fields(&self, day: NaiveDate) -> BTreeSet<String> {
        let mut covering = self
            .windows
            .iter()
            .filter(|window| window.category == AccessCategory::Sleep && window.contains(day));
        let Some(first) = covering.next() else {
            return BTreeSet::new();
        };
        covering.fold(first.excluded.clone(), |acc, window| {
            acc.intersection(&window.excluded).cloned().collect()
        })
    }

    /// Fields still excluded for one workout.
    pub fn workout_excluded_fields(&self, workout_id: &str) -> BTreeSet<String> {
        self.workout_excluded
            .get(workout_id)
            .cloned()
            .unwrap_or_default()
    }

    /// Whether the category has at least one grant window (whether or not it overlaps the request range).
    fn has_window(&self, category: AccessCategory) -> bool {
        self.windows
            .iter()
            .any(|window| window.category == category)
    }
}

/// Decision result: denied. `code` is a stable two-part error code, placed in the structured error data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Denied {
    pub code: &'static str,
    pub reason: String,
}

pub const SCOPE_DENIED: &str = "err.mcp.scope_denied";

pub const SCOPE_NO_GRANTS: &str = "err.mcp.scope_no_grants";

fn denied(reason: impl Into<String>) -> Denied {
    Denied {
        code: SCOPE_DENIED,
        reason: reason.into(),
    }
}

/* ------------------------------ Decision ------------------------------ */

/// The single grant entry point. FullReadOnly allows directly. TaskScoped trims item by item by grant.
pub fn authorize(
    scope: &AccessScope,
    grants: &[TaskGrant],
    request: &DataRequest,
) -> std::result::Result<Permit, Denied> {
    match scope {
        AccessScope::FullReadOnly => Ok(Permit::unrestricted()),
        AccessScope::TaskScoped => authorize_task(grants, request),
    }
}

fn authorize_task(
    grants: &[TaskGrant],
    request: &DataRequest,
) -> std::result::Result<Permit, Denied> {
    if grants.is_empty() {
        return Err(Denied {
            code: SCOPE_NO_GRANTS,
            reason:
                "No task is shared with MCP yet. On the Tasks page of the desktop app, mark a task as \"Shared with MCP\" and try again."
                    .into(),
        });
    }
    // A whole-database aggregate (data health) cannot be cleanly trimmed to task windows: coverage gaps, last sync time and
    // newest_sample_at all use whole-database scope, and no honest subset can be built. Deny it fully,
    // so the caller knows "this tool has no meaning in task scope", instead of giving a report that looks
    // global but was trimmed.
    if request.whole_library {
        return Err(denied(format!(
            "{} uses a whole-database view (data health, metric list, diet details, life events) and cannot be honestly trimmed to the task grant scope. It is not available in task scope.",
            request.tool
        )));
    }

    let workout_ids: BTreeSet<String> = grants
        .iter()
        .flat_map(|grant| grant.workout_ids.iter().cloned())
        .collect();
    let mut workout_excluded = BTreeMap::new();
    for workout_id in &workout_ids {
        let mut excluded: Option<BTreeSet<String>> = None;
        for grant in grants
            .iter()
            .filter(|grant| grant.workout_ids.contains(workout_id))
        {
            excluded = Some(match excluded {
                None => grant.workout_excluded.clone(),
                Some(acc) => acc.intersection(&grant.workout_excluded).cloned().collect(),
            });
        }
        if let Some(excluded) = excluded.filter(|set| !set.is_empty()) {
            workout_excluded.insert(workout_id.clone(), excluded);
        }
    }

    // Requested workouts must all be in the grant union. If one is not, deny fully instead of
    // silently skipping it (silent skips make people think the record does not exist).
    if let Some(foreign) = request
        .workout_ids
        .iter()
        .find(|id| !workout_ids.contains(*id))
    {
        return Err(denied(format!(
            "Workout {foreign} is not in the grant list of any shared task. Use list_workouts first to see the shared records."
        )));
    }

    let windows = granted_windows(grants, &request.categories, request.date_range);
    let permit = Permit {
        workout_ids,
        workout_excluded,
        windows,
        grants: grants.len(),
    };

    // Every requested category must be servable. If one is not, deny fully (R3):
    // - Workout: granted by id. A listing (no id) is always satisfiable. With an empty grant set it
    //   honestly returns an empty list. A named request was already checked against the union above.
    // - Windowed categories: the request range must overlap the category's grant windows. If metrics with no grant were silently dropped,
    //   the model would read that as "no data", not "no grant". Those are two different truths.
    // - PersonalNote / Attachment: no tool serves them in the first batch, so they count as not satisfiable.
    for category in &request.categories {
        let covered = match category {
            AccessCategory::Workout => true,
            AccessCategory::PersonalNote | AccessCategory::Attachment => false,
            windowed => permit.has_window(*windowed),
        };
        if !covered {
            return Err(denied(format!(
                "The requested {} data is not in the grant scope of any shared task, or has no date overlap with the grant windows. permittedRanges in the error data lists the windows actually granted.",
                category.as_str()
            )));
        }
    }
    // Check each named metric: a window for the category is not enough, the window must not exclude it (R01). An excluded
    // metric is denied honestly, instead of returning an empty series that makes the model think "no data".
    for metric in &request.metrics {
        let Some(category) = metric_category(metric) else {
            continue;
        };
        if !permit.metric_has_window(category, metric) {
            return Err(denied(format!(
                "Metric {metric} is excluded by the shared tasks, or has no date overlap with the grant windows."
            )));
        }
    }
    Ok(permit)
}

/// Grant window query: the union of valid windows of some tasks, for some categories, in a date range.
///
/// `authorize` uses it to decide, and the denial response uses it to tell the caller "what is actually allowed".
/// Windows of the same category merge when they overlap or touch, so N workouts of one task do not make N almost identical windows.
pub fn granted_windows(
    grants: &[TaskGrant],
    categories: &[AccessCategory],
    date_range: Option<(NaiveDate, NaiveDate)>,
) -> Vec<GrantWindow> {
    let windows = grants
        .iter()
        .flat_map(|grant| grant.windows.iter())
        .filter(|window| categories.contains(&window.category))
        .filter_map(|window| match date_range {
            Some((start, end)) => window.intersect(start, end),
            None => Some(window.clone()),
        })
        .collect();
    merge_windows(windows)
}

/// Windows with the same category and the same exclusion set are sorted by start, then merged when they overlap or touch (end to start also
/// merges into one). Windows with different exclusion sets do not merge: merging would let one side's exclusion remove the other side's grant.
fn merge_windows(mut windows: Vec<GrantWindow>) -> Vec<GrantWindow> {
    windows.sort_by(|a, b| {
        (a.category, &a.excluded, a.start_date, a.end_date).cmp(&(
            b.category,
            &b.excluded,
            b.start_date,
            b.end_date,
        ))
    });
    let mut merged: Vec<GrantWindow> = Vec::with_capacity(windows.len());
    for window in windows {
        if let Some(last) = merged.last_mut() {
            let adjacent = Duration::try_days(1)
                .and_then(|one| last.end_date.checked_add_signed(one))
                .is_some_and(|next| window.start_date <= next);
            if last.category == window.category && last.excluded == window.excluded && adjacent {
                last.end_date = last.end_date.max(window.end_date);
                continue;
            }
        }
        merged.push(window);
    }
    merged
}

/* ------------------------------ Category -> metric map ------------------------------

* The task category of every metric in the contract that has a daily series. This table is the basis of the access-side
* decision: the category sets the window, and the window sets which days can go out.
*
* It is kept apart from the metric contract in contract.rs on purpose: contract.rs is the versioned public
* contract (S1), and category ownership is grant policy (this file). `AiTaskCategory` has no "diet"
* category, so intake metrics go under body for now: they are inputs to the body ledger. If the task model later
* adds a nutrition category, change only this table. */

/// Metric name -> task category. Returns `None` when not found, so task scope fails closed on unregistered metrics.
pub fn metric_category(metric: &str) -> Option<AccessCategory> {
    Some(match metric {
        // Night metrics belong to sleep by day: the blood oxygen series is measured during sleep on the Zepp side.
        "sleep_score" | "spo2" | "spo2_odi" | "spo2_night_score" | "spo2_measured_minutes" => {
            AccessCategory::Sleep
        }
        "readiness" | "physical_readiness" | "mental_readiness" | "hybrid_charge"
        | "physical_charge" | "mental_charge" | "stress" | "respiratory_rate" | "sleep_hrv"
        | "sleep_rhr" | "hrv" | "hrv_rmssd" | "hrv_baseline" | "rhr_baseline" | "ahi_baseline" => {
            AccessCategory::Recovery
        }
        "resting_hr" => AccessCategory::RestingHr,
        // Training load and daily activity are in the same category: they are the same ledger of "how much training".
        "training_load"
        | "vo2max"
        | "lactate_threshold_hr"
        | "lactate_threshold_pace"
        | "pai_daily"
        | "pai_low_zone"
        | "pai_medium_zone"
        | "pai_high_zone"
        | "pai_total"
        | "pai_low_zone_minutes"
        | "pai_medium_zone_minutes"
        | "pai_high_zone_minutes"
        | "pai_low_zone_lower_hr"
        | "pai_medium_zone_lower_hr"
        | "pai_high_zone_lower_hr"
        | "steps"
        | "distance"
        | "active_calories"
        | "active_minutes"
        | "step_goal"
        | "calorie_goal"
        | "active_minutes_goal" => AccessCategory::Training,
        "weight" | "bmi" | "height" | "body_fat_rate" | "body_water_rate" | "muscle_mass"
        | "bone_mass" | "protein_rate" | "visceral_fat" | "bmr" | "body_balance_score"
        | "intake_calories" | "intake_protein_g" | "intake_fat_g" | "intake_carbs_g" => {
            AccessCategory::Body
        }
        "blood_pressure_systolic" | "blood_pressure_diastolic" | "blood_pressure_pulse" => {
            AccessCategory::Body
        }
        _ => return None,
    })
}

/* ------------------------------ Series and structure trimming ------------------------------

* All filtering after the grant happens in memory: SQL already fetched the rows in the request range. This
* only removes, never adds. Trimmed parts never appear in the response in any form. */

/// Trim each series of `get_metric_series` to the grant windows of its category.
///
/// A series with zero points after trimming is dropped fully (the same as "this category has no grant"). The series that remain
/// have points only on granted days. `latest`/`average`/`days_with_data`/`window_days`
/// are recomputed from the trimmed result, so each series describes the range it actually covers.
pub fn clip_metric_series(series: Vec<MetricSeries>, permit: &Permit) -> Vec<MetricSeries> {
    series
        .into_iter()
        .filter_map(|mut series| {
            let category = metric_category(&series.metric)?;
            let metric = series.metric.clone();
            if !permit.metric_has_window(category, &metric) {
                return None;
            }
            series.points.retain(|point| {
                NaiveDate::parse_from_str(&point.date, "%Y-%m-%d")
                    .map(|day| permit.metric_permitted(category, &metric, day))
                    .unwrap_or(false)
            });
            let values: Vec<f64> = series.points.iter().map(|point| point.value).collect();
            series.latest = series.points.last().cloned();
            series.average = mean(&values).map(round1);
            (series.minimum, series.maximum) =
                crate::models::metric_series_extrema(&series.metric, &series.points);
            series.days_with_data = series.points.len() as i64;
            series.window_days = permit.metric_day_count(category, &metric);
            Some(series)
        })
        .collect()
}

/* ------------------------------ Field-level projection (R01) ------------------------------

* Category windows decide "which days", and exclusion sets decide "which fields". Field names match the task model
* `excluded_metrics` (the keys of `ai_tasks::coverage::category_units`). */

/// Sleep stage minute fields: when any one is excluded, the stage timeline does not go out either.
/// Otherwise the excluded minutes could be computed back from the stage segments. Export uses the same table.
pub const SLEEP_STAGE_FIELDS: [&str; 4] = [
    "deep_minutes",
    "light_minutes",
    "rem_minutes",
    "awake_minutes",
];

/// Delete the excluded fields from one sleep record's JSON (and `stages` too when stage minutes are excluded).
pub fn project_sleep_fields(value: &mut serde_json::Value, excluded: &BTreeSet<String>) {
    let Some(object) = value.as_object_mut() else {
        return;
    };
    for field in excluded {
        object.remove(field);
    }
    if SLEEP_STAGE_FIELDS
        .iter()
        .any(|field| excluded.contains(*field))
    {
        object.remove("stages");
    }
}

/// camelCase fields of the `list_workouts` output <-> task model field names.
const WORKOUT_LIST_FIELDS: [(&str, &str); 4] = [
    ("distance_meters", "distanceMeters"),
    ("calories", "calories"),
    ("avg_hr", "avgHr"),
    ("max_hr", "maxHr"),
];

/// Delete the excluded fields from one `list_workouts` record.
pub fn project_workout_list_entry(value: &mut serde_json::Value, excluded: &BTreeSet<String>) {
    let Some(object) = value.as_object_mut() else {
        return;
    };
    for (field, key) in WORKOUT_LIST_FIELDS {
        if excluded.contains(field) {
            object.remove(key);
        }
    }
}

/// Delete the excluded fields from a full `get_workout_detail` workout (snake_case, field names match the task model),
/// plus the fields that can compute them back: heart rate zones do not go out when heart rate is excluded, stride does not go out when distance is excluded
/// (steps × stride is distance), and max / min altitude do not go out when ascent / descent is excluded.
pub fn project_workout_detail(value: &mut serde_json::Value, excluded: &BTreeSet<String>) {
    let Some(object) = value.as_object_mut() else {
        return;
    };
    for field in excluded {
        object.remove(field);
    }
    if ["avg_hr", "max_hr", "min_hr"]
        .iter()
        .any(|field| excluded.contains(*field))
    {
        object.remove("hr_zones");
        if let Some(sets) = object
            .get_mut("strength")
            .and_then(|s| s.get_mut("sets"))
            .and_then(serde_json::Value::as_array_mut)
        {
            for set in sets {
                if let Some(set) = set.as_object_mut() {
                    set.remove("avg_hr");
                }
            }
        }
    }
    if excluded.contains("calories") {
        if let Some(sets) = object
            .get_mut("strength")
            .and_then(|s| s.get_mut("sets"))
            .and_then(serde_json::Value::as_array_mut)
        {
            for set in sets {
                if let Some(set) = set.as_object_mut() {
                    set.remove("calories_kcal");
                }
            }
        }
    }
    if excluded.contains("distance_meters") {
        object.remove("avg_stride_cm");
    }
    if excluded.contains("elevation_gain_m") || excluded.contains("elevation_loss_m") {
        object.remove("max_altitude_m");
        object.remove("min_altitude_m");
    }
}

/// camelCase fields of the `list_sleep_sessions` output <-> task model field names.
const SLEEP_LIST_FIELDS: [(&str, &str); 2] =
    [("score", "score"), ("duration_minutes", "durationMinutes")];

/// Delete the excluded fields from one `list_sleep_sessions` record.
pub fn project_sleep_list_entry(value: &mut serde_json::Value, excluded: &BTreeSet<String>) {
    let Some(object) = value.as_object_mut() else {
        return;
    };
    for (field, key) in SLEEP_LIST_FIELDS {
        if excluded.contains(field) {
            object.remove(key);
        }
    }
}

/// Insight fact metric -> the workout fields it depends on. Pace depends on both distance and duration.
fn insight_fact_fields(metric: &str) -> &'static [&'static str] {
    match metric {
        "distance" => &["distance_meters"],
        "duration" => &["moving_seconds"],
        "pace" => &["distance_meters", "moving_seconds"],
        "avg_hr" => &["avg_hr"],
        "training_load" => &["training_load"],
        _ => &[],
    }
}

/// Delete the insight facts that depend on fields the target workout excludes. When heart rate is excluded,
/// heart rate drift does not go out either.
pub fn project_workout_insight(insight: &mut WorkoutInsight, excluded: &BTreeSet<String>) {
    if excluded.is_empty() {
        return;
    }
    insight.facts.retain(|fact| {
        !insight_fact_fields(&fact.metric)
            .iter()
            .any(|field| excluded.contains(*field))
    });
    if excluded.contains("avg_hr") || excluded.contains("max_hr") {
        insight.heart_rate_drift = None;
        insight.heart_rate_drift_unavailable = None;
    }
}

/// Strip identity fields recursively.
///
/// Task scope does not expose `device_id`: it is an identifier shaped like a device serial number. It has no use
/// for health questions, but giving it out is a real fingerprint. At the output that picks fields by allowlist (list_workouts) this
/// is extra safety. At the output that serializes the full struct (sleep detail) this is the redaction that takes effect.
pub fn strip_identity_fields(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            map.remove("device_id");
            for child in map.values_mut() {
                strip_identity_fields(child);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                strip_identity_fields(item);
            }
        }
        _ => {}
    }
}

/* ------------------------------ Insight baseline rescore ------------------------------

* The `workout_insight` baseline is computed on the whole database: baseline_included/excluded carry the
* ids, dates and distances of workouts with no grant, and baseline_value / confidence in facts aggregate samples with no grant
* (R1). A post-filter that only deletes arrays is not enough: the mean itself is a leak.
*
* So this **recomputes** the baseline on the granted set: the candidate set is still what `workout_insight` gave,
* included ∪ excluded (they already passed the comparability filter). Trim to granted ids first, then fill the
* freed slots with granted rows from `beyond_max_samples` in the original order. Last, recompute each fact's comparison value, sample count,
* evidence references and confidence against the granted baseline.
*
* The decision table here mirrors the same rule as `insight::run_fact`: sample thresholds come from
* public constants in `insight::baseline`, and reason codes keep their original values. If the table there changes,
* this one must change too (a change request went to S1: core will provide a `workout_insight` variant with an allowlist,
* and then this function retires). */

#[cfg(test)]
mod tests;
