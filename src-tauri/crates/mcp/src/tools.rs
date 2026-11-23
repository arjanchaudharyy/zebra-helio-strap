//! Tool calls: open the read-only library, build data requests from the grant scope, and build denial and failure replies (split out of main.rs, same logic).

use super::*;

pub(super) fn open_db() -> Result<(Database, u64), (i64, String)> {
    let dir = paths::resolve_data_dir()
        .map_err(|error| (ERR_DATABASE, format!("Cannot determine the data directory: {error}")))?;
    let db_path = dir.join("zepp.db");
    if !db_path.exists() {
        return Err((
            ERR_NOT_CONFIGURED,
            "There is no ZeppBridge database on this computer yet. Connect an account in the desktop app and sync once first.".into(),
        ));
    }
    let bytes = std::fs::metadata(&db_path)
        .map(|meta| meta.len())
        .unwrap_or(0);
    // query_only connection: SQLite itself rejects writes. Read-only does not depend on a branch here.
    let db =
        Database::open_read_only(db_path).map_err(|error| (ERR_DATABASE, error.user_message()))?;
    Ok((db, bytes))
}

pub(super) fn call_tool(params: &Value, scope: &AccessScope) -> Result<Value, (i64, String)> {
    call_tool_with_db(params, scope, open_db)
}

pub(super) fn call_tool_with_db(
    params: &Value,
    scope: &AccessScope,
    open: impl FnOnce() -> Result<(Database, u64), (i64, String)>,
) -> Result<Value, (i64, String)> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or((ERR_INVALID_PARAMS, "missing tool name".to_string()))?;
    if !tool_definitions()
        .iter()
        .any(|tool| tool["name"].as_str() == Some(name))
    {
        return Err((ERR_METHOD_NOT_FOUND, format!("Unknown tool: {name}")));
    }
    if params
        .get("arguments")
        .is_some_and(|args| !args.is_object())
    {
        return Err((ERR_INVALID_PARAMS, "arguments must be an object".into()));
    }
    Ok(execute_tool_with_db(name, params, scope, open))
}

/// The scope self-description in a tool result. `grants` is the number of shared tasks this call actually saw;
/// scope denials carry it too, so the client can tell "not granted" from "task mode is off".
pub(super) fn scope_json(scope: &AccessScope, grants: usize) -> Value {
    json!({ "mode": scope.as_str(), "grants": grants })
}

/// Grant windows serialize as `{category,start,end}`. The `permittedRanges` in a denial payload
/// uses it to tell the caller "what is actually allowed", so the caller can retry with that (R3).
pub(super) fn windows_json(windows: &[access::GrantWindow]) -> Value {
    Value::Array(
        windows
            .iter()
            .map(|window| {
                json!({
                    "category": window.category.as_str(),
                    "start": window.start_date.format("%Y-%m-%d").to_string(),
                    "end": window.end_date.format("%Y-%m-%d").to_string(),
                })
            })
            .collect(),
    )
}

/// Plain tool failure: one sentence for the model (same shape as before), with the scope block attached as usual.
pub(super) fn tool_error(scope: &AccessScope, grants: usize, message: String) -> Value {
    json!({
        "content": [{"type":"text", "text": message}],
        "isError": true,
        "scope": scope_json(scope, grants),
    })
}

/// Scope denial: a stable code + the actual grant windows go into `structuredContent.error`, so clients
/// can branch without parsing the message text; text also carries the code, so text-only clients see it.
pub(super) fn scope_denial(
    scope: &AccessScope,
    grants: usize,
    code: &'static str,
    reason: String,
    permitted_ranges: Value,
) -> Value {
    json!({
        "content": [{"type":"text", "text": format!("{code}: {reason}")}],
        "isError": true,
        "scope": scope_json(scope, grants),
        "structuredContent": {
            "error": {
                "code": code,
                "reason": reason,
                "permittedRanges": permitted_ranges,
            },
            "scope": scope_json(scope, grants),
        },
    })
}

/// The failure exit while a tool runs. A non-empty `denial` means a scope denial (stable code + grant windows);
/// a non-empty `code` means another failure with a stable code (training plan tools). The code goes into `structuredContent.error`.
pub(super) struct CallFailure {
    pub(super) message: String,
    pub(super) denial: Option<(&'static str, Value)>,
    pub(super) code: Option<&'static str>,
}

impl CallFailure {
    pub(super) fn plain(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            denial: None,
            code: None,
        }
    }

    pub(super) fn denied(message: impl Into<String>, permitted_ranges: Value) -> Self {
        Self {
            message: message.into(),
            denial: Some((access::SCOPE_DENIED, permitted_ranges)),
            code: None,
        }
    }

    pub(super) fn coded(code: &'static str, message: String) -> Self {
        Self {
            message,
            denial: None,
            code: Some(code),
        }
    }
}

/// A plain failure with a stable code: the code and the message go into `structuredContent.error`, and text carries the code too.
pub(super) fn coded_error(
    scope: &AccessScope,
    grants: usize,
    code: &'static str,
    reason: String,
) -> Value {
    json!({
        "content": [{"type":"text", "text": format!("{code}: {reason}")}],
        "isError": true,
        "scope": scope_json(scope, grants),
        "structuredContent": {
            "error": { "code": code, "reason": reason },
            "scope": scope_json(scope, grants),
        },
    })
}

/// Tool call -> grant request. Every registered tool must have one mapping line here;
/// `every_registered_tool_builds_a_data_request` guards this invariant (R10).
pub(super) fn build_request(name: &str, args: &Value) -> Result<DataRequest, String> {
    let mut request = DataRequest::default();
    match name {
        "list_workouts" => {
            request.tool = "list_workouts";
            request.categories = vec![AccessCategory::Workout];
        }
        "get_workout_insight" => {
            request.tool = "get_workout_insight";
            request.categories = vec![AccessCategory::Workout];
            let workout_id = args
                .get("workoutId")
                .and_then(Value::as_str)
                .ok_or_else(|| "missing workoutId".to_string())?;
            request.workout_ids = vec![workout_id.to_string()];
        }
        "get_metric_series" => {
            request.tool = "get_metric_series";
            let metrics = metric_args(args)?;
            let mut categories = Vec::new();
            for metric in &metrics {
                // Metrics outside the contract produce no data anyway; only mapped metrics get a window.
                // Anything the mapping table cannot find naturally gets no data points in task scope.
                if let Some(category) = access::metric_category(metric) {
                    if !categories.contains(&category) {
                        categories.push(category);
                    }
                }
            }
            request.categories = categories;
            request.metrics = metrics;
            request.date_range = Some(days_back(args));
        }
        "get_sleep_detail" => {
            request.tool = "get_sleep_detail";
            request.categories = vec![AccessCategory::Sleep];
            request.latest_sleep = args.get("sleepId").and_then(Value::as_str).is_none();
        }
        "get_data_health" => {
            request.tool = "get_data_health";
            // Coverage gaps, last sync time and newest_sample_at are whole-library values. No honest
            // subset exists, so task mode denies it as a whole (R5).
            request.whole_library = true;
        }
        // Whole-library lists / categories the task model lacks (single food entries, life events): task mode denies them as a whole,
        // rather than give something that looks complete but was cut (code review R13).
        "list_available_metrics" => {
            request.tool = "list_available_metrics";
            request.whole_library = true;
        }
        // Long-term baselines need the full history (up to ten years). A task grant window cannot give an honest "baseline", so deny it as a whole.
        "get_baselines" => {
            request.tool = "get_baselines";
            request.metrics = metric_args(args)?;
            request.whole_library = true;
        }
        // Zebra strap files: outside the database, whole-library only.
        "get_strap_heart_rate" | "get_strap_alerts" => {
            request.tool = if name == "get_strap_heart_rate" { "get_strap_heart_rate" } else { "get_strap_alerts" };
            request.whole_library = true;
        }
        "list_life_events" => {
            request.tool = "list_life_events";
            date_args(args)?;
            request.whole_library = true;
        }
        "get_food_data" => {
            // Daily intake totals belong to body (access::metric_category); single entries do not go out in task mode.
            request.tool = "get_food_data";
            request.categories = vec![AccessCategory::Body];
            request.metrics = FOOD_METRICS.map(str::to_string).to_vec();
            request.date_range = Some(days_back(args));
        }
        "get_metric_records" => {
            request.tool = "get_metric_records";
            let (metric, source) = metric_record_args(args)?;
            // Metrics with no category: categories stays empty, and the executor denies it in task mode (fail closed).
            if let Some(category) = record_category(metric, source) {
                request.categories = vec![category];
                request.metrics = vec![metric.to_string()];
            }
            let (start, end) = date_args(args)?;
            if let (Some(start), Some(end)) = (start, end) {
                request.date_range = Some((start, end));
            }
        }
        "list_sleep_sessions" => {
            request.tool = "list_sleep_sessions";
            request.categories = vec![AccessCategory::Sleep];
        }
        "get_workout_detail" | "get_workout_series" => {
            request.tool = if name == "get_workout_detail" {
                "get_workout_detail"
            } else {
                "get_workout_series"
            };
            request.categories = vec![AccessCategory::Workout];
            let workout_id = args
                .get("workoutId")
                .and_then(Value::as_str)
                .ok_or_else(|| "missing workoutId".to_string())?;
            request.workout_ids = vec![workout_id.to_string()];
        }
        // Coach tools: they cross categories and dates (or use the training plan ledger). Task scope has no honest subset, so deny them as a whole.
        "get_training_context"
        | "get_athlete_profile"
        | "get_training_plan"
        | "draft_training_plan"
        | "publish_training_plan"
        | "propose_coach_note" => {
            request.tool = COACH_TOOLS
                .iter()
                .find(|tool| **tool == name)
                .copied()
                .unwrap_or_default();
            request.whole_library = true;
        }
        // Write tools and actions change the whole history or the whole library. Task scope always denies them.
        name if WRITE_TOOLS.contains(&name) || ACTION_TOOLS.contains(&name) => {
            request.tool = WRITE_TOOLS
                .iter()
                .chain(ACTION_TOOLS.iter())
                .find(|tool| **tool == name)
                .copied()
                .unwrap_or_default();
            request.whole_library = true;
        }
        other => {
            return Err(format!("There is no tool named {other}."));
        }
    }
    Ok(request)
}

/// The five coach tools, plus "propose a note to remember".
pub(super) const COACH_TOOLS: [&str; 6] = [
    "get_training_context",
    "get_athlete_profile",
    "get_training_plan",
    "draft_training_plan",
    "publish_training_plan",
    "propose_coach_note",
];

/// The four intake metrics that `get_food_data` totals by day.
pub(super) const FOOD_METRICS: [&str; 4] = [
    "intake_calories",
    "intake_protein_g",
    "intake_fat_g",
    "intake_carbs_g",
];

/// Converts `days` (including today) into a local day range, with the same window algorithm as `storage::metric_series`.
pub(super) fn days_back(args: &Value) -> (NaiveDate, NaiveDate) {
    let days = args
        .get("days")
        .and_then(Value::as_i64)
        .unwrap_or(90)
        .clamp(1, 1825);
    let end = Local::now().date_naive();
    (end - Duration::days(days - 1), end)
}

/// `startDate` / `endDate`: if given, they must be YYYY-MM-DD. A wrong value is reported, not silently treated as missing.
pub(super) fn date_args(args: &Value) -> Result<(Option<NaiveDate>, Option<NaiveDate>), String> {
    let read = |field: &str| -> Result<Option<NaiveDate>, String> {
        match args.get(field) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(text)) => NaiveDate::parse_from_str(text, "%Y-%m-%d")
                .map(Some)
                .map_err(|_| format!("{field} must be written as YYYY-MM-DD")),
            Some(_) => Err(format!("{field} must be written as YYYY-MM-DD")),
        }
    };
    Ok((read("startDate")?, read("endDate")?))
}

pub(super) fn metric_record_args(args: &Value) -> Result<(&str, &str), String> {
    let metric = args
        .get("metric")
        .and_then(Value::as_str)
        .ok_or_else(|| "missing metric".to_string())?;
    let source = args
        .get("source")
        .and_then(Value::as_str)
        .ok_or_else(|| "missing source".to_string())?;
    if !["daily_metrics", "metric_samples", "sleep_sessions"].contains(&source) {
        return Err(format!(
            "unknown source: {source}. Options: daily_metrics, metric_samples, sleep_sessions."
        ));
    }
    Ok((metric, source))
}

/// Which grant category a single reading belongs to: scores in the sleep table belong to sleep, the rest map by metric name.
pub(super) fn record_category(metric: &str, source: &str) -> Option<AccessCategory> {
    if source == "sleep_sessions" {
        Some(AccessCategory::Sleep)
    } else {
        access::metric_category(metric)
    }
}

/// Paging arguments: `limit` is clamped to each tool's own cap, `offset` is at most one million.
pub(super) fn page_args(args: &Value, default: u64, max: u64) -> (usize, usize) {
    let limit = args
        .get("limit")
        .and_then(Value::as_u64)
        .unwrap_or(default)
        .clamp(1, max) as usize;
    let offset = args
        .get("offset")
        .and_then(Value::as_u64)
        .unwrap_or(0)
        .min(1_000_000) as usize;
    (limit, offset)
}

pub(super) fn metric_args(args: &Value) -> Result<Vec<String>, String> {
    let metrics: Vec<String> = args
        .get("metrics")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    if metrics.is_empty() {
        return Err("metrics must not be empty".into());
    }
    Ok(metrics)
}

pub(super) fn execute_tool_with_db(
    name: &str,
    params: &Value,
    scope: &AccessScope,
    open: impl FnOnce() -> Result<(Database, u64), (i64, String)>,
) -> Value {
    let args = params.get("arguments").cloned().unwrap_or(json!({}));
    let (db, database_bytes) = match open() {
        Ok(pair) => pair,
        Err((_code, message)) => return tool_error(scope, 0, message),
    };

    // The grant is reread on every call: the connection is reopened per request anyway (every `tools/call`
    // builds a new read-only connection in `open_db`). A new connection gets a new WAL read snapshot, so
    // flipping "Share with MCP" on the task page takes effect on the next request. In task mode, a grant that cannot be read
    // means the grant cannot be proven. Fail closed: do not pretend it exists.
    let grants = match access::shared_task_grants(&db) {
        Ok(grants) => grants,
        Err(error) => {
            if scope.is_task_scoped() {
                return scope_denial(
                    scope,
                    0,
                    access::SCOPE_DENIED,
                    format!("Reading task grants failed, treated as not granted: {}", error.user_message()),
                    json!([]),
                );
            }
            Vec::new()
        }
    };

    let request = match build_request(name, &args) {
        Ok(request) => request,
        Err(message) => return tool_error(scope, grants.len(), message),
    };
    let permit = match access::authorize(scope, &grants, &request) {
        Ok(permit) => permit,
        Err(denied) => {
            let ranges = windows_json(&access::granted_windows(&grants, &request.categories, None));
            return scope_denial(scope, grants.len(), denied.code, denied.reason, ranges);
        }
    };

    let mut payload = match run_tool(name, &args, &db, database_bytes, scope, &permit) {
        Ok(payload) => payload,
        Err(failure) => match (failure.denial, failure.code) {
            (Some((code, ranges)), _) => {
                return scope_denial(scope, grants.len(), code, failure.message, ranges);
            }
            (None, Some(code)) => return coded_error(scope, grants.len(), code, failure.message),
            (None, None) => return tool_error(scope, grants.len(), failure.message),
        },
    };

    // Metrics the user excluded (metric_prefs): remove them from the reply and state it is the user's choice. If the list cannot be read, reply as usual:
    // exclusion is a user preference, not a grant boundary.
    if let Ok(excluded) = db.excluded_metrics() {
        zeppbridge_core::storage::metric_prefs::apply_exclusions(&mut payload, &excluded);
    }

    // Second safeguard for task scope (R6): the field-whitelist exits have no device_id anyway;
    // exits that serde a whole struct (sleep detail) rely on this recursive strip as a backstop.
    if scope.is_task_scoped() {
        access::strip_identity_fields(&mut payload);
    }
    if let Some(object) = payload.as_object_mut() {
        object.insert("scope".to_string(), scope_json(scope, grants.len()));
    }

    // MCP content is text for the model to read; structured data also goes into structuredContent,
    // so clients that can use structure do not parse the string again. The text uses compact serialization: the same
    // JSON, equal after the client parses it, without the whitespace cost of pretty printing.
    let text = payload.to_string();
    json!({
        "content": [{ "type": "text", "text": text }],
        "structuredContent": payload,
        "isError": false,
        "scope": scope_json(scope, grants.len()),
    })
}

pub(super) fn run_tool(
    name: &str,
    args: &Value,
    db: &Database,
    database_bytes: u64,
    scope: &AccessScope,
    permit: &Permit,
) -> Result<Value, CallFailure> {
    match name {
        "list_workouts" => run_list_workouts(db, args, scope, permit),
        "get_workout_insight" => run_workout_insight(db, args, scope, permit),
        "get_metric_series" => run_metric_series(db, args, scope, permit),
        "get_sleep_detail" => run_sleep_detail(db, args, scope, permit),
        "get_data_health" => run_data_health(db, args, database_bytes),
        "get_food_data" => run_food_data(db, args, scope, permit),
        "list_available_metrics" => run_available_metrics(db),
        "get_baselines" => run_baselines(db, args),
        "get_metric_records" => run_metric_records(db, args, scope, permit),
        "list_sleep_sessions" => run_sleep_sessions(db, args, scope, permit),
        "get_workout_detail" => run_workout_detail(db, args, scope, permit),
        "get_workout_series" => run_workout_series(db, args, scope, permit),
        "list_life_events" => run_life_events(db, args),
        "get_training_context" => run_training_context(db, args),
        "get_athlete_profile" => run_athlete_profile(db),
        "get_strap_heart_rate" => strap::run_strap_heart_rate(db, args),
        "get_strap_alerts" => strap::run_strap_alerts(db),
        "get_training_plan" => run_training_plan(db),
        "draft_training_plan" => run_draft_plan(db, args),
        "publish_training_plan" => run_publish_plan(db, args),
        "propose_coach_note" => run_remember_note(db, args),
        "sync_now" => run_sync(db),
        "export_data" => run_export(db, args),
        name if WRITE_TOOLS.contains(&name) => run_write(db, name, args),
        other => Err(CallFailure::plain(format!("There is no tool named {other}."))),
    }
}
