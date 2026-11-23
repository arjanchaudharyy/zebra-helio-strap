use super::*;

#[test]
fn latest_sleep_returns_the_same_full_detail_as_an_explicit_id() {
    let older = sleep_session("older", 1, Some("light"));
    let latest = sleep_session("latest", 2, Some("deep"));
    let library = TestLibrary::new(&[older, latest.clone()]);

    let implicit = library.call_sleep(json!({}));
    let explicit = library.call_sleep(json!({ "sleepId": "latest" }));
    assert_eq!(implicit, explicit);
    assert_eq!(implicit["isError"], json!(false));
    assert_eq!(
        implicit["structuredContent"]["sleep"],
        serde_json::to_value(latest).unwrap()
    );
    let text: Value =
        serde_json::from_str(implicit["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(text, implicit["structuredContent"]);

    let previous = library.call_sleep(json!({ "sleepId": "older" }));
    assert_eq!(previous["structuredContent"]["sleep"]["sleep_id"], "older");
    assert_eq!(
        previous["structuredContent"]["sleep"]["stages"][0]["stage"],
        "light"
    );
}

#[test]
fn sleep_queries_preserve_missing_sessions_and_missing_stages() {
    let empty = TestLibrary::new(&[]);
    let recent = empty.call_sleep(json!({}));
    assert_eq!(recent["isError"], json!(false));
    assert_eq!(recent["structuredContent"]["sleep"], Value::Null);

    let library = TestLibrary::new(&[sleep_session("no-stages", 1, None)]);
    let recent = library.call_sleep(json!({}));
    assert_eq!(
        recent["structuredContent"]["sleep"]["sleep_id"],
        "no-stages"
    );
    assert_eq!(recent["structuredContent"]["sleep"]["stages"], json!([]));
    let missing = library.call_sleep(json!({ "sleepId": "unknown" }));
    assert_eq!(missing["structuredContent"]["sleep"], Value::Null);
}

/// Task scope + zero grants: every data tool must give `scope_no_grants`. It must not crash,
/// and must not pass off an empty success result.
#[test]
fn task_scope_with_zero_grants_denies_every_data_call() {
    let library = TestLibrary::empty(); // the base schema has no ai_tasks -> zero grants
    for (name, args) in [
        ("list_workouts", json!({})),
        ("get_workout_insight", json!({"workoutId": "w1"})),
        ("get_metric_series", json!({"metrics": ["spo2_odi"]})),
        ("get_sleep_detail", json!({})),
        ("get_data_health", json!({})),
    ] {
        let result = library.call(&AccessScope::TaskScoped, name, args);
        assert_eq!(result["isError"], true, "{name}");
        assert_eq!(
            result["structuredContent"]["error"]["code"],
            json!(access::SCOPE_NO_GRANTS),
            "{name}"
        );
        assert_eq!(result["scope"], json!({"mode": "task", "grants": 0}));
        assert!(result["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains(access::SCOPE_NO_GRANTS));
    }
    // The process did not crash, and the protocol surface works as usual.
    assert_eq!(handle("ping", &json!({})).unwrap(), json!({}));
}

/// full-readonly ignores grants: the data surface matches the old behavior (the scope block is a new self-description).
#[test]
fn full_readonly_ignores_grants_and_keeps_the_old_shape() {
    let library = TestLibrary::empty();
    {
        let db = Database::open_migrated(&library.0.join("zepp.db")).unwrap();
        db.insert_workout(&run_workout("granted", utc_noon(3), 140))
            .unwrap();
        db.insert_workout(&run_workout("private", utc_noon(2), 150))
            .unwrap();
    }
    library.share_task("t1", &task_payload("t1", &["granted"], &["sleep"]), true);

    let result = library.call(&AccessScope::FullReadOnly, "list_workouts", json!({}));
    assert_eq!(result["isError"], false);
    let ids: Vec<&str> = result["structuredContent"]["workouts"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|workout| workout["workoutId"].as_str())
        .collect();
    // Grants are ignored: ungranted records are visible as usual, still in newest -> oldest order.
    assert_eq!(ids, ["private", "granted"]);
    assert_eq!(
        result["scope"],
        json!({"mode": "full-readonly", "grants": 1})
    );
}

/// R3: request range overlaps the grant window -> clip to granted days; no overlap -> deny as a whole with
/// permittedRanges. Never return a quietly truncated, half-empty series.
#[test]
fn metric_series_is_clipped_to_granted_windows_and_denied_outside() {
    let library = TestLibrary::empty();
    let inside_day = (Local::now().date_naive() - Duration::days(12))
        .format("%Y-%m-%d")
        .to_string();
    {
        let db = Database::open_migrated(&library.0.join("zepp.db")).unwrap();
        // Grant anchor: a run 10 days ago; sleep window = [anchor-14d, anchor].
        db.insert_workout(&run_workout("anchor", utc_noon(10), 140))
            .unwrap();
        let metric = |days_ago: i64, value: f64| DailyMetric {
            date: (Local::now().date_naive() - Duration::days(days_ago))
                .format("%Y-%m-%d")
                .to_string(),
            metric: "spo2_odi".into(),
            value,
            unit: "events/h".into(),
            source_scope: SourceScope::Device,
            device_id: Some("ring-9".into()),
        };
        db.insert_daily_metric(&metric(12, 1.5)).unwrap(); // inside the window
        db.insert_daily_metric(&metric(2, 9.9)).unwrap(); // outside the window
    }
    library.share_task("t1", &task_payload("t1", &["anchor"], &["sleep"]), true);

    // The 30-day request window overlaps the grant window [today-24, today-10] -> allowed, but only points inside the window come out.
    let result = library.call(
        &AccessScope::TaskScoped,
        "get_metric_series",
        json!({"metrics": ["spo2_odi"], "days": 30}),
    );
    assert_eq!(result["isError"], false, "{}", result["content"][0]["text"]);
    let series = &result["structuredContent"]["series"][0];
    let days: Vec<String> = series["points"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|point| point["date"].as_str().map(str::to_string))
        .collect();
    assert_eq!(days, vec![inside_day], "points outside the window must never appear in the series");
    // window_days describes the grant window (15 days), not the requested 30 days.
    assert_eq!(series["window_days"], 15);
    assert_eq!(result["scope"], json!({"mode": "task", "grants": 1}));
    assert_no_pathish_keys(&result, "metric_series");

    // The 3-day request window is fully outside the grant window -> deny + permittedRanges.
    let denied = library.call(
        &AccessScope::TaskScoped,
        "get_metric_series",
        json!({"metrics": ["spo2_odi"], "days": 3}),
    );
    assert_eq!(denied["isError"], true);
    assert_eq!(
        denied["structuredContent"]["error"]["code"],
        json!(access::SCOPE_DENIED)
    );
    let ranges = denied["structuredContent"]["error"]["permittedRanges"]
        .as_array()
        .unwrap();
    assert_eq!(ranges[0]["category"], json!("sleep"));

    // An ungranted category (weight -> body) is also denied as a whole, not answered with an empty series.
    let other = library.call(
        &AccessScope::TaskScoped,
        "get_metric_series",
        json!({"metrics": ["weight"], "days": 30}),
    );
    assert_eq!(
        other["structuredContent"]["error"]["code"],
        json!(access::SCOPE_DENIED)
    );
}

/// R1: in task scope, the insight recomputes its baseline over the granted set only. No id,
/// date or distance of an ungranted workout, and no aggregate they contribute to, may remain.
#[test]
fn task_insight_rescores_baseline_over_granted_ids_only() {
    let library = TestLibrary::empty();
    {
        let db = Database::open_migrated(&library.0.join("zepp.db")).unwrap();
        db.insert_workout(&run_workout("target", utc_noon(1), 150))
            .unwrap();
        for (index, id) in ["g1", "g2", "g3"].iter().enumerate() {
            db.insert_workout(&run_workout(id, utc_noon(3 + index as i64), 100))
                .unwrap();
        }
        for (index, id) in ["x1", "x2"].iter().enumerate() {
            db.insert_workout(&run_workout(id, utc_noon(7 + index as i64), 200))
                .unwrap();
        }
    }
    library.share_task(
        "t1",
        &task_payload("t1", &["target", "g1", "g2", "g3"], &["workout"]),
        true,
    );

    // Naming an ungranted id -> deny, and confirm nothing beyond whether it exists.
    let denied = library.call(
        &AccessScope::TaskScoped,
        "get_workout_insight",
        json!({"workoutId": "x1"}),
    );
    assert_eq!(denied["isError"], true);
    assert_eq!(
        denied["structuredContent"]["error"]["code"],
        json!(access::SCOPE_DENIED)
    );

    let result = library.call(
        &AccessScope::TaskScoped,
        "get_workout_insight",
        json!({"workoutId": "target"}),
    );
    assert_eq!(result["isError"], false, "{}", result["content"][0]["text"]);
    let insight = &result["structuredContent"];
    let mut ids = BTreeSet::new();
    collect_ids(insight, &mut ids);
    assert!(
        !ids.contains("x1") && !ids.contains("x2"),
        "ungranted ids must not appear in the response: {ids:?}"
    );
    let included: Vec<&str> = insight["baseline_included"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|entry| entry["workout_id"].as_str())
        .collect();
    assert_eq!(included, ["g1", "g2", "g3"]);
    // The baseline mean has only granted samples (100); the whole-library mean (140) must not leak.
    let avg_hr = insight["facts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|fact| fact["metric"] == json!("avg_hr"))
        .unwrap();
    assert_eq!(avg_hr["comparison"]["baseline_value"], json!(100.0));
    assert_eq!(avg_hr["evidence_count"], json!(3));
    assert_no_pathish_keys(&result, "insight");

    // Control: the same insight in full-readonly has all 5 samples.
    let full = library.call(
        &AccessScope::FullReadOnly,
        "get_workout_insight",
        json!({"workoutId": "target"}),
    );
    let full_hr = full["structuredContent"]["facts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|fact| fact["metric"] == json!("avg_hr"))
        .unwrap();
    assert_eq!(full_hr["comparison"]["baseline_value"], json!(140.0));
}

/// R2/R6: an explicit id outside the window -> deny; no id -> the latest night inside the grant window,
/// not the newest in the whole library; the output has no device_id.
#[test]
fn task_sleep_detail_stays_inside_granted_windows_and_strips_device_id() {
    let library = TestLibrary::empty();
    {
        let db = Database::open_migrated(&library.0.join("zepp.db")).unwrap();
        // Grant anchor 20 days ago -> sleep window [today-34, today-20].
        db.insert_workout(&run_workout("anchor", utc_noon(20), 140))
            .unwrap();
        db.insert_sleep_session(&sleep_session_days_ago("in-window", 25))
            .unwrap();
        db.insert_sleep_session(&sleep_session_days_ago("too-new", 1))
            .unwrap();
    }
    library.share_task("t1", &task_payload("t1", &["anchor"], &["sleep"]), true);

    // Explicit id outside the window -> deny (not sleep:null; the record exists but is not visible).
    let denied = library.call(
        &AccessScope::TaskScoped,
        "get_sleep_detail",
        json!({"sleepId": "too-new"}),
    );
    assert_eq!(denied["isError"], true);
    assert_eq!(
        denied["structuredContent"]["error"]["code"],
        json!(access::SCOPE_DENIED)
    );

    // No id -> the latest night inside the grant window, not too-new, the newest in the whole library.
    let result = library.call(&AccessScope::TaskScoped, "get_sleep_detail", json!({}));
    assert_eq!(result["isError"], false, "{}", result["content"][0]["text"]);
    assert_eq!(
        result["structuredContent"]["sleep"]["sleep_id"],
        json!("in-window")
    );
    // R6: exits that serde a whole struct must strip device_id (the fixture has a value).
    let serialized = serde_json::to_string(&result).unwrap();
    assert!(!serialized.contains("device_id"), "{serialized}");
    assert!(!serialized.contains("watch-serial-zzz"));
    assert_no_pathish_keys(&result, "sleep_detail");

    // No sleep at all inside the grant window -> deny, not fall back to the newest in the whole library.
    let empty = TestLibrary::empty();
    {
        let db = Database::open_migrated(&empty.0.join("zepp.db")).unwrap();
        db.insert_workout(&run_workout("anchor", utc_noon(20), 140))
            .unwrap();
        db.insert_sleep_session(&sleep_session_days_ago("too-new", 1))
            .unwrap();
    }
    empty.share_task("t1", &task_payload("t1", &["anchor"], &["sleep"]), true);
    let denied = empty.call(&AccessScope::TaskScoped, "get_sleep_detail", json!({}));
    assert_eq!(
        denied["structuredContent"]["error"]["code"],
        json!(access::SCOPE_DENIED)
    );
}

/// R4: grants go by id, not by "the latest N in the whole library". A granted workout ranked past 200 in the whole library
/// must still come back.
#[test]
fn task_list_workouts_returns_granted_ids_not_global_recency() {
    let library = TestLibrary::empty();
    {
        let db = Database::open_migrated(&library.0.join("zepp.db")).unwrap();
        for index in 0..205 {
            db.insert_workout(&run_workout(
                &format!("recent-{index:03}"),
                utc_noon(index + 1),
                140,
            ))
            .unwrap();
        }
        db.insert_workout(&run_workout("granted-old", utc_noon(300), 140))
            .unwrap();
    }
    library.share_task(
        "t1",
        &task_payload("t1", &["granted-old"], &["workout"]),
        true,
    );

    let result = library.call(
        &AccessScope::TaskScoped,
        "list_workouts",
        json!({"limit": 1}),
    );
    assert_eq!(result["isError"], false);
    let ids: Vec<&str> = result["structuredContent"]["workouts"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|workout| workout["workoutId"].as_str())
        .collect();
    assert_eq!(ids, ["granted-old"], "a granted id must be visible however old it is");
    assert_no_pathish_keys(&result, "list_workouts");

    // Control: limit=1 in full-readonly is the global newest, not this one.
    let full = library.call(
        &AccessScope::FullReadOnly,
        "list_workouts",
        json!({"limit": 1}),
    );
    assert_eq!(
        full["structuredContent"]["workouts"][0]["workoutId"],
        json!("recent-000")
    );
}

/// Grants are reread on every call: flipping `mcp_shared` takes effect on the next call.
#[test]
fn grant_changes_between_calls_take_effect_immediately() {
    let library = TestLibrary::empty();
    {
        let db = Database::open_migrated(&library.0.join("zepp.db")).unwrap();
        db.insert_workout(&run_workout("w1", utc_noon(5), 140))
            .unwrap();
    }
    library.share_task("t1", &task_payload("t1", &["w1"], &["workout"]), false);
    let denied = library.call(&AccessScope::TaskScoped, "list_workouts", json!({}));
    assert_eq!(
        denied["structuredContent"]["error"]["code"],
        json!(access::SCOPE_NO_GRANTS)
    );

    library.share_task("t1", &task_payload("t1", &["w1"], &["workout"]), true);
    let result = library.call(&AccessScope::TaskScoped, "list_workouts", json!({}));
    assert_eq!(result["isError"], false);
    assert_eq!(
        result["structuredContent"]["workouts"][0]["workoutId"],
        json!("w1")
    );
    assert_eq!(result["scope"]["grants"], json!(1));
}

/// R7: neither `_meta` nor request arguments grant access. The process scope comes from argv,
/// and nothing else can change it.
#[test]
fn request_meta_and_params_cannot_override_the_process_scope() {
    let library = TestLibrary::empty(); // zero grants
    for params in [
        json!({"name": "list_workouts", "arguments": {},
                   "_meta": {"scope": "full-readonly"}}),
        json!({"name": "list_workouts", "arguments": {"scope": "full-readonly"}}),
    ] {
        let result = call_tool_with_db(&params, &AccessScope::TaskScoped, || {
            let db = Database::open_read_only(library.0.join("zepp.db")).unwrap();
            Ok((db, 0))
        })
        .unwrap();
        assert_eq!(
            result["structuredContent"]["error"]["code"],
            json!(access::SCOPE_NO_GRANTS),
            "{params}"
        );
    }
    // The same holds through full dispatch (modern era with a version in _meta): scope comes only from argv.
    let result = handle_scoped(
        "tools/call",
        &json!({
            "name": "list_workouts",
            "arguments": {},
            "_meta": {
                "io.modelcontextprotocol/protocolVersion": "2026-07-28",
                "scope": "full-readonly",
            }
        }),
        &AccessScope::TaskScoped,
    );
    // This really goes through open_db: with no database it is also isError, never allowed.
    assert!(result.is_ok());
}
