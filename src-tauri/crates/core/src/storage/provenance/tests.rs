use super::health::*;
use super::*;
use crate::models::{CapabilityStatus, MetricSample, RawRecord, SourceScope, Workout};
use chrono::TimeZone;

fn db() -> Database {
    Database::in_memory().unwrap()
}

fn day(offset: i64) -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 8, 1, 12, 0, 0).unwrap() + Duration::days(offset)
}

fn sample(metric: &str, offset: i64, scope: SourceScope) -> MetricSample {
    MetricSample {
        metric: metric.into(),
        timestamp: day(offset),
        value: 60.0 + offset as f64,
        unit: "bpm".into(),
        source_scope: scope,
        device_id: Some("device-a".into()),
    }
}

#[test]
fn the_three_stages_are_recorded_independently() {
    let db = db();
    // The payload came back but could not be read: this must show as "fetch ok / parse failed",
    // and must not collapse into one vague red dot.
    db.record_stream_stage("sleep", Stage::Fetch, &StageOutcome::Ok)
        .unwrap();
    db.record_stream_stage(
        "sleep",
        Stage::Parse,
        &StageOutcome::Failed {
            kind: StageErrorKind::UnrecognizedPayload,
            message: Some("band_data encoding not recognized".into()),
        },
    )
    .unwrap();

    let (states, _) = db.stage_states().unwrap();
    let stages = states.get("sleep").unwrap();
    assert_eq!(stages[0].state, "ok");
    assert_eq!(stages[1].state, "failed");
    assert_eq!(
        stages[1].error_kind.as_deref(),
        Some("unrecognized_payload")
    );
    assert_eq!(stages[2].state, "never", "never written is not a failure");
}

#[test]
fn a_later_success_clears_an_earlier_failure_but_keeps_the_last_good_time() {
    let db = db();
    db.record_stream_stage(
        "hrv",
        Stage::Fetch,
        &StageOutcome::Failed {
            kind: StageErrorKind::Network,
            message: Some("timed out".into()),
        },
    )
    .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(2));
    db.record_stream_stage("hrv", Stage::Fetch, &StageOutcome::Ok)
        .unwrap();

    let (states, _) = db.stage_states().unwrap();
    let fetch = &states.get("hrv").unwrap()[0];
    assert_eq!(fetch.state, "ok");
    assert!(fetch.error_kind.is_none());
    assert!(fetch.last_ok_at.is_some());
}

#[test]
fn cloud_sync_local_replay_and_manual_reprocess_are_separate_timelines() {
    let db = db();
    db.record_cloud_sync("2026-08-20T00:00:00+00:00", "updated", 1)
        .unwrap();
    db.record_local_replay(false).unwrap();

    let health = db.data_health(90, 0).unwrap();
    assert_eq!(
        health.timings.last_cloud_sync_at.as_deref(),
        Some("2026-08-20T00:00:00+00:00"),
        "a local replay must not change the cloud sync time"
    );
    assert!(health.timings.last_local_replay_at.is_some());
    assert!(
        health.timings.last_manual_reprocess_at.is_none(),
        "an automatic background replay is not a manual reparse"
    );

    db.record_local_replay(true).unwrap();
    let health = db.data_health(90, 0).unwrap();
    assert!(health.timings.last_manual_reprocess_at.is_some());
    assert_eq!(
        health.timings.last_cloud_sync_at.as_deref(),
        Some("2026-08-20T00:00:00+00:00")
    );
}

#[test]
fn coverage_is_explained_by_cadence_not_by_one_completeness_percentage() {
    // Continuous stream: we can say "which days are missing".
    let daily = explain_coverage(
        StreamCadence::Daily,
        30,
        Observed {
            days: vec!["2026-08-01".into(), "2026-08-04".into()],
        },
    );
    assert_eq!(daily.kind, "gaps");
    assert_eq!(daily.gap_total, 2);
    assert_eq!(daily.gap_dates, vec!["2026-08-02", "2026-08-03"]);

    // Occasional stream: we can only say "which days had observations". A few VO2max values a year is normal,
    // and one shared completeness measure would always paint it all red.
    let occasional = explain_coverage(
        StreamCadence::Occasional,
        365,
        Observed {
            days: vec!["2026-03-02".into(), "2026-08-01".into()],
        },
    );
    assert_eq!(occasional.kind, "observations");
    assert_eq!(occasional.gap_total, 0);
    assert!(occasional.gap_dates.is_empty());
    assert_eq!(occasional.latest_observed_at.as_deref(), Some("2026-08-01"));

    // Per-event streams are the same: no workout means no workout.
    assert_eq!(
        explain_coverage(StreamCadence::PerEvent, 30, Observed::default()).kind,
        "observations"
    );
}

#[test]
fn gaps_are_only_counted_after_the_first_observed_day() {
    // A new user has only the last three days of data. Counting the blank days before as gaps
    // would show them a wall of red on first open.
    let coverage = explain_coverage(
        StreamCadence::Daily,
        365,
        Observed {
            days: vec![
                "2026-08-01".into(),
                "2026-08-02".into(),
                "2026-08-03".into(),
            ],
        },
    );
    assert_eq!(coverage.gap_total, 0);
    assert_eq!(coverage.observed_days, 3);
}

#[test]
fn unknown_source_scopes_are_never_folded_into_device_data() {
    let db = db();
    db.insert_metric_sample(&sample("heart_rate", 0, SourceScope::Device))
        .unwrap();
    db.insert_metric_sample(&sample("heart_rate", 1, SourceScope::UserFused))
        .unwrap();
    db.insert_metric_sample(&sample("heart_rate", 2, SourceScope::Unknown))
        .unwrap();

    let health = db.data_health(90, 0).unwrap();
    let heart_rate = health
        .streams
        .iter()
        .find(|stream| stream.stream == "heart_rate")
        .unwrap();
    let mut sources: Vec<&str> = heart_rate
        .sources
        .iter()
        .map(|entry| entry.source.as_str())
        .collect();
    sources.sort_unstable();
    assert_eq!(sources, vec!["device", "unknown", "user_fused"]);
    assert!(heart_rate.sources.iter().all(|entry| entry.records == 1));
}

#[test]
fn health_falls_back_to_fetched_at_so_an_upgraded_library_is_not_called_never_fetched() {
    let db = db();
    db.insert_raw_record(&crate::models::RawRecord {
        stream: "workouts".into(),
        source_key: "w-1".into(),
        source_scope: SourceScope::Device,
        device_id: Some("device-a".into()),
        start_utc: day(0),
        end_utc: Some(day(0)),
        payload: serde_json::json!({ "items": [] }),
        capability: crate::models::CapabilityStatus::Verified,
    })
    .unwrap();

    let health = db.data_health(90, 0).unwrap();
    let workouts = health
        .streams
        .iter()
        .find(|stream| stream.stream == "workouts")
        .unwrap();
    assert_eq!(
        workouts.fetch.state, "ok",
        "the library clearly has payloads, so it cannot say they were never fetched"
    );
    assert_eq!(workouts.raw_records, 1);
}

#[test]
fn integrity_check_is_explicit_and_its_result_is_remembered() {
    let db = db();
    assert!(
        db.data_health(90, 0)
            .unwrap()
            .database
            .last_integrity_check
            .is_none(),
        "opening the page must not run a full library scan automatically"
    );
    let result = db.run_integrity_check().unwrap();
    assert!(result.ok);
    assert!(result.detail.is_none());
    let remembered = db
        .data_health(90, 0)
        .unwrap()
        .database
        .last_integrity_check
        .unwrap();
    assert_eq!(remembered.checked_at, result.checked_at);
}

#[test]
fn actions_only_appear_when_there_is_something_to_do() {
    let db = db();
    db.record_stream_stage(
        "heart_rate",
        Stage::Fetch,
        &StageOutcome::Failed {
            kind: StageErrorKind::Auth,
            message: Some("re-authentication required".into()),
        },
    )
    .unwrap();
    let health = db.data_health(90, 0).unwrap();
    // REPLAY_IN_PROGRESS describes the whole process, not this in-memory
    // database. Another parallel test can legitimately hold the replay
    // guard while this assertion runs, so pin that independent input to
    // the idle state this unit test is meant to exercise.
    let mut database = health.database.clone();
    database.replay_in_progress = false;
    let actions = suggested_actions(&database, &health.timings, &health.streams);
    let ids: Vec<&str> = actions.iter().map(|action| action.id.as_str()).collect();
    assert!(ids.contains(&"reauth"));
    assert!(
        !ids.contains(&"reprocess"),
        "with no payloads waiting for normalization, do not suggest a replay"
    );
}
#[test]
fn normalization_health_tracks_attempts_not_canonical_ownership() {
    let db = db();
    let mut raw = RawRecord {
        stream: "daily_summary".into(),
        source_key: "first-page".into(),
        source_scope: SourceScope::UserFused,
        device_id: None,
        start_utc: day(0),
        end_utc: None,
        payload: serde_json::json!({"data": [{"date": "2026-09-14", "steps": 1234}]}),
        capability: CapabilityStatus::Verified,
    };
    db.persist_fetched_record(&raw).unwrap();
    raw.source_key = "overlapping-page".into();
    db.persist_fetched_record(&raw).unwrap();
    let health = db.data_health(14, 0).unwrap();
    assert_eq!(health.database.canonical_records, 1);
    assert_eq!(health.database.pending_normalization, 0);
    assert_eq!(health.database.normalization_by_stream[0].records, 2);

    raw.stream = "wellness".into();
    raw.source_key = "unsupported-wellness".into();
    raw.payload = serde_json::json!({"items": []});
    db.persist_fetched_record(&raw).unwrap();
    raw.stream = "heart_rate".into();
    raw.source_key = "empty-heart-rate".into();
    assert!(db.persist_fetched_record(&raw).is_err());
    // Only unreadable payloads count as quarantined. An empty set from the cloud means "nothing in this period" and is reported on its own (#143:
    // the heart rate endpoint returned empty for each day, thousands were quarantined, and the user thought parsing was broken).
    raw.source_key = "unreadable-heart-rate".into();
    raw.payload = serde_json::json!({"unexpected": 1});
    assert!(db.persist_fetched_record(&raw).is_err());
    let health = db.data_health(14, 0).unwrap();
    assert_eq!(health.database.pending_normalization, 0);
    assert!(health
        .database
        .normalization_by_stream
        .iter()
        .any(|r| r.stream == "wellness"
            && r.state == "processed_without_output"
            && r.records == 1));
    assert!(health
        .database
        .normalization_by_stream
        .iter()
        .any(|r| r.stream == "heart_rate" && r.state == "quarantined" && r.records == 1));
    assert!(health
        .database
        .normalization_by_stream
        .iter()
        .any(|r| r.stream == "heart_rate" && r.state == "empty_response" && r.records == 1));

    // An interrupted sync leaves raw pending even at the current revision.
    db.set_app_meta("normalizer_revision", NORMALIZER_REVISION)
        .unwrap();
    raw.stream = "daily_summary".into();
    raw.source_key = "first-page".into();
    raw.payload = serde_json::json!({"data": [{"date": "2026-09-14", "steps": 5678}]});
    db.insert_raw_record(&raw).unwrap();
    assert_eq!(
        db.data_health(14, 0)
            .unwrap()
            .database
            .pending_normalization,
        1
    );
    assert!(db.pending_replay_plan().unwrap().is_some());
    db.reprocess_raw_records_if_needed().unwrap().unwrap();
    assert_eq!(
        db.data_health(14, 0)
            .unwrap()
            .database
            .pending_normalization,
        0
    );
    assert!(db.pending_replay_plan().unwrap().is_none());
}

#[test]
fn a_replayed_workout_detail_is_not_pending_normalization() {
    // Workout detail output goes into workout_samples / route_points / workout_pauses.
    // These three tables link by workout_id and have no raw_record_id. The old count checked only the four
    // tables with raw_record_id, so every well-parsed detail payload counted as
    // "pending normalization" forever. However often the user replayed, the number did not drop.
    let db = db();
    db.insert_workout(&Workout {
        workout_id: "1700000000".into(),
        workout_type: "run".into(),
        normalized_type: "run".into(),
        type_source: "numeric_mapped".into(),
        user_override: None,
        effective_type: "run".into(),
        custom_label: None,
        start_time: day(0),
        end_time: day(0) + Duration::minutes(10),
        distance_meters: Some(1000.0),
        calories: Some(80),
        avg_hr: Some(140),
        max_hr: Some(160),
        training_load: None,
        vo2max: None,
        source_scope: SourceScope::Device,
        device_id: None,
        synced_at: None,
        gps_available: false,
        sample_count: 0,
        zepp_source: Some("run.gps".into()),
        zepp_type: Some(1),
        ..Default::default()
    })
    .unwrap();

    let payload = serde_json::json!({
        "trackid": 1_700_000_000i64,
        "source": "run.gps",
        "time": "0;1;",
        "longitude_latitude": "4004663552,11629333504;16403,8392;",
        "heart_rate": "1,80;1,2;"
    });
    let source_key = "workout_detail:1700000000:run.gps";
    let raw_id = db
        .insert_raw_record(&RawRecord {
            stream: "workout_detail".into(),
            source_key: source_key.into(),
            source_scope: SourceScope::Device,
            device_id: None,
            start_utc: day(0),
            end_utc: None,
            payload: payload.clone(),
            capability: CapabilityStatus::Verified,
        })
        .unwrap();
    db.normalize_and_persist_raw(raw_id, "workout_detail", source_key, &payload)
        .unwrap();

    assert_eq!(
        db.data_health(90, 0)
            .unwrap()
            .database
            .pending_normalization,
        0,
        "a detail that already produced per-point samples must not count as \"pending normalization\""
    );

    // The reverse check: a payload that really produced nothing must still be counted,
    // or this fix would only "hide the problem".
    db.insert_raw_record(&RawRecord {
        stream: "workout_detail".into(),
        source_key: "workout_detail:1799999999:run.gps".into(),
        source_scope: SourceScope::Device,
        device_id: None,
        start_utc: day(1),
        end_utc: None,
        payload: serde_json::json!({ "items": [] }),
        capability: CapabilityStatus::Verified,
    })
    .unwrap();
    assert_eq!(
        db.data_health(90, 0)
            .unwrap()
            .database
            .pending_normalization,
        1,
        "a payload that produced no records must still be counted"
    );
}
/// The data health page must know every stream that sync runs.
///
/// `STREAM_CATALOG` is the full content of that page: a stream not in this table has no row
/// on the page. There is no "last fetched at", no gap check, and nothing turns red when it starts
/// to fail. Sync can add a stream without touching this table, so the two lists
/// drift apart silently. That is how weight was missed: fetch, storage, export and contract were all correct,
/// but this page acted as if it did not exist.
///
/// This test does not grep the source. It hard-codes the names that sync really emits as a reference:
/// whoever adds a stream must change both places, or this test fails.
#[test]
fn every_synced_stream_appears_on_the_health_page() {
    // Matches the first argument of each `emit(...)` in `SyncManager::run`, one to one.
    const SYNCED_STREAMS: [&str; 8] = [
        "heart_rate",
        "daily_summary",
        "workouts",
        "workout_detail",
        "sleep",
        "hrv",
        "wellness",
        "weight",
    ];
    let known: std::collections::BTreeSet<&str> = STREAM_CATALOG
        .iter()
        .map(|(stream, _, _)| *stream)
        .collect();
    let missing: Vec<&str> = SYNCED_STREAMS
        .iter()
        .copied()
        .filter(|stream| !known.contains(stream))
        .collect();
    assert!(
        missing.is_empty(),
        "these streams sync, but the data health page does not know them: {missing:?}"
    );
}
