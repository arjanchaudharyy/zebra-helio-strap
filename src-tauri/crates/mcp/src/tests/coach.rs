//! Coach tools: context does not invent data, drafting does not save bad plans, publishing is gated by the switch, task scope denies as a whole.

use super::*;
use zeppbridge_core::models::DailyMetric;

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

fn day(days_ahead: i64) -> String {
    (Local::now().date_naive() + Duration::days(days_ahead))
        .format("%Y-%m-%d")
        .to_string()
}

fn easy_run_plan() -> Value {
    json!({
        "format": "zeppbridge-plan/3",
        "summary": "One easy run",
        "from": day(1),
        "to": day(3),
        "workouts": [{
            "date": day(2), "sport": "running", "variant": "outdoor",
            "name": "Easy run", "focus": "Aerobic", "description": "Relaxed run",
            "steps": [{ "kind": "active", "duration": "40min", "target": "hr 130-145" }]
        }]
    })
}

fn open_drafts(library: &TestLibrary) -> usize {
    Database::open_read_only(library.0.join("zepp.db"))
        .unwrap()
        .open_plan_drafts()
        .unwrap()
        .len()
}

#[test]
fn training_context_leaves_empty_days_null_instead_of_zero() {
    let library = TestLibrary::empty();
    {
        let db = Database::open_migrated(&library.0.join("zepp.db")).unwrap();
        db.insert_daily_metric(&daily(2, "sleep_hrv", 88.0))
            .unwrap();
        db.insert_daily_metric(&daily(2, "resting_hr", 51.0))
            .unwrap();
        let mut run = run_workout("r1", utc_noon(2), 150);
        run.moving_seconds = Some(3000);
        db.insert_workout(&run).unwrap();
    }
    let result = library.call(
        &AccessScope::FullReadOnly,
        "get_training_context",
        json!({"days": 7}),
    );
    let context = ok(&result);
    let days = context["days"].as_array().unwrap();
    assert_eq!(days.len(), 7, "one row per day, empty days must be there too");
    let with_data = &days[4];
    assert_eq!(with_data["sleepHrv"], 88.0);
    assert_eq!(with_data["restingHr"], 51.0);
    assert_eq!(with_data["workouts"], 1);
    // A day with no samples: the field is null, not 0.
    let empty = &days[5];
    assert!(empty["sleepHrv"].is_null() && empty["restingHr"].is_null());
    assert!(empty["sleepMinutes"].is_null() && empty["steps"].is_null());
    // 10 km / 3000 s of moving time = 300 s/km.
    assert_eq!(context["workouts"][0]["avgPaceSecPerKm"], 300.0);
    assert_eq!(context["daysSinceLastWorkout"], 2);
}

#[test]
fn a_plan_that_fails_validation_is_not_saved() {
    let library = TestLibrary::empty();
    let walk = json!({
        "workouts": [{ "date": day(2), "sport": "walking", "name": "Walk",
                       "steps": [{ "kind": "active", "duration": "30min" }] }]
    });
    let result = library.call(
        &AccessScope::FullReadOnly,
        "draft_training_plan",
        json!({ "plan": walk }),
    );
    let draft = ok(&result);
    assert!(draft["draftId"].is_null());
    assert_eq!(draft["publishable"], false);
    let codes: Vec<&str> = draft["check"]["issues"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|issue| issue["message_code"].as_str())
        .collect();
    assert!(codes.contains(&"ui.training_plan.issue.sport_not_deliverable"));
    assert_eq!(open_drafts(&library), 0, "a plan that fails validation must leave no draft");

    let saved = library.call(
        &AccessScope::FullReadOnly,
        "draft_training_plan",
        json!({ "plan": easy_run_plan() }),
    );
    let saved = ok(&saved);
    assert!(saved["draftId"].as_str().unwrap().starts_with("plan-"));
    assert_eq!(saved["aiMayPublish"], false);
    assert_eq!(open_drafts(&library), 1);
}

#[test]
fn publishing_is_refused_until_the_user_allows_it() {
    let library = TestLibrary::empty();
    let saved = library.call(
        &AccessScope::FullReadOnly,
        "draft_training_plan",
        json!({ "plan": easy_run_plan() }),
    );
    let draft_id = ok(&saved)["draftId"].as_str().unwrap().to_string();

    let refused = library.call(
        &AccessScope::FullReadOnly,
        "publish_training_plan",
        json!({ "draftId": draft_id }),
    );
    assert_eq!(refused["isError"], true);
    assert_eq!(
        refused["structuredContent"]["error"]["code"],
        "err.training_plan.ai_publish_disabled"
    );
    // The ledger did not change at all: the draft is still open and there is no push record.
    let db = Database::open_read_only(library.0.join("zepp.db")).unwrap();
    assert_eq!(db.open_plan_drafts().unwrap().len(), 1);
    assert!(db.plan_publishes(10).unwrap().is_empty());
}

#[test]
fn task_scope_refuses_every_coach_tool() {
    let library = TestLibrary::empty();
    library.share_task("t1", &task_payload("t1", &[], &["sleep"]), true);
    for (name, args) in [
        ("get_training_context", json!({})),
        ("get_athlete_profile", json!({})),
        ("get_training_plan", json!({})),
        ("draft_training_plan", json!({ "plan": easy_run_plan() })),
        ("publish_training_plan", json!({ "draftId": "plan-x" })),
        ("propose_coach_note", json!({ "note": "Left knee hurts" })),
    ] {
        let result = library.call(&AccessScope::TaskScoped, name, args);
        assert_eq!(result["isError"], true, "{name} must be denied in task scope");
    }
    assert_eq!(open_drafts(&library), 0, "drafting in task scope must not write the library");
    let db = Database::open_read_only(library.0.join("zepp.db")).unwrap();
    assert!(db.proposed_coach_notes().unwrap().is_empty());
}

#[test]
fn a_remembered_note_counts_at_once_and_the_user_can_undo_it() {
    let library = TestLibrary::empty();
    let first = library.call(
        &AccessScope::FullReadOnly,
        "propose_coach_note",
        json!({ "note": "Left knee hurts on runs over 15 km" }),
    );
    assert_eq!(ok(&first)["applied"]["note"]["outcome"], "remembered");
    let again = library.call(
        &AccessScope::FullReadOnly,
        "propose_coach_note",
        json!({ "note": "Left knee hurts on runs over 15 km" }),
    );
    assert_eq!(ok(&again)["applied"]["note"]["outcome"], "already_known");

    // No inbox now: the note is in the profile at once, and the source is still mcp.
    let profile = library.call(&AccessScope::FullReadOnly, "get_athlete_profile", json!({}));
    assert_eq!(
        ok(&profile)["coachNotes"][0]["text"],
        "Left knee hurts on runs over 15 km"
    );
    assert_eq!(ok(&profile)["coachNotes"][0]["source"], "mcp");

    let write_id = ok(&first)["applied"]["writeId"].as_i64().unwrap();
    let db = Database::open_migrated(&library.0.join("zepp.db")).unwrap();
    db.undo_mcp_write(write_id).unwrap();
    drop(db);
    let profile = library.call(&AccessScope::FullReadOnly, "get_athlete_profile", json!({}));
    assert_eq!(ok(&profile)["coachNotes"], json!([]));
    let rewrite = library.call(
        &AccessScope::FullReadOnly,
        "propose_coach_note",
        json!({ "note": "Left knee hurts on runs over 15 km" }),
    );
    assert_eq!(
        ok(&rewrite)["applied"]["note"]["outcome"],
        "rejected_before"
    );
}
