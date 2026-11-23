//! Wrap-up tests of A7 protocol behavior: migration, CRUD, read-only built-in templates, P4 windows, dedup,
//! attachment privacy, prepare writes and blocked, MCP grant windows.

use super::export::stat_task_attachments;
use super::model::*;
use super::store::normalize_task;
use crate::access::{granted_windows, shared_task_grants, AccessCategory};
use crate::models::error::ZeppBridgeError;
use crate::models::{DailyMetric, MetricSample, SleepSession, SourceScope, Workout};
use crate::storage::{Database, CURRENT_SCHEMA_VERSION};
use chrono::{DateTime, Duration, Local, NaiveDate, TimeZone, Utc};

// ---------- Small helpers ----------

fn err_code(error: &ZeppBridgeError) -> &str {
    error.code()
}

fn utc(y: i32, m: u32, d: u32, h: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(y, m, d, h, 0, 0).unwrap()
}

/// Local start day of a workout, the P4 anchor day. Tests and code use the same localtime conversion,
/// so this holds in any time zone.
fn local_day(t: DateTime<Utc>) -> NaiveDate {
    t.with_timezone(&Local).date_naive()
}

fn insert_workout(db: &Database, id: &str, start: DateTime<Utc>) {
    db.insert_workout(&Workout {
        workout_id: id.to_string(),
        workout_type: "running".into(),
        normalized_type: "running".into(),
        type_source: "string_field".into(),
        effective_type: "running".into(),
        start_time: start,
        end_time: start + Duration::hours(1),
        distance_meters: Some(10_000.0),
        calories: Some(600),
        avg_hr: Some(140),
        max_hr: Some(170),
        source_scope: SourceScope::UserFused,
        ..Default::default()
    })
    .unwrap();
}

fn insert_sleep(db: &Database, id: &str, end: DateTime<Utc>) {
    db.insert_sleep_session(&SleepSession {
        sleep_id: id.to_string(),
        start_time: end - Duration::hours(7),
        end_time: end,
        score: Some(82),
        duration_minutes: 400,
        deep_minutes: Some(90),
        light_minutes: Some(220),
        rem_minutes: Some(70),
        awake_minutes: Some(20),
        source_scope: SourceScope::Device,
        device_id: None,
        synced_at: None,
        time_in_bed_minutes: None,
        stages: Vec::new(),
        wake_count: Some(2),
    })
    .unwrap();
}

fn insert_daily(db: &Database, metric: &str, date: NaiveDate, value: f64) {
    db.insert_daily_metric(&DailyMetric {
        date: date.to_string(),
        metric: metric.to_string(),
        value,
        unit: "x".into(),
        source_scope: SourceScope::UserFused,
        device_id: None,
    })
    .unwrap();
}

fn insert_sample(db: &Database, metric: &str, ts: DateTime<Utc>, value: f64) {
    db.insert_metric_sample(&MetricSample {
        metric: metric.to_string(),
        timestamp: ts,
        value,
        unit: "x".into(),
        source_scope: SourceScope::Device,
        device_id: None,
    })
    .unwrap();
}

fn range(category: AiTaskCategory, days_before: i64, include_day: bool) -> AiTaskCategoryRange {
    AiTaskCategoryRange {
        category,
        enabled: true,
        days_before,
        include_workout_day: include_day,
        excluded_metrics: Vec::new(),
        picked_days: Vec::new(),
    }
}

fn task() -> AiTask {
    AiTask {
        schema_version: AI_TASK_SCHEMA_VERSION,
        id: String::new(),
        title: "Test task".into(),
        template_id: None,
        workout_ids: Vec::new(),
        categories: Vec::new(),
        detail_level: AiTaskDetailLevel::Standard,
        prompt: "Check my recent state".into(),
        personal_note: String::new(),
        attachments: Vec::new(),
        include_precise_gps: false,
        mcp_shared: false,
        created_at: String::new(),
        updated_at: String::new(),
    }
}

fn template(id: &str, builtin: bool) -> AiTaskTemplate {
    AiTaskTemplate {
        schema_version: AI_TASK_SCHEMA_VERSION,
        id: id.to_string(),
        builtin,
        name: "Template".into(),
        name_code: None,
        categories: Vec::new(),
        detail_level: AiTaskDetailLevel::Standard,
        prompt_template: String::new(),
        prompt_code: None,
        default_summaries: Vec::new(),
        required_series: Vec::new(),
        created_at: String::new(),
        updated_at: String::new(),
    }
}

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("zb-ai-task-test-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

// ---------- v32 migration ----------

#[test]
fn v32_migration_creates_tables_and_seeds_three_builtins() {
    let db = Database::in_memory().unwrap();
    let version: i64 = db
        .conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, CURRENT_SCHEMA_VERSION);
    db.conn
        .query_row("SELECT COUNT(*) FROM ai_tasks", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap();

    let templates = db.list_ai_task_templates().unwrap();
    assert_eq!(templates.len(), 7);
    let ids: Vec<&str> = templates.iter().map(|t| t.id.as_str()).collect();
    for id in [
        "recovery_run",
        "long_run_compare",
        "hr_drift",
        "sleep_review",
        "recovery_trend",
        "week_review",
        "next_week",
    ] {
        assert!(ids.contains(&id), "missing built-in template {id}");
    }
    for template in &templates {
        assert!(template.builtin);
        assert_eq!(template.schema_version, 1);
        assert_eq!(
            template.name_code.as_deref(),
            Some(format!("ui.ai_template.{}.name", template.id).as_str())
        );
        assert_eq!(
            template.prompt_code.as_deref(),
            Some(format!("ui.ai_template.{}.prompt", template.id).as_str())
        );
        // A built-in payload always lists all nine categories (resting HR is its own category since 1A A10). Resting HR follows the "Heart rate" item.
        assert_eq!(template.categories.len(), 9);
        let of = |c: AiTaskCategory| {
            template
                .categories
                .iter()
                .find(|r| r.category == c)
                .unwrap()
        };
        assert_eq!(
            of(AiTaskCategory::RestingHr).enabled,
            of(AiTaskCategory::HeartRate).enabled
        );
        assert_eq!(
            of(AiTaskCategory::RestingHr).days_before,
            of(AiTaskCategory::HeartRate).days_before
        );
    }
}

#[test]
fn v32_upgrade_from_v31_seeds_builtins_and_stays_idempotent() {
    // First build a full v32 database, then roll back to v31 and drop the new tables. That is the shape of a
    // "real v31 old database": all old tables exist, only ai_tasks / ai_task_templates are missing.
    let dir = temp_dir("v31");
    let path = dir.join("zepp.db");
    drop(Database::open_migrated(&path).unwrap());
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(
            "DROP TABLE ai_tasks;
             DROP TABLE ai_task_templates;
             PRAGMA user_version = 31;",
        )
        .unwrap();
    }
    let db = Database::open_migrated(&path).unwrap();
    let version: i64 = db
        .conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, CURRENT_SCHEMA_VERSION);
    assert_eq!(db.list_ai_task_templates().unwrap().len(), 7);
    drop(db);

    // A read-only connection accepts the current schema version.
    let ro = Database::open_read_only(path.clone()).unwrap();
    assert_eq!(ro.list_ai_task_templates().unwrap().len(), 7);
    drop(ro);

    // Delete one built-in row and migrate again. INSERT OR IGNORE restores it.
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute(
            "DELETE FROM ai_task_templates WHERE id = 'recovery_run'",
            [],
        )
        .unwrap();
    }
    let db = Database::open_migrated(&path).unwrap();
    assert_eq!(db.list_ai_task_templates().unwrap().len(), 7);
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------- CRUD ----------

#[test]
fn task_crud_roundtrip() {
    let db = Database::in_memory().unwrap();
    let mut t = task();
    t.schema_version = 99; // Even if the frontend sends junk, pin it back to 1
    t.workout_ids = vec!["w1".into(), "w1".into(), " w2 ".into(), "w1".into()];
    t.template_id = Some("  ".into());

    let saved = db.save_ai_task(&t).unwrap();
    assert!(saved.id.starts_with("task-"));
    assert_eq!(saved.schema_version, 1);
    assert!(!saved.created_at.is_empty() && !saved.updated_at.is_empty());
    assert_eq!(saved.workout_ids, vec!["w1".to_string(), "w2".to_string()]);
    assert_eq!(saved.template_id, None);

    let fetched = db.require_ai_task(&saved.id).unwrap();
    assert_eq!(fetched.title, "Test task");

    let list = db.list_ai_tasks().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].workout_count, 2);
    assert!(!list[0].mcp_shared);

    // Update: created_at stays, and the mcp_shared flip goes through the same save.
    let mut again = fetched.clone();
    again.mcp_shared = true;
    again.title = "Renamed".into();
    let saved2 = db.save_ai_task(&again).unwrap();
    assert_eq!(saved2.created_at, fetched.created_at);
    assert!(db.list_ai_tasks().unwrap()[0].mcp_shared);

    db.delete_ai_task(&saved.id).unwrap();
    let error = db.delete_ai_task(&saved.id).unwrap_err();
    assert_eq!(err_code(&error), "err.ai_task.not_found");
    let error = db.require_ai_task(&saved.id).unwrap_err();
    assert_eq!(err_code(&error), "err.ai_task.not_found");
}

#[test]
fn task_validation_rejects_bad_input() {
    let db = Database::in_memory().unwrap();

    let mut t = task();
    t.title = " ".into();
    assert_eq!(
        err_code(&db.save_ai_task(&t).unwrap_err()),
        "err.ai_task.invalid"
    );

    let mut t = task();
    // "All" = ten years (3.0.4). Anything older is refused.
    t.categories = vec![range(AiTaskCategory::Sleep, 3651, true)];
    assert_eq!(
        err_code(&db.save_ai_task(&t).unwrap_err()),
        "err.ai_task.invalid"
    );

    // A repeated category keeps only the first.
    let mut t = task();
    t.categories = vec![
        range(AiTaskCategory::Sleep, 7, true),
        range(AiTaskCategory::Sleep, 99, false),
        range(AiTaskCategory::Workout, 3, true),
    ];
    let normalized = normalize_task(&t).unwrap();
    assert_eq!(normalized.categories.len(), 2);
    assert_eq!(normalized.categories[0].days_before, 7);
    assert!(normalized.categories[0].enabled);

    // An attachment with missing fields is refused.
    let mut t = task();
    t.attachments = vec![AiTaskAttachmentRef {
        id: "a1".into(),
        path: " ".into(),
        display_name: "Medical report".into(),
        kind: AiTaskAttachmentKind::Pdf,
        byte_len: None,
        added_at: "2026-09-01T00:00:00Z".into(),
    }];
    assert_eq!(
        err_code(&db.save_ai_task(&t).unwrap_err()),
        "err.ai_task.invalid"
    );
}

#[test]
fn builtin_templates_are_readonly() {
    let db = Database::in_memory().unwrap();

    // Edit a built-in template -> builtin_readonly
    let mut copy = db.get_ai_task_template("recovery_run").unwrap().unwrap();
    copy.name = "Renamed".into();
    assert_eq!(
        err_code(&db.save_ai_task_template(&copy).unwrap_err()),
        "err.ai_template.builtin_readonly"
    );
    assert_eq!(
        err_code(&db.delete_ai_task_template("recovery_run").unwrap_err()),
        "err.ai_template.builtin_readonly"
    );
    // A new id cannot carry builtin=true either
    assert_eq!(
        err_code(
            &db.save_ai_task_template(&template("tmpl-x", true))
                .unwrap_err()
        ),
        "err.ai_template.invalid"
    );

    // "Save as user template" is the only valid path.
    let mut mine = template("", false);
    mine.name_code = Some("ui.ai_template.recovery_run.name".into());
    let saved = db.save_ai_task_template(&mine).unwrap();
    assert!(saved.id.starts_with("tmpl-"));
    assert!(!saved.builtin);
    assert_eq!(saved.name_code, None, "user templates must not carry built-in copy handles");

    // User templates can be edited and deleted. Deleting a missing one -> not_found.
    let mut rename = db.get_ai_task_template(&saved.id).unwrap().unwrap();
    rename.name = "My template".into();
    db.save_ai_task_template(&rename).unwrap();
    db.delete_ai_task_template(&saved.id).unwrap();
    assert_eq!(
        err_code(&db.delete_ai_task_template(&saved.id).unwrap_err()),
        "err.ai_template.not_found"
    );
}

// ---------- P4 coverage ----------

#[test]
fn coverage_windows_follow_p4_edges() {
    let db = Database::in_memory().unwrap();
    let start = utc(2026, 9, 10, 10);
    insert_workout(&db, "w1", start);
    let anchor_day = local_day(start);
    insert_daily(&db, "resting_hr", anchor_day - Duration::days(1), 52.0);
    insert_daily(&db, "resting_hr", anchor_day, 51.0);

    let mut t = task();
    t.workout_ids = vec!["w1".into()];
    t.categories = vec![range(AiTaskCategory::Recovery, 3, true)];
    let preview = db.ai_task_preview(&t).unwrap();
    let row = preview
        .coverage
        .iter()
        .find(|r| r.category == AiTaskCategory::Recovery)
        .unwrap();
    assert_eq!(row.start_date, (anchor_day - Duration::days(3)).to_string());
    assert_eq!(row.end_date, anchor_day.to_string());
    assert_eq!(row.days_in_range, 4);
    assert_eq!(row.days_with_data, 2);
    // The UI draws "which days have data" from it: the count must match days_with_data, and dates fall in the window, ascending.
    assert_eq!(row.covered_dates.len() as i64, row.days_with_data);
    assert!(row.covered_dates.iter().all(
        |day| day.as_str() >= row.start_date.as_str() && day.as_str() <= row.end_date.as_str()
    ));
    assert!(row.covered_dates.windows(2).all(|pair| pair[0] < pair[1]));
    assert!(!row.missing);
    assert!(row.sources.contains(&"user_fused".to_string()));
    assert!(row.units.contains_key("resting_hr"));

    // include_workout_day=false -> the right end is the day before.
    let mut t = task();
    t.workout_ids = vec!["w1".into()];
    t.categories = vec![range(AiTaskCategory::Recovery, 3, false)];
    let preview = db.ai_task_preview(&t).unwrap();
    let row = &preview.coverage[0];
    assert_eq!(row.end_date, (anchor_day - Duration::days(1)).to_string());
    assert_eq!(row.days_in_range, 3);
    assert_eq!(row.days_with_data, 1);

    // days_before=0 without the day itself -> empty window, no row.
    let mut t = task();
    t.workout_ids = vec!["w1".into()];
    t.categories = vec![range(AiTaskCategory::Recovery, 0, false)];
    let preview = db.ai_task_preview(&t).unwrap();
    assert!(preview.coverage.is_empty());
}

#[test]
fn coverage_dedups_overlapping_windows_and_links_anchors() {
    let db = Database::in_memory().unwrap();
    let s1 = utc(2026, 9, 10, 10);
    let s2 = utc(2026, 9, 12, 10);
    insert_workout(&db, "w1", s1);
    insert_workout(&db, "w2", s2);
    let d1 = local_day(s1);
    let d2 = local_day(s2);
    // Data on the overlap day (inside both windows).
    let overlap_day = d1.max(d2 - Duration::days(3));
    insert_daily(&db, "resting_hr", overlap_day, 55.0);

    let mut t = task();
    t.workout_ids = vec!["w1".into(), "w2".into()];
    // With the "Workout" category on, the anchor ids appear in the day rows' linked_workout_ids.
    t.categories = vec![
        range(AiTaskCategory::Recovery, 3, true),
        range(AiTaskCategory::Workout, 0, true),
    ];

    let preview = db.ai_task_preview(&t).unwrap();
    let rows: Vec<_> = preview
        .coverage
        .iter()
        .filter(|r| r.category == AiTaskCategory::Recovery)
        .collect();
    // One row per anchor + one merged row.
    assert_eq!(rows.len(), 3);
    let merged = rows.iter().find(|r| r.workout_id.is_none()).unwrap();
    assert_eq!(merged.start_date, (d1 - Duration::days(3)).to_string());
    assert_eq!(merged.end_date, d2.to_string());
    assert!(merged.days_in_range >= 4);
    assert!(merged.days_with_data >= 1);
    assert_eq!(
        merged.covered_dates.len() as i64,
        merged.days_with_data,
        "merged row dates are the union of the windows, with the same count after dedup"
    );

    // In the export document, the overlap day's (category,date) appears once, linked to both anchors.
    let dir = temp_dir("dedup");
    let prepared = db.ai_task_prepare(&t, "", &dir).unwrap();
    assert_eq!(prepared.status, AiTaskPrepareStatus::Ready);
    let json_text = std::fs::read_to_string(prepared.json_path.unwrap()).unwrap();
    let doc: serde_json::Value = serde_json::from_str(&json_text).unwrap();
    let context = doc["context"].as_array().unwrap();
    let recovery = context
        .iter()
        .find(|c| c["category"] == "recovery")
        .unwrap();
    let days = recovery["days"].as_array().unwrap();
    let overlap_entries: Vec<_> = days
        .iter()
        .filter(|d| d["date"] == overlap_day.to_string())
        .collect();
    assert_eq!(overlap_entries.len(), 1, "(category,date) must be deduped");
    let linked: Vec<&str> = overlap_entries[0]["linked_workout_ids"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert!(linked.contains(&"w1") && linked.contains(&"w2"));
    // The same metric on the same day is written once.
    let resting: Vec<_> = overlap_entries[0]["metrics"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|m| m["metric"] == "resting_hr")
        .collect();
    assert_eq!(resting.len(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn sleep_coverage_attributes_by_wake_day() {
    let db = Database::in_memory().unwrap();
    let start = utc(2026, 9, 10, 10);
    insert_workout(&db, "w1", start);
    let anchor_day = local_day(start);
    // Sleep belongs to end_time (the wake-up day). This rule must not change.
    let sleep_end = utc(2026, 9, 10, 22);
    insert_sleep(&db, "s1", sleep_end);
    let expected_day = local_day(sleep_end);

    let mut t = task();
    t.workout_ids = vec!["w1".into()];
    t.categories = vec![range(AiTaskCategory::Sleep, 7, true)];
    let preview = db.ai_task_preview(&t).unwrap();
    let row = preview
        .coverage
        .iter()
        .find(|r| r.category == AiTaskCategory::Sleep)
        .unwrap();
    let window_start = anchor_day - Duration::days(7);
    let in_window = expected_day >= window_start && expected_day <= anchor_day;
    assert_eq!(row.days_with_data, i64::from(in_window));
    assert_eq!(row.missing, !in_window);
}

#[test]
fn missing_workout_is_a_coded_error() {
    let db = Database::in_memory().unwrap();
    let mut t = task();
    t.workout_ids = vec!["ghost-1".into(), "ghost-2".into()];
    let error = db.ai_task_preview(&t).unwrap_err();
    assert_eq!(err_code(&error), "err.ai_task.workout_not_found");
    if let ZeppBridgeError::AiTask(inner) = &error {
        let params = inner.params().unwrap();
        assert_eq!(params["workout_ids"].as_array().unwrap().len(), 2);
    } else {
        panic!("expected an AiTask error");
    }
}

// ---------- Attachments / prepare ----------

#[test]
fn shared_grants_expand_only_shared_tasks() {
    let db = Database::in_memory().unwrap();
    insert_workout(&db, "w1", utc(2026, 9, 10, 10));

    let mut t = task();
    t.workout_ids = vec!["w1".into()];
    t.categories = vec![
        range(AiTaskCategory::Recovery, 3, true),
        range(AiTaskCategory::PersonalNote, 0, true),
    ];
    t.mcp_shared = true;
    let shared = db.save_ai_task(&t).unwrap();

    // A task that is not shared does not enter the grant surface.
    let mut t2 = task();
    t2.title = "Not shared".into();
    t2.workout_ids = vec!["w1".into()];
    db.save_ai_task(&t2).unwrap();

    let grants = shared_task_grants(&db).unwrap();
    assert_eq!(grants.len(), 1);
    let grant = &grants[0];
    assert_eq!(grant.task_id, shared.id);
    // The workout category is off: w1 is only a window anchor, not a readable entity (R01).
    assert_eq!(grant.anchor_workout_ids, vec!["w1".to_string()]);
    assert!(grant.workout_ids.is_empty());
    let anchor_day = local_day(utc(2026, 9, 10, 10));
    // Recovery opens a window. PersonalNote is task content, not a time series, so it gives no grant window.
    assert_eq!(grant.windows.len(), 1);
    assert_eq!(grant.windows[0].category, AccessCategory::Recovery);
    assert_eq!(grant.windows[0].start_date, anchor_day - Duration::days(3));
    assert_eq!(grant.windows[0].end_date, anchor_day);

    // After sharing is turned off, the next read has no grant. Skipping the reread is not an option.
    let mut off = shared.clone();
    off.mcp_shared = false;
    db.save_ai_task(&off).unwrap();
    assert!(shared_task_grants(&db).unwrap().is_empty());
}

/// Tasks saved before the split (1A A10) have only a `heart_rate` row. On read, add a `resting_hr` row with the same settings,
/// and the dragged-out resting HR follows. MCP grant windows cover both categories, so old tasks share the same data as before the split, with resting HR kept.
#[test]
fn legacy_heart_rate_task_reads_back_split_and_still_grants_resting_hr() {
    let db = Database::in_memory().unwrap();
    insert_workout(&db, "w1", utc(2026, 9, 10, 10));
    let mut t = task();
    t.workout_ids = vec!["w1".into()];
    t.mcp_shared = true;
    let saved = db.save_ai_task(&t).unwrap();
    let mut legacy = range(AiTaskCategory::HeartRate, 5, true);
    legacy.excluded_metrics = vec!["resting_hr".into(), "heart_rate".into()];
    let mut doc = serde_json::to_value(&saved).unwrap();
    doc["categories"] = serde_json::to_value(vec![legacy]).unwrap();
    db.conn
        .execute(
            "UPDATE ai_tasks SET payload = ?2 WHERE id = ?1",
            rusqlite::params![saved.id, doc.to_string()],
        )
        .unwrap();

    let read = db.require_ai_task(&saved.id).unwrap();
    let cats: Vec<_> = read
        .categories
        .iter()
        .map(|r| {
            (
                r.category,
                r.enabled,
                r.days_before,
                r.excluded_metrics.clone(),
            )
        })
        .collect();
    assert_eq!(
        cats,
        vec![
            (
                AiTaskCategory::RestingHr,
                true,
                5,
                vec!["resting_hr".to_string()]
            ),
            (
                AiTaskCategory::HeartRate,
                true,
                5,
                vec!["heart_rate".to_string()]
            ),
        ]
    );

    let grants = shared_task_grants(&db).unwrap();
    let mut granted: Vec<_> = grants[0].windows.iter().map(|w| w.category).collect();
    granted.sort();
    assert_eq!(
        granted,
        vec![AccessCategory::HeartRate, AccessCategory::RestingHr]
    );
}

/// Batch delete (1D D7): one transaction deletes all. It returns the originals and pin state; storing them back is the undo. Missing ids are skipped without error.
#[test]
fn batch_delete_returns_what_undo_needs_and_skips_missing_ids() {
    let db = Database::in_memory().unwrap();
    let mut a = task();
    a.title = "Alpha".into();
    let a = db.save_ai_task(&a).unwrap();
    let mut b = task();
    b.title = "Beta".into();
    b.prompt = "Another question".into();
    let b = db.save_ai_task(&b).unwrap();
    db.set_ai_task_pinned(&b.id, true).unwrap();

    let gone = db
        .delete_ai_tasks(&[
            a.id.clone(),
            "task-missing".into(),
            b.id.clone(),
            a.id.clone(),
        ])
        .unwrap();
    assert_eq!(
        gone.iter()
            .map(|d| (d.task.title.as_str(), d.pinned))
            .collect::<Vec<_>>(),
        vec![("Alpha", false), ("Beta", true)]
    );
    assert!(db.list_ai_tasks().unwrap().is_empty());

    for item in &gone {
        let back = db.save_ai_task(&item.task).unwrap();
        assert_eq!(back.id, item.task.id);
        if item.pinned {
            db.set_ai_task_pinned(&back.id, true).unwrap();
        }
    }
    let list = db.list_ai_tasks().unwrap();
    assert_eq!(list.len(), 2);
    assert!(list.iter().any(|t| t.id == b.id && t.pinned));
}

#[test]
fn shared_grants_merge_adjacent_windows_across_anchors() {
    let db = Database::in_memory().unwrap();
    // Two anchors 2 days apart with days_before=3 -> the windows touch and must merge into one.
    let s1 = utc(2026, 9, 10, 10);
    let s2 = utc(2026, 9, 12, 10);
    insert_workout(&db, "w1", s1);
    insert_workout(&db, "w2", s2);

    let mut t = task();
    t.workout_ids = vec!["w1".into(), "w2".into()];
    t.categories = vec![range(AiTaskCategory::Sleep, 3, true)];
    t.mcp_shared = true;
    db.save_ai_task(&t).unwrap();

    let grants = shared_task_grants(&db).unwrap();
    assert_eq!(grants.len(), 1);
    assert_eq!(grants[0].windows.len(), 2, "loading opens one window per anchor, with no early merge");
    // The merge happens on the check side in granted_windows: two touching Sleep windows merge into one.
    let merged = granted_windows(&grants, &[AccessCategory::Sleep], None);
    assert_eq!(merged.len(), 1, "touching grant day windows must merge into one");
    assert_eq!(merged[0].start_date, local_day(s1) - Duration::days(3));
    assert_eq!(merged[0].end_date, local_day(s2));

    // References a deleted workout: a broad grant must not fail because of it. The ghost id stays in
    // the raw anchor list; it just gets no window (w1's window expands as usual).
    let mut t = task();
    t.workout_ids = vec!["w1".into(), "ghost".into()];
    t.categories = vec![range(AiTaskCategory::Sleep, 1, true)];
    t.mcp_shared = true;
    db.save_ai_task(&t).unwrap();
    let grants = shared_task_grants(&db).unwrap();
    let grant = grants
        .iter()
        .find(|g| g.anchor_workout_ids.iter().any(|id| id == "ghost"))
        .unwrap();
    assert!(
        !grant.windows.is_empty(),
        "a workout that is not found is only skipped and does not break the whole grant"
    );
}

mod prepare;

#[test]
fn the_same_question_again_updates_the_saved_task_and_pins_survive() {
    let db = Database::in_memory().unwrap();
    let mut first = task();
    first.categories = vec![range(AiTaskCategory::Sleep, 13, true)];
    let saved = db.save_ai_task(&first).unwrap();
    // Same template, same time range, same question (ignoring leading/trailing spaces): update the original, do not create a new one.
    let mut again = task();
    again.categories = vec![range(AiTaskCategory::Sleep, 13, true)];
    again.prompt = "  Check my recent state ".into();
    again.title = "Asked again".into();
    assert_eq!(db.save_ai_task(&again).unwrap().id, saved.id);
    assert_eq!(db.list_ai_tasks().unwrap().len(), 1);
    // A different lookback range is a different task.
    let mut other = again.clone();
    other.categories = vec![range(AiTaskCategory::Sleep, 29, true)];
    let other = db.save_ai_task(&other).unwrap();
    assert_ne!(other.id, saved.id);
    // Pinned tasks come first. Pinning does not change updated_at.
    let before = db.require_ai_task(&saved.id).unwrap().updated_at;
    db.set_ai_task_pinned(&saved.id, true).unwrap();
    let list = db.list_ai_tasks().unwrap();
    assert_eq!(list[0].id, saved.id);
    assert!(list[0].pinned && !list[1].pinned);
    assert_eq!(db.require_ai_task(&saved.id).unwrap().updated_at, before);
    assert_eq!(
        err_code(&db.set_ai_task_pinned("task-missing", true).unwrap_err()),
        "err.ai_task.not_found"
    );
}

#[test]
fn picked_days_export_only_those_days_and_the_collection_survives_a_reopen() {
    let db = Database::in_memory().unwrap();
    let today = Local::now().date_naive();
    for offset in 0..5 {
        insert_daily(
            &db,
            "resting_hr",
            today - Duration::days(offset),
            50.0 + offset as f64,
        );
    }
    let mut whole = task();
    whole.categories = vec![range(AiTaskCategory::Recovery, 4, true)];
    let whole = db.ai_task_preview(&whole).unwrap();

    // The inbox picked three days (out of order, with a repeat on purpose): only these three go out. Unpicked days in between are not in range and not exported.
    let picked = [today, today - Duration::days(4), today - Duration::days(2)];
    let mut t = task();
    let mut picked_range = range(AiTaskCategory::Recovery, 4, true);
    picked_range.picked_days = picked
        .iter()
        .chain(picked.iter().take(1))
        .map(|day| day.to_string())
        .collect();
    t.categories = vec![picked_range];
    let preview = db.ai_task_preview(&t).unwrap();
    let row = &preview.coverage[0];
    let mut expected: Vec<String> = picked.iter().map(|day| day.to_string()).collect();
    expected.sort();
    expected.dedup();
    assert_eq!(row.start_date, expected[0]);
    assert_eq!(row.end_date, expected[2]);
    assert_eq!(row.days_in_range, 3);
    assert_eq!(row.days_with_data, 3);
    assert_eq!(row.covered_dates, expected);
    assert_eq!(row.picked_dates, expected);
    assert_eq!(row.metric_days.get("resting_hr"), Some(&3));
    assert!(
        preview.estimated_bytes < whole.estimated_bytes,
        "unpicked days must not go out"
    );
    assert!(whole.coverage[0].picked_dates.is_empty());

    // A saved task: dates sorted and deduped. Different picked days make a different task. A bad date is refused.
    let saved = db.save_ai_task(&t).unwrap();
    assert_eq!(saved.categories[0].picked_days, expected);
    let mut other = t.clone();
    other.categories[0].picked_days = vec![today.to_string()];
    assert_ne!(db.save_ai_task(&other).unwrap().id, saved.id);
    let mut bad = t.clone();
    bad.categories[0].picked_days = vec!["2026-13-40".into()];
    assert!(db.save_ai_task(&bad).is_err());

    // Inbox: deduped and sorted, reads back the same. A bad date refuses the whole set and the old one stays.
    use crate::storage::card_collection::CardPick;
    let pick = |key: &str, date: NaiveDate| CardPick {
        key: key.into(),
        category: AiTaskCategory::Recovery,
        date: date.to_string(),
    };
    let stored = db
        .set_card_collection(&[
            pick("hrv", today),
            pick("hrv", today),
            pick("cat:sleep", today),
        ])
        .unwrap();
    assert_eq!(stored.len(), 2);
    assert_eq!(db.card_collection().unwrap(), stored);
    let mut broken = pick("hrv", today);
    broken.date = "yesterday".into();
    assert!(db.set_card_collection(&[broken]).is_err());
    assert_eq!(db.card_collection().unwrap(), stored);
}
