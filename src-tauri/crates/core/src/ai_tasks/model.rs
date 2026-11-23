//! Data structures of BETA1 A7-P1/P2. Field names are snake_case on both sides and are the public contract.
//!
//! Key boundaries:
//! - [`AiTaskAttachmentRef`] is the **stored form**, with a local `path`. It must stay on this machine.
//! - [`AiTaskAttachmentPublicRef`] is the **export form**. It is the only attachment reference allowed in export JSON
//!   and MCP output, and its type has no `path` field.
//! - `AiTaskIssue` in preview/prepare results is "a state, not an error": it uses `ui.ai_task.*`
//!   codes. Only a real `Err` uses `err.ai_task.*` (P3 extra rule).

use serde::{Deserialize, Serialize};

/// Schema version of the task/template payload. The payload JSON matches the `schema_version` field.
pub const AI_TASK_SCHEMA_VERSION: i64 = 1;

fn default_schema_version() -> i64 {
    AI_TASK_SCHEMA_VERSION
}

fn default_days_before() -> i64 {
    14
}

fn default_true() -> bool {
    true
}

/// Data categories a task covers. `personal_note` / `attachment` are task-level content
/// and do not take part in window data fetches that look back from the workout start day.
///
/// Since 3.0 beta.48, `heart_rate` means only **all-day HR** (point samples). Resting HR is its own
/// `resting_hr` category (the AI page splits "Heart rate" into "Resting HR" and "All-day HR").
/// The enum only grows and never renames. Old task documents with only `heart_rate` get a `resting_hr` row
/// with the same settings from [`upgrade_heart_rate_split`] on read, so the exported data is the same as before.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiTaskCategory {
    Workout,
    Sleep,
    Recovery,
    HeartRate,
    Training,
    Body,
    PersonalNote,
    Attachment,
    RestingHr,
}

/// Old documents (before the split) have one `heart_rate` row that covers all-day HR and resting HR. Add a `resting_hr` row
/// that copies the switch, window and picked days. Split excluded metrics by owner (`resting_hr` goes to the new row, the rest stay).
/// If a `resting_hr` row exists (new document), do nothing. Idempotent.
pub fn upgrade_heart_rate_split(categories: &mut Vec<AiTaskCategoryRange>) {
    if categories
        .iter()
        .any(|range| range.category == AiTaskCategory::RestingHr)
    {
        return;
    }
    let Some(at) = categories
        .iter()
        .position(|range| range.category == AiTaskCategory::HeartRate)
    else {
        return;
    };
    let legacy = &mut categories[at];
    let (resting, rest): (Vec<String>, Vec<String>) = legacy
        .excluded_metrics
        .drain(..)
        .partition(|metric| metric == "resting_hr");
    legacy.excluded_metrics = rest;
    let split = AiTaskCategoryRange {
        category: AiTaskCategory::RestingHr,
        excluded_metrics: resting,
        ..legacy.clone()
    };
    categories.insert(at, split);
}

impl AiTaskCategory {
    /// Whether this category fetches data by workout window.
    ///
    /// `personal_note` / `attachment` produce no window: the first is text the user wrote,
    /// the second is files the user picked. Neither has "data in a date range".
    pub fn is_windowed(self) -> bool {
        !matches!(self, Self::PersonalNote | Self::Attachment)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiTaskDetailLevel {
    Summary,
    #[default]
    Standard,
    /// Only detailed adds point series (workout_samples / route).
    Detailed,
}

/// Window settings of one category: `[workout local start day - days_before, workout start day]`
/// (with `include_workout_day=false` the right end is the day before).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AiTaskCategoryRange {
    pub category: AiTaskCategory,
    #[serde(default)]
    pub enabled: bool,
    /// Each workout looks back from its own start day. Default 14.
    #[serde(default = "default_days_before")]
    pub days_before: i64,
    /// Default true.
    #[serde(default = "default_true")]
    pub include_workout_day: bool,
    /// Metric names (metric categories) or field names (workout/sleep categories, keys of `units`)
    /// the user dragged out in the relation graph. Export skips them.
    /// Coverage `metric_days` still gives their day counts, so the UI can show "how much you get if you drag it back".
    #[serde(default)]
    pub excluded_metrics: Vec<String>,
    /// Specific days picked in the inbox (local day `YYYY-MM-DD`, polish batch 7.4). When not empty, this category sends only these days:
    /// the window shrinks to `[earliest, latest]`, and unpicked days in between are not exported or counted as covered. `days_before`
    /// is then used only by MCP task grants (grants use a continuous window, wider than the picked days; see `access::shared_tasks`).
    /// Empty = fetch a continuous range by `days_before` as before.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub picked_days: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiTaskAttachmentKind {
    Pdf,
    Image,
}

/// Stored form of an attachment reference. `path` is a local absolute path. It **moves only between sqlite
/// and stat**. No export JSON / MCP output may contain it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AiTaskAttachmentRef {
    pub id: String,
    /// Local path. Before export it must change form through [`AiTaskAttachmentPublicRef::from`].
    pub path: String,
    pub display_name: String,
    pub kind: AiTaskAttachmentKind,
    /// Size recorded when added. preview/prepare use it as the baseline to decide `changed`.
    /// `None` means no size was recorded (old data or manual entry), so change cannot be detected.
    #[serde(default)]
    pub byte_len: Option<i64>,
    pub added_at: String,
}

/// Export form of an attachment reference: display name, type, size. The type has no `path`,
/// so the old "build clean data, then redact" path cannot even go wrong here.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AiTaskAttachmentPublicRef {
    pub display_name: String,
    pub kind: AiTaskAttachmentKind,
    #[serde(default)]
    pub byte_len: Option<i64>,
}

impl From<&AiTaskAttachmentRef> for AiTaskAttachmentPublicRef {
    fn from(reference: &AiTaskAttachmentRef) -> Self {
        Self {
            display_name: reference.display_name.clone(),
            kind: reference.kind,
            byte_len: reference.byte_len,
        }
    }
}

/// P1: analysis task. The backend writes `created_at`/`updated_at` and overwrites whatever the frontend sends.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AiTask {
    #[serde(default = "default_schema_version")]
    pub schema_version: i64,
    /// An empty id means an unsaved draft. `ai_task_save` assigns a new `task-*` id.
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub template_id: Option<String>,
    /// 1..N, may be empty (not picked yet).
    #[serde(default)]
    pub workout_ids: Vec<String>,
    #[serde(default)]
    pub categories: Vec<AiTaskCategoryRange>,
    #[serde(default)]
    pub detail_level: AiTaskDetailLevel,
    /// User-editable prompt. The template gives only the first draft.
    #[serde(default)]
    pub prompt: String,
    #[serde(default)]
    pub personal_note: String,
    #[serde(default)]
    pub attachments: Vec<AiTaskAttachmentRef>,
    /// Default false. When `false`, the export has no point route.
    #[serde(default)]
    pub include_precise_gps: bool,
    /// Default false. true = open to task-scoped MCP (used by S5).
    #[serde(default)]
    pub mcp_shared: bool,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
}

/// P1: task template. Built-in templates are read-only and can only be saved as user templates.
/// A template does **not** store workout_ids / attachments / data snapshots.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AiTaskTemplate {
    #[serde(default = "default_schema_version")]
    pub schema_version: i64,
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub builtin: bool,
    #[serde(default)]
    pub name: String,
    /// Built-in templates get copy from `ui.ai_template.<id>.name`. User templates have null.
    #[serde(default)]
    pub name_code: Option<String>,
    #[serde(default)]
    pub categories: Vec<AiTaskCategoryRange>,
    #[serde(default)]
    pub detail_level: AiTaskDetailLevel,
    /// For built-in templates this is the fallback text. The UI gets localized text by `prompt_code`.
    #[serde(default)]
    pub prompt_template: String,
    #[serde(default)]
    pub prompt_code: Option<String>,
    /// Declarative hints: which summary fields to want by default and which series must have data.
    /// Beta1 has no consumer yet. It is stored for W3+ checks/hints.
    #[serde(default)]
    pub default_summaries: Vec<String>,
    #[serde(default)]
    pub required_series: Vec<String>,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
}

/// Row returned by P3 `ai_task_list`, read straight from the index columns without parsing payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AiTaskSummary {
    pub id: String,
    pub title: String,
    pub template_id: Option<String>,
    pub workout_count: i64,
    pub updated_at: String,
    pub mcp_shared: bool,
    /// Tasks the user pinned come first and never fold into "Earlier" for being idle.
    #[serde(default)]
    pub pinned: bool,
    /// Enabled data categories (`heart_rate`, `sleep`, ...). The task page filters by them.
    #[serde(default)]
    pub categories: Vec<String>,
}

/// P2: coverage facts of one category in one window.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AiTaskCoverage {
    pub category: AiTaskCategory,
    /// One row per workout. When a category has several workout windows, add one merged `null` row
    /// (window union, totals after date dedup). When a task has no linked workouts, the window anchors on today,
    /// and the only row is also `null`.
    pub workout_id: Option<String>,
    /// Local day `YYYY-MM-DD`, inclusive.
    pub start_date: String,
    pub end_date: String,
    pub days_in_range: i64,
    pub days_with_data: i64,
    /// Local days `YYYY-MM-DD` with data in the window, ascending. The UI uses it to draw "which days have data, which lack it".
    /// It appears only in preview, not in the export file. The merged row of several windows is the union.
    #[serde(default)]
    pub covered_dates: Vec<String>,
    /// Matched `source_scope` values (`user_fused` / `device` / `unknown`).
    pub sources: Vec<String>,
    /// Metric -> unit map the category queries (fixed field units for workout/sleep).
    pub units: std::collections::BTreeMap<String, String>,
    /// Days with data per metric (or per field), including excluded metrics.
    #[serde(default)]
    pub metric_days: std::collections::BTreeMap<String, i64>,
    /// No data at all on any day in the window.
    pub missing: bool,
    /// This category sends picked days (the inbox): the picked days, ascending. Empty for a continuous window and absent from JSON.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub picked_dates: Vec<String>,
}

/// Workout summary listed to the frontend in preview. Thinner than [`crate::models::Workout`]:
/// no identity fields such as `device_id`, and no internal columns unrelated to the handoff.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AiTaskWorkoutBrief {
    pub workout_id: String,
    /// `effective_type`: a name the user changed wins over the normalized type.
    pub workout_type: String,
    /// RFC3339 UTC.
    pub start_time: String,
    pub end_time: String,
    /// Local start day, the window anchor (P4).
    pub start_date: String,
    pub distance_meters: Option<f64>,
    pub calories: Option<i32>,
    pub avg_hr: Option<i32>,
    pub max_hr: Option<i32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiTaskAttachmentState {
    Ok,
    Missing,
    Changed,
}

/// P2: whether an attachment is usable now.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AiTaskAttachmentStatus {
    pub id: String,
    pub status: AiTaskAttachmentState,
    /// Current measured size. `null` when the file does not exist.
    pub byte_len: Option<i64>,
}

/// Item of `warnings` / `blocked`: a state, not an error, so it uses `ui.ai_task.*` codes.
/// `params` holds numbers/names for UI layout, with no paths or identity keys.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AiTaskIssue {
    pub code: String,
    /// Fallback message.
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

impl AiTaskIssue {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_string(),
            message: message.into(),
            params: None,
        }
    }

    pub fn with_params(mut self, params: serde_json::Value) -> Self {
        self.params = Some(params);
        self
    }
}

/// P2: preview result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AiTaskPreview {
    /// `"draft"` for a draft (not saved).
    pub task_id: String,
    pub workouts: Vec<AiTaskWorkoutBrief>,
    pub coverage: Vec<AiTaskCoverage>,
    pub attachments: Vec<AiTaskAttachmentStatus>,
    /// Estimated UTF-8 byte count of the `health-context.json` that would be written (measured on the same built document).
    pub estimated_bytes: i64,
    pub warnings: Vec<AiTaskIssue>,
    /// Estimate when rendered as one `.md` within the given budget (batch 7). `null` when no budget is given.
    #[serde(default)]
    pub markdown: Option<AiTaskMarkdownEstimate>,
}

/// Size and downgrades of one `.md`. The UI uses it to write "about N tokens, the free plan can read it all / needs a paid plan"
/// and "workout curves averaged over N seconds".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AiTaskMarkdownEstimate {
    pub approx_tokens: i64,
    pub byte_len: i64,
    /// How many seconds the workout curves were averaged over. `null` when there are no curves.
    pub curve_average_seconds: Option<u32>,
    /// Workouts kept as summary lines only to fit the budget (oldest go first).
    pub summarized_workouts: Vec<String>,
    /// Still over budget at the last level: the UI asks to shorten the range.
    pub over_budget: bool,
    pub token_budget: i64,
}

/// P2: prepare result. With `status=blocked` no files are written, and `json_path`/`prompt_path`
/// are `null`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AiTaskPrepareResult {
    pub status: AiTaskPrepareStatus,
    pub task_id: String,
    pub output_dir: String,
    pub json_path: Option<String>,
    pub prompt_path: Option<String>,
    /// Built by the backend: user/template prompt + the coverage note localized by the frontend (verbatim).
    /// Also present when blocked, so the frontend can show it directly.
    pub prompt_text: String,
    /// Byte count of `health-context.json`. 0 when blocked.
    pub byte_len: i64,
    /// Number of attachment originals copied into `<output_dir>/attachments/`.
    #[serde(default)]
    pub copied_attachments: i64,
    /// Its path when delivered as one `.md` (then `json_path` / `prompt_path` are `null`).
    #[serde(default)]
    pub md_path: Option<String>,
    /// Size and downgrades of the `.md`. `null` when the delivery is not `.md`.
    #[serde(default)]
    pub markdown: Option<AiTaskMarkdownEstimate>,
    pub attachments: Vec<AiTaskAttachmentStatus>,
    pub blocked: Vec<AiTaskIssue>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiTaskPrepareStatus {
    Ready,
    Blocked,
}

/// Row of P3 `ai_task_attachment_stat`. `path` is valid here: this is the local UI
/// checking the file the user gave, not exported data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AiTaskAttachmentStat {
    pub path: String,
    pub exists: bool,
    pub byte_len: Option<i64>,
    /// RFC3339. `null` when the file does not exist.
    pub mtime: Option<String>,
}

/// One task removed by batch delete (undo = store it back as it was, then pin by `pinned`).
#[derive(Debug, Clone, Serialize)]
pub struct DeletedAiTask {
    pub task: AiTask,
    pub pinned: bool,
}

/// Built-in templates seeded by migration v32.
pub(crate) struct BuiltinTemplateSeed {
    pub id: &'static str,
    /// Fallback name in the column. The payload also has `name_code` for the UI to get localized copy.
    pub name: &'static str,
    /// Full AiTaskTemplate JSON (schema_version=1).
    pub payload: String,
}

/// Fixed timestamp of built-in rows: they are not user data. A fixed value keeps all databases byte-identical,
/// and an `INSERT OR IGNORE` rerun does not drift.
pub(crate) const BUILTIN_SEED_TIMESTAMP: &str = "2026-09-23T00:00:00Z";

/// Fill the list to all eight categories from (category, enabled, days_before) triples.
/// Unlisted categories are stored as `enabled=false, days_before=14`, so the frontend always gets
/// all eight `categories` and never guesses defaults.
fn builtin_categories(entries: &[(AiTaskCategory, bool, i64)]) -> Vec<AiTaskCategoryRange> {
    [
        AiTaskCategory::Workout,
        AiTaskCategory::Sleep,
        AiTaskCategory::Recovery,
        AiTaskCategory::RestingHr,
        AiTaskCategory::HeartRate,
        AiTaskCategory::Training,
        AiTaskCategory::Body,
        AiTaskCategory::PersonalNote,
        AiTaskCategory::Attachment,
    ]
    .into_iter()
    .map(|category| {
        // Built-in templates use the triples written before the split: resting HR follows the "Heart rate" item.
        let lookup = if category == AiTaskCategory::RestingHr {
            AiTaskCategory::HeartRate
        } else {
            category
        };
        let (enabled, days_before) = entries
            .iter()
            .find(|(c, _, _)| *c == lookup)
            .map(|(_, e, d)| (*e, *d))
            .unwrap_or((false, 14));
        AiTaskCategoryRange {
            category,
            enabled,
            days_before,
            include_workout_day: true,
            excluded_metrics: Vec::new(),
            picked_days: Vec::new(),
        }
    })
    .collect()
}

fn builtin_payload(
    id: &str,
    name: &str,
    categories: &[(AiTaskCategory, bool, i64)],
    detail_level: AiTaskDetailLevel,
    prompt_template: &str,
    default_summaries: &[&str],
    required_series: &[&str],
) -> String {
    // The seed payload can be a hand-written literal or serde output, but a hand-written literal makes
    // misspelled field names easy and the compiler stays silent. Here we build the struct and serialize it, so
    // the seed script and the read path share one field definition.
    let template = AiTaskTemplate {
        schema_version: AI_TASK_SCHEMA_VERSION,
        id: id.to_string(),
        builtin: true,
        name: name.to_string(),
        name_code: Some(format!("ui.ai_template.{id}.name")),
        categories: builtin_categories(categories),
        detail_level,
        prompt_template: prompt_template.to_string(),
        prompt_code: Some(format!("ui.ai_template.{id}.prompt")),
        default_summaries: default_summaries.iter().map(|s| s.to_string()).collect(),
        required_series: required_series.iter().map(|s| s.to_string()).collect(),
        created_at: BUILTIN_SEED_TIMESTAMP.to_string(),
        updated_at: BUILTIN_SEED_TIMESTAMP.to_string(),
    };
    serde_json::to_string(&template).expect("serializing a built-in template never fails")
}

/// Built-in templates (A7-P1): `builtin` cannot be deleted/overwritten, only saved as a user template.
///
/// The first three are running topics. The last three (sleep, recovery trend, week review) were added in a review.
/// Before, every topic was about running, and people who care mainly about sleep thought this was running software. At startup
/// `INSERT OR IGNORE` seeds them idempotently, so old databases get them on next open.
///
/// `LazyLock`, not `const`: serde generates the payload, so nobody maintains a hand-written
/// JSON literal that no one reviews.
pub(crate) static BUILTIN_TEMPLATE_SEEDS: std::sync::LazyLock<[BuiltinTemplateSeed; 7]> =
    std::sync::LazyLock::new(|| {
        [
            BuiltinTemplateSeed {
                id: "recovery_run",
                name: "Recovery run",
                payload: builtin_payload(
                    "recovery_run",
                    "Recovery run",
                    &[
                        (AiTaskCategory::Workout, true, 7),
                        (AiTaskCategory::Sleep, true, 3),
                        (AiTaskCategory::Recovery, true, 14),
                        (AiTaskCategory::HeartRate, true, 7),
                        (AiTaskCategory::Training, false, 14),
                        (AiTaskCategory::Body, false, 14),
                        (AiTaskCategory::PersonalNote, true, 0),
                        (AiTaskCategory::Attachment, true, 0),
                    ],
                    AiTaskDetailLevel::Standard,
                    "This was a recovery run. Use the sleep, resting HR and HRV of the last few days to judge whether I have recovered, assess whether the intensity of this session was right, and suggest the next session.",
                    &["sleep_score", "resting_hr", "hrv_rmssd"],
                    &["resting_hr", "sleep_hrv", "hrv_rmssd"],
                ),
            },
            BuiltinTemplateSeed {
                id: "long_run_compare",
                name: "Compare long runs",
                payload: builtin_payload(
                    "long_run_compare",
                    "Compare long runs",
                    &[
                        (AiTaskCategory::Workout, true, 28),
                        (AiTaskCategory::Sleep, true, 7),
                        (AiTaskCategory::Recovery, true, 14),
                        (AiTaskCategory::HeartRate, true, 14),
                        (AiTaskCategory::Training, true, 28),
                        (AiTaskCategory::Body, false, 14),
                        (AiTaskCategory::PersonalNote, true, 0),
                        (AiTaskCategory::Attachment, true, 0),
                    ],
                    AiTaskDetailLevel::Detailed,
                    "Compare these long runs: how pace, heart rate, HR drift and perceived fatigue line up. Judge the trend in aerobic fitness and point out which run had the best quality.",
                    &["distance", "avg_hr", "pace"],
                    &["training_load", "resting_hr"],
                ),
            },
            BuiltinTemplateSeed {
                id: "hr_drift",
                name: "HR drift",
                payload: builtin_payload(
                    "hr_drift",
                    "HR drift",
                    &[
                        (AiTaskCategory::Workout, true, 14),
                        (AiTaskCategory::Sleep, true, 3),
                        (AiTaskCategory::Recovery, true, 14),
                        (AiTaskCategory::HeartRate, true, 14),
                        (AiTaskCategory::Training, true, 14),
                        (AiTaskCategory::Body, false, 14),
                        (AiTaskCategory::PersonalNote, true, 0),
                        (AiTaskCategory::Attachment, true, 0),
                    ],
                    AiTaskDetailLevel::Detailed,
                    "Analyze the HR drift of this workout: how far heart rate decouples from pace/power and in which phase. Use recent sleep and recovery data to suggest likely causes.",
                    &["avg_hr", "max_hr", "pace"],
                    &["heart_rate"],
                ),
            },
            BuiltinTemplateSeed {
                id: "sleep_review",
                name: "Recent sleep",
                payload: builtin_payload(
                    "sleep_review",
                    "Recent sleep",
                    &[
                        (AiTaskCategory::Workout, true, 14),
                        (AiTaskCategory::Sleep, true, 14),
                        (AiTaskCategory::Recovery, true, 14),
                        (AiTaskCategory::HeartRate, true, 14),
                        (AiTaskCategory::Training, false, 14),
                        (AiTaskCategory::Body, false, 14),
                        (AiTaskCategory::PersonalNote, true, 0),
                        (AiTaskCategory::Attachment, true, 0),
                    ],
                    AiTaskDetailLevel::Standard,
                    "Look at my sleep over the last two weeks: total duration, whether bedtime and wake time are regular, whether the deep and REM shares changed, and how night HR and HRV moved. Point out the nights that stand out, and use that day's workouts and personal notes to suggest likely causes.",
                    &["sleep_score", "resting_hr"],
                    &[],
                ),
            },
            BuiltinTemplateSeed {
                id: "recovery_trend",
                name: "Recovery trend",
                payload: builtin_payload(
                    "recovery_trend",
                    "Recovery trend",
                    &[
                        (AiTaskCategory::Workout, true, 28),
                        (AiTaskCategory::Sleep, true, 28),
                        (AiTaskCategory::Recovery, true, 28),
                        (AiTaskCategory::HeartRate, true, 28),
                        (AiTaskCategory::Training, true, 28),
                        (AiTaskCategory::Body, false, 28),
                        (AiTaskCategory::PersonalNote, true, 0),
                        (AiTaskCategory::Attachment, true, 0),
                    ],
                    AiTaskDetailLevel::Summary,
                    "Look at whether my recovery got better or worse over the last four weeks: the trend of resting HR, HRV, sleep and training load, and whether they agree. Separate real trends from normal daily variation. Say plainly which days have missing data.",
                    &["resting_hr", "hrv_rmssd", "sleep_score"],
                    &["resting_hr", "hrv_rmssd"],
                ),
            },
            BuiltinTemplateSeed {
                id: "week_review",
                name: "This week",
                payload: builtin_payload(
                    "week_review",
                    "This week",
                    &[
                        (AiTaskCategory::Workout, true, 6),
                        (AiTaskCategory::Sleep, true, 6),
                        (AiTaskCategory::Recovery, true, 6),
                        (AiTaskCategory::HeartRate, true, 6),
                        (AiTaskCategory::Training, true, 6),
                        (AiTaskCategory::Body, true, 6),
                        (AiTaskCategory::PersonalNote, true, 0),
                        (AiTaskCategory::Attachment, true, 0),
                    ],
                    AiTaskDetailLevel::Summary,
                    "Look at this week: what changed in sleep, recovery, heart rate and activity compared with usual, what deserves attention and what is only normal variation. Speak only from the data and do not diagnose. Say plainly which days have missing data.",
                    &["sleep_score", "resting_hr", "training_load"],
                    &[],
                ),
            },
            BuiltinTemplateSeed {
                id: "next_week",
                name: "Plan next week",
                payload: builtin_payload(
                    "next_week", "Plan next week",
                    &[
                        (AiTaskCategory::Workout, true, 29), (AiTaskCategory::Sleep, true, 29),
                        (AiTaskCategory::Recovery, true, 29), (AiTaskCategory::HeartRate, true, 29),
                        (AiTaskCategory::Training, true, 29), (AiTaskCategory::Body, true, 29),
                        (AiTaskCategory::PersonalNote, true, 0), (AiTaskCategory::Attachment, true, 0),
                    ],
                    AiTaskDetailLevel::Standard,
                    "Use my recent training, sleep, recovery and nutrition to discuss next week's plan first, and allow several rounds of changes. Only after I say it is final, output the final plan in the format at the end of the file.",
                    &["sleep_score", "resting_hr", "training_load"], &[],
                ),
            },
        ]
    });
