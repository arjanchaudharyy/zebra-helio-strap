use serde_json::{json, Value};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use chrono::{Duration, TimeZone, Utc};
use zeppbridge_core::models::{MetricSample, SleepSession, SourceScope, Workout};
use zeppbridge_core::storage::Database;

#[test]
fn stdio_recovers_from_bad_input_and_returns_tool_errors_in_result() {
    let missing_dir = std::env::temp_dir().join(format!(
        "zeppbridge-mcp-stdio-{}-{}",
        std::process::id(),
        zeppbridge_core::unique_suffix()
    ));
    let mut child = Command::new(env!("CARGO_BIN_EXE_zeppbridge-mcp"))
        .env("ZEPPBRIDGE_DATA_DIR", &missing_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(b"\xff\n").unwrap();
    writeln!(
        stdin,
        "{}",
        json!({"jsonrpc":"2.0","id":1,"method":"ping","padding":"x".repeat(1024*1024)})
    )
    .unwrap();
    for request in [
        json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"list_workouts"}}),
        json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"list_workouts","_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28"}}}),
        json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"unknown"}}),
        json!({"jsonrpc":"2.0","id":5,"method":"ping"}),
    ] {
        writeln!(stdin, "{request}").unwrap();
    }
    drop(stdin);
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let responses: Vec<Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(responses.len(), 6);
    assert_eq!(responses[0]["error"]["code"], -32700);
    assert_eq!(responses[1]["error"]["code"], -32600);
    for response in &responses[2..4] {
        assert!(response.get("error").is_none());
        assert_eq!(response["result"]["isError"], true);
        assert!(!response["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .is_empty());
    }
    assert_eq!(responses[3]["result"]["resultType"], "complete");
    assert_eq!(responses[4]["error"]["code"], -32601);
    assert_eq!(responses[5]["id"], 5);
    assert_eq!(responses[5]["result"], json!({}));
}

/* ------------------------------ --scope ------------------------------ */

/// argv is the only way to grant access: an unknown argument or value makes the process exit at once,
/// with one stderr line, nothing on stdout, and the server loop never starts.
#[test]
fn bad_argv_exits_nonzero_without_serving() {
    for args in [
        vec!["--scope", "bogus"],
        vec!["--scope=bogus"],
        vec!["--bogus"],
        vec!["--scope"],                            // missing value
        vec!["--scope", "task", "--scope", "task"], // repeated
        vec!["positional"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_zeppbridge-mcp"))
            .args(&args)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(!output.status.success(), "{args:?} must exit non-zero");
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert_eq!(stderr.lines().count(), 1, "{args:?} stderr must have exactly one line");
        assert!(output.stdout.is_empty(), "{args:?} stdout must be empty");
    }
}

/// UTC noon: the local day does not shift in any time zone within +/-14h.
fn utc_noon(days_ago: i64) -> chrono::DateTime<Utc> {
    let day = chrono::Local::now().date_naive() - Duration::days(days_ago);
    Utc.from_utc_datetime(&day.and_hms_opt(12, 0, 0).unwrap())
}

fn workout(id: &str, days_ago: i64) -> Workout {
    let start = utc_noon(days_ago);
    Workout {
        workout_id: id.into(),
        workout_type: "run".into(),
        normalized_type: "run".into(),
        type_source: "numeric_mapped".into(),
        user_override: None,
        effective_type: "run".into(),
        custom_label: None,
        start_time: start,
        end_time: start + Duration::minutes(50),
        distance_meters: Some(10_000.0),
        calories: Some(500),
        avg_hr: Some(140),
        max_hr: Some(170),
        training_load: Some(60.0),
        vo2max: None,
        min_hr: None,
        total_steps: None,
        moving_seconds: None,
        elevation_gain_m: None,
        elevation_loss_m: None,
        max_altitude_m: None,
        min_altitude_m: None,
        training_effect: None,
        anaerobic_training_effect: None,
        rpe: None,
        avg_cadence_spm: None,
        max_cadence_spm: None,
        avg_stride_cm: None,
        gear_id: None,
        hr_zones: Vec::new(),
        source_scope: SourceScope::Device,
        device_id: Some("watch-serial-001".into()),
        synced_at: Some(start + Duration::hours(2)),
        gps_available: true,
        sample_count: 0,
        zepp_source: None,
        zepp_type: Some(1),
    }
}

fn sleep_session(id: &str, end_days_ago: i64) -> SleepSession {
    let end = Utc::now() - Duration::days(end_days_ago);
    SleepSession {
        sleep_id: id.into(),
        start_time: end - Duration::hours(8),
        end_time: end,
        score: Some(80),
        duration_minutes: 480,
        deep_minutes: Some(100),
        light_minutes: Some(300),
        rem_minutes: Some(80),
        awake_minutes: Some(0),
        source_scope: SourceScope::Device,
        device_id: Some("watch-serial-zzz".into()),
        synced_at: Some(end + Duration::hours(1)),
        time_in_bed_minutes: None,
        stages: Vec::new(),
        wake_count: Some(1),
    }
}

/// Fixture library: two workouts (granted and private), one night inside the grant window + one outside,
/// HRV=42 on the granted day. Two tasks with `mcp_shared=1`:
/// - t1 anchors `granted`: sleep, workout (excludes avg_hr), recovery (excludes hrv);
/// - t2 anchors `private`: only sleep is on, the workout category is off. An anchor is not the same as readable (R01).
fn scoped_library() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "zeppbridge-mcp-scope-{}-{}",
        std::process::id(),
        zeppbridge_core::unique_suffix()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    {
        let db = Database::open_migrated(&dir.join("zepp.db")).unwrap();
        db.insert_workout(&workout("granted", 20)).unwrap();
        db.insert_workout(&workout("private", 2)).unwrap();
        db.insert_sleep_session(&sleep_session("in-window", 25))
            .unwrap();
        db.insert_sleep_session(&sleep_session("outside", 1))
            .unwrap();
        db.insert_metric_sample_with_raw(
            &MetricSample {
                metric: "hrv".into(),
                timestamp: utc_noon(20),
                value: 42.0,
                unit: "ms".into(),
                source_scope: SourceScope::Device,
                device_id: None,
            },
            None,
        )
        .unwrap();
    }
    // The `ai_tasks` table is built by the core migration. Write only the columns the grant check reads.
    let conn = rusqlite::Connection::open(dir.join("zepp.db")).unwrap();
    let tasks = [
        json!({
            "id": "t1",
            "workout_ids": ["granted"],
            "categories": [
                {"category": "sleep", "enabled": true, "days_before": 14, "include_workout_day": true},
                {"category": "workout", "enabled": true, "days_before": 0, "include_workout_day": true,
                 "excluded_metrics": ["avg_hr"]},
                {"category": "recovery", "enabled": true, "days_before": 14, "include_workout_day": true,
                 "excluded_metrics": ["hrv"]},
            ],
        }),
        json!({
            "id": "t2",
            "workout_ids": ["private"],
            "categories": [
                {"category": "sleep", "enabled": true, "days_before": 0, "include_workout_day": true},
                {"category": "workout", "enabled": false},
            ],
        }),
    ];
    for task in tasks {
        conn.execute(
            "INSERT INTO ai_tasks (id, payload, mcp_shared, created_at, updated_at)
             VALUES (?1, ?2, 1, '', '')",
            [task["id"].as_str().unwrap().to_string(), task.to_string()],
        )
        .unwrap();
    }
    dir
}

fn spawn_mcp(args: &[&str], data_dir: &Path) -> Child {
    Command::new(env!("CARGO_BIN_EXE_zeppbridge-mcp"))
        .args(args)
        .env("ZEPPBRIDGE_DATA_DIR", data_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap()
}

fn transcript(child: &mut Child, requests: &[Value]) -> Vec<Value> {
    let mut stdin = child.stdin.take().unwrap();
    for request in requests {
        writeln!(stdin, "{request}").unwrap();
    }
    drop(stdin);
    Vec::new() // placeholder: the caller reads stdout
}

/// End to end: run initialize -> tools/list -> tools/call once for each scope.
#[test]
fn scope_flag_controls_what_the_stdio_server_exposes() {
    let dir = scoped_library();
    let requests = [
        json!({"jsonrpc":"2.0","id":1,"method":"initialize",
               "params":{"protocolVersion":"2024-11-05","capabilities":{}}}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
        json!({"jsonrpc":"2.0","id":3,"method":"tools/call",
               "params":{"name":"list_workouts","arguments":{}}}),
        json!({"jsonrpc":"2.0","id":4,"method":"tools/call",
               "params":{"name":"get_sleep_detail","arguments":{}}}),
        json!({"jsonrpc":"2.0","id":5,"method":"tools/call",
               "params":{"name":"get_data_health","arguments":{}}}),
        json!({"jsonrpc":"2.0","id":6,"method":"tools/call",
               "params":{"name":"get_metric_series","arguments":{"metrics":["hrv"],"days":60}}}),
        json!({"jsonrpc":"2.0","id":7,"method":"tools/call",
               "params":{"name":"get_workout_insight","arguments":{"workoutId":"private"}}}),
    ];

    // Old-style start with no arguments = full-readonly: both workouts, the latest night and data health are all there.
    {
        let mut child = spawn_mcp(&[], &dir);
        transcript(&mut child, &requests);
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success());
        let responses: Vec<Value> = String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(responses.len(), 7);
        assert_eq!(responses[0]["result"]["scope"]["mode"], "full-readonly");
        assert_eq!(
            responses[1]["result"]["tools"].as_array().unwrap().len(),
            29
        );
        let ids: Vec<&str> = responses[2]["result"]["structuredContent"]["workouts"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|workout| workout["workoutId"].as_str())
            .collect();
        assert_eq!(ids, ["private", "granted"]);
        assert_eq!(responses[2]["result"]["scope"]["mode"], "full-readonly");
        assert_eq!(
            responses[3]["result"]["structuredContent"]["sleep"]["sleep_id"],
            "outside"
        );
        assert_eq!(responses[4]["result"]["isError"], false);
        // Whole-library read-only ignores tasks: HRV is readable as usual.
        let hrv = serde_json::to_string(&responses[5]["result"]["structuredContent"]).unwrap();
        assert!(hrv.contains("42"), "{hrv}");
    }

    // --scope task: only the granted workout and the latest night inside the grant window; data health is denied as a whole.
    {
        let mut child = spawn_mcp(&["--scope", "task"], &dir);
        transcript(&mut child, &requests);
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success());
        let responses: Vec<Value> = String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(responses.len(), 7);
        assert_eq!(responses[0]["result"]["scope"]["mode"], "task");
        assert!(responses[0]["result"]["instructions"]
            .as_str()
            .unwrap()
            .contains("task:"));
        let list = &responses[2]["result"];
        assert_eq!(list["scope"], json!({"mode": "task", "grants": 2}));
        let workouts = list["structuredContent"]["workouts"].as_array().unwrap();
        let ids: Vec<&str> = workouts
            .iter()
            .filter_map(|workout| workout["workoutId"].as_str())
            .collect();
        assert_eq!(
            ids,
            ["granted"],
            "task scope returns only readable workouts; the anchor workout with the workout category off does not appear"
        );
        assert!(workouts[0].get("avgHr").is_none(), "the excluded avg_hr must not go out");
        assert_eq!(workouts[0]["maxHr"], 170);
        let sleep = &responses[3]["result"];
        assert_eq!(
            sleep["structuredContent"]["sleep"]["sleep_id"], "in-window",
            "with no id, only the latest night inside the grant window can come back"
        );
        let serialized = serde_json::to_string(&sleep).unwrap();
        assert!(!serialized.contains("device_id"), "{serialized}");
        let health = &responses[4]["result"];
        assert_eq!(health["isError"], true);
        assert_eq!(
            health["structuredContent"]["error"]["code"],
            "err.mcp.scope_denied"
        );
        assert_eq!(health["scope"]["mode"], "task");
        // R01: the excluded HRV returns no reading. It is denied openly.
        let hrv = &responses[5]["result"];
        assert_eq!(hrv["isError"], true, "{hrv}");
        assert!(!serde_json::to_string(hrv).unwrap().contains("42"));
        // R01: the anchor workout (workout category off) gives no detail.
        let insight = &responses[6]["result"];
        assert_eq!(insight["isError"], true, "{insight}");
        assert_eq!(
            insight["structuredContent"]["error"]["code"],
            "err.mcp.scope_denied"
        );
    }

    let _ = std::fs::remove_dir_all(&dir);
}
