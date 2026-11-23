//! The seven browse tools (code review R13): docs table = real tools/list; all callable in full mode;
//! task mode denies whole-library tools, clips by window, and field exclusions do not leak.

use super::*;

use zeppbridge_core::models::{DailyMetric, SourceScope};

/// Names in the docs tool table (the first cell is a backticked name with an underscore; CLI subcommands such as `sync` do not count).
fn documented_tools(doc: &str) -> BTreeSet<String> {
    doc.lines()
        .filter_map(|line| line.strip_prefix("| `"))
        .filter_map(|rest| rest.split_once("` |").map(|(name, _)| name))
        .filter(|name| {
            name.contains('_') && name.chars().all(|c| c.is_ascii_lowercase() || c == '_')
        })
        .map(str::to_string)
        .collect()
}

#[test]
fn the_documented_tools_are_exactly_the_registered_ones() {
    let registered: BTreeSet<String> = tool_definitions()
        .iter()
        .filter_map(|tool| tool["name"].as_str().map(str::to_string))
        .collect();
    for (doc, text) in [
        (
            "cli-and-mcp.md",
            include_str!("../../../../../docs/reference/cli-and-mcp.md"),
        ),
    ] {
        assert_eq!(
            documented_tools(text),
            registered,
            "the tool table in {doc} does not match the real tools/list"
        );
    }
}

fn daily(days_ago: i64, metric: &str, value: f64) -> DailyMetric {
    DailyMetric {
        date: (Local::now().date_naive() - Duration::days(days_ago))
            .format("%Y-%m-%d")
            .to_string(),
        metric: metric.into(),
        value,
        unit: "u".into(),
        source_scope: SourceScope::Device,
        device_id: Some("ring-9".into()),
    }
}

fn ok(result: &Value) -> &Value {
    assert_eq!(result["isError"], false, "{}", result["content"][0]["text"]);
    &result["structuredContent"]
}

fn assert_denied(result: &Value) {
    assert_eq!(result["isError"], true);
    assert_eq!(
        result["structuredContent"]["error"]["code"],
        json!(access::SCOPE_DENIED),
        "{}",
        result["content"][0]["text"]
    );
}

/// A small library with an anchor workout, sleep and two daily values. Anchor 10 days ago -> window [today-24, today-10].
fn small_library() -> TestLibrary {
    let library = TestLibrary::empty();
    let db = Database::open_migrated(&library.0.join("zepp.db")).unwrap();
    db.insert_workout(&run_workout("anchor", utc_noon(10), 140))
        .unwrap();
    db.insert_workout(&run_workout("older", utc_noon(40), 150))
        .unwrap();
    db.insert_sleep_session(&sleep_session_days_ago("in-window", 12))
        .unwrap();
    db.insert_sleep_session(&sleep_session_days_ago("too-new", 1))
        .unwrap();
    db.insert_daily_metric(&daily(12, "spo2_odi", 1.5)).unwrap();
    db.insert_daily_metric(&daily(2, "spo2_odi", 9.9)).unwrap();
    db.insert_daily_metric(&daily(12, "mystery_metric", 3.0))
        .unwrap();
    library
}

#[test]
fn full_scope_answers_every_new_tool() {
    let library = small_library();
    let full = AccessScope::FullReadOnly;
    for (name, args) in [
        ("get_food_data", json!({"days": 30})),
        ("list_available_metrics", json!({})),
        (
            "get_metric_records",
            json!({"metric": "mystery_metric", "source": "daily_metrics"}),
        ),
        ("list_sleep_sessions", json!({"limit": 1})),
        ("get_workout_detail", json!({"workoutId": "anchor"})),
        (
            "get_workout_series",
            json!({"workoutId": "anchor", "section": "summary"}),
        ),
        ("list_life_events", json!({})),
    ] {
        let result = library.call(&full, name, args);
        assert_eq!(
            result["isError"], false,
            "{name}: {}",
            result["content"][0]["text"]
        );
    }
    let sleep = library.call(&full, "list_sleep_sessions", json!({"limit": 1}));
    assert_eq!(ok(&sleep)["hasMore"], true);
    let inventory = library.call(&full, "list_available_metrics", json!({}));
    let names: Vec<&str> = ok(&inventory)["metrics"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|metric| metric["metric"].as_str())
        .collect();
    assert!(
        names.contains(&"mystery_metric"),
        "metrics outside the contract must be discoverable too: {names:?}"
    );
    // list_workouts has paging again: the second page is the older one.
    let page = library.call(&full, "list_workouts", json!({"limit": 1, "offset": 1}));
    assert_eq!(ok(&page)["workouts"][0]["workoutId"], json!("older"));
    assert_eq!(ok(&page)["hasMore"], false);
    // A wrong date is reported clearly.
    let bad = library.call(&full, "list_life_events", json!({"startDate": "9/1"}));
    assert_eq!(bad["isError"], true);
}

#[test]
fn task_scope_refuses_whole_library_tools_and_food_entries() {
    let library = small_library();
    library.share_task(
        "t1",
        &task_payload("t1", &["anchor"], &["sleep", "body"]),
        true,
    );
    let task = AccessScope::TaskScoped;
    assert_denied(&library.call(&task, "list_available_metrics", json!({})));
    assert_denied(&library.call(&task, "list_life_events", json!({})));
    // Food: only daily totals (body category); single entries do not go out.
    let food = library.call(&task, "get_food_data", json!({"days": 30}));
    let food = ok(&food);
    assert!(food.get("entries").is_none(), "task mode must not leak single food entries");
    assert_eq!(food["mealDetailsAvailable"], false);
}

#[test]
fn task_metric_records_and_sleep_list_stay_inside_granted_windows() {
    let library = small_library();
    let payload = json!({
        "id": "t1",
        "workout_ids": ["anchor"],
        "categories": [{
            "category": "sleep", "enabled": true, "days_before": 14,
            "include_workout_day": true, "excluded_metrics": ["score"],
        }],
    })
    .to_string();
    library.share_task("t1", &payload, true);
    let task = AccessScope::TaskScoped;

    let records = library.call(
        &task,
        "get_metric_records",
        json!({"metric": "spo2_odi", "source": "daily_metrics"}),
    );
    let values: Vec<f64> = ok(&records)["records"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|record| record["value"].as_f64())
        .collect();
    assert_eq!(values, vec![1.5], "readings outside the window must never appear");

    // Metrics that fit no category: fail closed.
    assert_denied(&library.call(
        &task,
        "get_metric_records",
        json!({"metric": "mystery_metric", "source": "daily_metrics"}),
    ));

    let sessions = library.call(&task, "list_sleep_sessions", json!({}));
    let sessions = ok(&sessions)["sessions"].as_array().unwrap().clone();
    let ids: Vec<&str> = sessions
        .iter()
        .filter_map(|s| s["sleepId"].as_str())
        .collect();
    assert_eq!(ids, vec!["in-window"]);
    assert!(sessions[0].get("score").is_none(), "the task excluded the sleep score");
    assert!(sessions[0].get("durationMinutes").is_some());
}

#[test]
fn task_workout_detail_and_series_respect_exclusions_and_withhold_gps() {
    let library = small_library();
    let with_exclusion = json!({
        "id": "t1",
        "workout_ids": ["anchor"],
        "categories": [{
            "category": "workout", "enabled": true, "days_before": 0,
            "include_workout_day": true, "excluded_metrics": ["avg_hr"],
        }],
    })
    .to_string();
    library.share_task("t1", &with_exclusion, true);
    let task = AccessScope::TaskScoped;

    let detail = library.call(&task, "get_workout_detail", json!({"workoutId": "anchor"}));
    let workout = &ok(&detail)["workout"];
    assert!(workout.get("avg_hr").is_none());
    assert!(
        workout.get("hr_zones").is_none(),
        "when heart rate is excluded, heart rate zones do not go out either"
    );
    assert!(workout.get("device_id").is_none());
    assert!(workout.get("distance_meters").is_some());
    // An ungranted workout is denied as a whole.
    assert_denied(&library.call(&task, "get_workout_detail", json!({"workoutId": "older"})));
    // Fields excluded -> the per-point series is denied as a whole; the track is never given.
    assert_denied(&library.call(
        &task,
        "get_workout_series",
        json!({"workoutId": "anchor", "section": "samples"}),
    ));
    library.share_task("t1", &task_payload("t1", &["anchor"], &["workout"]), true);
    let summary = library.call(
        &task,
        "get_workout_series",
        json!({"workoutId": "anchor", "section": "summary"}),
    );
    ok(&summary);
    assert_denied(&library.call(
        &task,
        "get_workout_series",
        json!({"workoutId": "anchor", "section": "route"}),
    ));
}

/// R12: a task that selects no workout ("sleep for the last 14 days") also has a window in MCP, the same range as the export.
#[test]
fn a_task_without_workouts_still_opens_its_recent_window() {
    let library = small_library();
    library.share_task("t1", &task_payload("t1", &[], &["sleep"]), true);
    let result = library.call(
        &AccessScope::TaskScoped,
        "get_metric_series",
        json!({"metrics": ["spo2_odi"], "days": 30}),
    );
    let series = &ok(&result)["series"][0];
    // Window [today-14, today]: the one from 2 days ago is inside, and so is the one from 12 days ago.
    assert_eq!(series["window_days"], 15);
    assert_eq!(series["points"].as_array().unwrap().len(), 2);
}
