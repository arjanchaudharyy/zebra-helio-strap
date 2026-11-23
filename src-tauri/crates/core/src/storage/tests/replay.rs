use super::*;

#[test]
fn changing_a_device_assignment_replays_context_types_and_preserves_workout_corrections() {
    let db = Database::in_memory().unwrap();
    db.insert_raw_record(&RawRecord {
        stream: "workouts".into(),
        source_key: "sport_history:run:assignment-change".into(),
        source_scope: SourceScope::Device,
        device_id: None,
        start_utc: ts(),
        end_utc: None,
        payload: serde_json::json!({"items": [{
            "workoutId": "assigned", "startTime": 1700000000, "endTime": 1700003600,
            "type": 15, "deviceId": "AABBCCDDEEFF"
        }]}),
        capability: CapabilityStatus::Verified,
    })
    .unwrap();
    db.reprocess_raw_records().unwrap();
    assert!(db.pending_replay_plan().unwrap().is_none());
    db.set_workout_type_override("assigned", Some("strength"))
        .unwrap();
    for (assignment, expected) in [
        (Some("amazfit-balance-2"), "open_water_swimming"),
        (Some("amazfit-t-rex-3"), "unknown:15"),
        (Some("amazfit-balance-2"), "open_water_swimming"),
        (None, "unknown:15"),
    ] {
        db.set_device_model_override("AABBCCDDEEFF", assignment)
            .unwrap();
        assert!(db.pending_replay_plan().unwrap().is_some());
        db.reprocess_raw_records_if_needed().unwrap().unwrap();
        let row = db.get_workout_detail("assigned").unwrap().unwrap();
        assert_eq!(row.normalized_type, expected);
        assert_eq!(row.effective_type, "strength");
        assert_eq!(row.zepp_type, Some(15));
        assert!(db.pending_replay_plan().unwrap().is_none());
    }
}

#[test]
fn context_type_replay_corrects_old_hiking_without_losing_local_edits() {
    let db = Database::in_memory().unwrap();
    db.insert_raw_record(&RawRecord {
        stream: "workouts".into(), source_key: "sport_history:run:context-types".into(),
        source_scope: SourceScope::Device, device_id: None, start_utc: ts(), end_utc: None,
        payload: serde_json::json!({"items": [
            {"workoutId": "balance", "startTime": 1700000000, "endTime": 1700003600, "type": 15, "deviceId": "AABBCCDDEEFF"},
            {"workoutId": "corrected", "startTime": 1700004000, "endTime": 1700007600, "type": 15, "deviceId": "AABBCCDDEEFF"},
            {"workoutId": "unattributed", "startTime": 1700008000, "endTime": 1700011600, "type": 15},
            {"workoutId": "other", "startTime": 1700012000, "endTime": 1700015600, "type": 15, "deviceId": "112233445566"},
            {"workoutId": "hike", "startTime": 1700016000, "endTime": 1700019600, "type": 224}
        ]}), capability: CapabilityStatus::Verified,
    }).unwrap();
    db.reprocess_raw_records().unwrap();
    // Emulate rows produced by the old unconditional 15 -> hiking rule.
    db.conn.execute("UPDATE workouts SET workout_type = 'hiking', workout_type_source = 'numeric_mapped' WHERE zepp_type = 15", []).unwrap();
    db.set_workout_type_override("corrected", Some("strength"))
        .unwrap();
    db.upsert_device_identity(&DeviceIdentityHint {
        aliases: vec!["AABBCCDDEEFF".into(), "23229501001311".into()],
        device_id: Some("AABBCCDDEEFF".into()),
        serial: Some("23229501001311".into()),
        name: Some("Amazfit T-Rex 3".into()),
        ..Default::default()
    })
    .unwrap();
    // The explicit user assignment via an alias supersedes an old profile name.
    db.set_device_model_override("23229501001311", Some("amazfit-balance-2"))
        .unwrap();
    db.set_app_meta(
        "normalizer_revision",
        "zepp-normalizer-before-context-types",
    )
    .unwrap();
    db.reprocess_raw_records_if_needed().unwrap().unwrap();
    for (id, expected) in [
        ("balance", "open_water_swimming"),
        ("corrected", "open_water_swimming"),
        ("unattributed", "unknown:15"),
        ("other", "unknown:15"),
        ("hike", "hiking"),
    ] {
        let row = db.get_workout_detail(id).unwrap().unwrap();
        assert_eq!(row.normalized_type, expected, "{id}");
        assert_eq!(
            row.effective_type,
            if id == "corrected" {
                "strength"
            } else {
                expected
            },
            "{id}"
        );
    }
    db.reprocess_raw_records().unwrap();
    assert_eq!(db.get_recent_workouts(10).unwrap().len(), 5);
    assert_eq!(
        db.get_workout_detail("corrected")
            .unwrap()
            .unwrap()
            .effective_type,
        "strength"
    );
}

#[test]
fn context_type_replay_uses_a_stored_device_name_and_rejects_conflicting_payload() {
    let db = Database::in_memory().unwrap();
    db.upsert_device_identity(&DeviceIdentityHint {
        device_id: Some("AABBCCDDEEFF".into()),
        name: Some("Amazfit Balance 2".into()),
        ..Default::default()
    })
    .unwrap();
    db.insert_raw_record(&RawRecord {
        stream: "workouts".into(), source_key: "sport_history:run:profile-context".into(),
        source_scope: SourceScope::Device, device_id: None, start_utc: ts(), end_utc: None,
        payload: serde_json::json!({"items": [
            {"workoutId": "profile", "startTime": 1700000000, "endTime": 1700003600, "type": 15, "deviceId": "AABBCCDDEEFF"},
            {"workoutId": "conflict", "startTime": 1700004000, "endTime": 1700007600, "type": 15, "deviceId": "AABBCCDDEEFF", "deviceName": "Amazfit T-Rex 3"}
        ]}), capability: CapabilityStatus::Verified,
    }).unwrap();
    db.reprocess_raw_records().unwrap();
    assert_eq!(
        db.get_workout_detail("profile")
            .unwrap()
            .unwrap()
            .normalized_type,
        "open_water_swimming"
    );
    assert_eq!(
        db.get_workout_detail("conflict")
            .unwrap()
            .unwrap()
            .normalized_type,
        "unknown:15"
    );
}

/// One workout plus its detail payload. The detail carries the laps the watch recorded.
///
/// This fixture simulates an old database from before the detail rule was stored: the `lap` rows from the detail are already cleared, and the
/// replay tests below must restore them.
fn insert_workout_with_lap_detail(db: &Database) {
    db.insert_raw_record(&RawRecord {
        stream: "workouts".into(),
        source_key: "sport_history:run:lap-fixture".into(),
        source_scope: SourceScope::Device,
        device_id: None,
        start_utc: ts(),
        end_utc: None,
        payload: serde_json::json!({
            "data": [{
                "trackid": 1_700_000_000i64,
                "end_time": 1_700_000_600i64,
                "type": 1,
                "dis": "1000"
            }]
        }),
        capability: CapabilityStatus::Verified,
    })
    .unwrap();
    db.insert_raw_record(&RawRecord {
            stream: "workout_detail".into(),
            source_key: "workout_detail:1700000000:run.huami.com".into(),
            source_scope: SourceScope::Device,
            device_id: None,
            start_utc: ts(),
            end_utc: None,
            payload: serde_json::json!({
                "data": {
                    "trackid": 1_700_000_000i64,
                    "time": "0;300;300;",
                    "heart_rate": "0,150;300,2;300,-2;",
                    // Two laps of 500 m / 300 s each, cumulative 300 and 600. This matches the summary's
                    // 1000 m and 600 s, so both reconciliations pass.
                    "lap": "0,300,500,s00000000000,150,300,-20000;1,300,500,s00000000000,152,600,-20000;"
                }
            }),
            capability: CapabilityStatus::Verified,
        })
        .unwrap();
    // `insert_raw_record` stores only the payload. Derived rows are built only at replay. First normalize everything
    // once to build the workout summary and per-second samples. A real old database looks exactly like this.
    db.reprocess_raw_records().unwrap();
    // Then clear the laps to simulate a historical database that has no derived detail rows yet.
    db.conn.execute("DELETE FROM workout_laps", []).unwrap();
}

/// Two replays must not produce two copies of the derived rows.
///
/// `replace_workout_series` cleared four tables and inserted again, but missed the new
/// `workout_laps`. None of these tables has a unique index: the only protection is those
/// DELETE statements, so one missing table means pure appends. A real full replay on this machine turned 341 laps into
/// 682: all 341 `(workout_id, lap_index)` pairs were doubled.
///
/// What fooled everyone is that **the first replay is correct**: people who upgraded from an old version got
/// correct laps, and the count doubled only at the next revision bump. So this test counts "after the second replay",
/// not "after replay". It counts all four tables together, so when we add a new derived table this test
/// covers it too.
#[test]
fn replaying_twice_does_not_duplicate_derived_rows() {
    let db = Database::in_memory().unwrap();
    insert_workout_with_lap_detail(&db);
    let counts = || -> Vec<(String, i64)> {
        [
            "workout_laps",
            "workout_splits",
            "workout_samples",
            "workout_pauses",
            "workout_strength_sets",
        ]
        .into_iter()
        .map(|table| {
            let n = db
                .conn
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                    row.get::<_, i64>(0)
                })
                .unwrap();
            (table.to_string(), n)
        })
        .collect()
    };

    db.reprocess_raw_records().unwrap();
    let after_first = counts();
    assert!(
        after_first
            .iter()
            .any(|(table, n)| table == "workout_laps" && *n == 2),
        "the fixture itself must decode two laps, or this test checks nothing: {after_first:?}"
    );

    db.reprocess_raw_records().unwrap();
    assert_eq!(
        counts(),
        after_first,
        "derived rows grew after the second replay: a table is missing its DELETE before replace"
    );
}

#[test]
fn previous_release_upgrade_replays_laps_and_rucking_and_preserves_facts() {
    let db = Database::in_memory().unwrap();
    db.insert_raw_record(&RawRecord {
        stream: "workouts".into(),
        source_key: "workouts-225".into(),
        source_scope: SourceScope::Device,
        device_id: None,
        start_utc: ts(),
        end_utc: None,
        payload: serde_json::json!({
            "data": [{
                "workout_id": "same-workout",
                "start_time": 1_700_000_000i64,
                "end_time": 1_700_000_600i64,
                "type": 225,
                "calorie": 120
            }]
        }),
        capability: CapabilityStatus::Verified,
    })
    .unwrap();
    db.insert_raw_record(&RawRecord {
        stream: "workout_detail".into(),
        source_key: "workout_detail:same-workout:run.gps".into(),
        source_scope: SourceScope::Device,
        device_id: None,
        start_utc: ts(),
        end_utc: None,
        payload: serde_json::json!({
            "trackid": 1_700_000_000i64,
            "time": "0;1;",
            "longitude_latitude": "4004663552,11629333504;16403,8392;",
            "heart_rate": "1,80;1,2;",
            "lap": "0,300,500,s00000000000,150,300,-20000;1,300,500,s00000000000,152,600,-20000;"
        }),
        capability: CapabilityStatus::Verified,
    })
    .unwrap();
    db.insert_raw_record(&RawRecord {
        stream: "daily_summary".into(),
        source_key: "daily-summary-unrelated".into(),
        source_scope: SourceScope::UserFused,
        device_id: None,
        start_utc: ts(),
        end_utc: None,
        payload: serde_json::json!({
            "data": [{ "date": "2023-11-14", "steps": 1234 }]
        }),
        capability: CapabilityStatus::Verified,
    })
    .unwrap();
    db.insert_workout(&workout_with_type(Some(225), "unknown:225", "unknown_code"))
        .unwrap();
    db.set_workout_type_override("same-workout", Some("strength"))
        .unwrap();
    let raw_before: (
        String,
        Option<Vec<u8>>,
        String,
        String,
        String,
        Option<String>,
    ) = db
        .conn
        .query_row(
            "SELECT payload, payload_zip, source_key, source_scope, start_utc, device_id
                 FROM raw_records WHERE source_key = 'workouts-225'",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        )
        .unwrap();
    let precondition = db.get_workout_detail("same-workout").unwrap().unwrap();
    assert_eq!(precondition.normalized_type, "unknown:225");
    assert_eq!(precondition.type_source, "unknown_code");
    assert_eq!(precondition.user_override.as_deref(), Some("strength"));
    assert_eq!(precondition.effective_type, "strength");
    db.conn
        .execute(
            "INSERT INTO app_meta(key, value, updated_at)
                 VALUES('normalizer_revision', ?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![PREVIOUS_RELEASE_NORMALIZER_REVISION, ts().to_rfc3339()],
        )
        .unwrap();

    let plan = db.pending_replay_plan().unwrap().unwrap();
    assert!(plan.streams.is_empty());
    assert_eq!(plan.raw_records, 3, "all streams need attempt provenance");

    let counts = db.reprocess_raw_records_if_needed().unwrap().unwrap();

    assert!(counts.contains_key("workouts"), "counts = {counts:?}");
    assert!(counts.contains_key("workout_detail"), "counts = {counts:?}");
    assert!(counts.contains_key("daily_summary"), "{counts:?}");
    assert_eq!(
        db.conn
            .query_row("SELECT COUNT(*) FROM workout_laps", [], |row| row
                .get::<_, i64>(0),)
            .unwrap(),
        2,
        "the v21 to v23 upgrade must restore the workout_detail laps"
    );
    let stored = db.get_workout_detail("same-workout").unwrap().unwrap();
    assert_eq!(stored.normalized_type, "rucking");
    assert_eq!(stored.type_source, "numeric_mapped");
    assert_eq!(stored.user_override.as_deref(), Some("strength"));
    assert_eq!(stored.effective_type, "strength");
    let raw_after: (
        String,
        Option<Vec<u8>>,
        String,
        String,
        String,
        Option<String>,
    ) = db
        .conn
        .query_row(
            "SELECT payload, payload_zip, source_key, source_scope, start_utc, device_id
                 FROM raw_records WHERE source_key = 'workouts-225'",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(raw_after, raw_before, "replay must not rewrite raw payloads or source facts");
    assert_eq!(
        db.stored_normalizer_revision().unwrap().as_deref(),
        Some(NORMALIZER_REVISION)
    );
    assert!(db.reprocess_raw_records_if_needed().unwrap().is_none());
}

#[test]
fn an_unshipped_v22_revision_still_replays_all_streams() {
    let db = Database::in_memory().unwrap();
    insert_workout_with_lap_detail(&db);
    db.conn
        .execute(
            "INSERT INTO app_meta(key, value, updated_at)
                 VALUES('normalizer_revision', ?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params!["zepp-normalizer-2026-09-v22-watch-laps", ts().to_rfc3339()],
        )
        .unwrap();

    let plan = db.pending_replay_plan().unwrap().unwrap();
    assert!(plan.streams.is_empty(), "an unreleased v22 must use a full-database replay");
    assert_eq!(plan.raw_records, 2);

    let counts = db.reprocess_raw_records_if_needed().unwrap().unwrap();
    assert!(counts.contains_key("workouts"), "counts = {counts:?}");
    assert!(counts.contains_key("workout_detail"), "counts = {counts:?}");
    assert_eq!(
        db.conn
            .query_row("SELECT COUNT(*) FROM workout_laps", [], |row| row
                .get::<_, i64>(0),)
            .unwrap(),
        2,
        "an unreleased v22 must still use a full replay and restore the laps from the detail"
    );
}

/// How a headless user's database knows it owes a replay.
///
/// The desktop app replays automatically at startup. The CLI has no such startup. So "does it owe one" must be answerable
/// **read-only**: `status` must be able to say it, MCP must be able to report it, and neither may write a single
/// byte to the database.
#[test]
fn a_stale_library_can_be_recognized_without_writing_to_it() {
    let db = Database::in_memory().unwrap();
    // This fixture must land on **a stream this version replays**. Otherwise raw_records is 0,
    // and a database with 0 payloads owes no replay anyway. The test would then check something unrelated.
    // When you change the normalizer rules or the replayed streams, change this too.
    insert_workout_with_lap_detail(&db);
    db.conn
        .execute(
            "INSERT INTO app_meta(key, value, updated_at)
                 VALUES('normalizer_revision', ?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![PREVIOUS_RELEASE_NORMALIZER_REVISION, ts().to_rfc3339()],
        )
        .unwrap();

    let plan = db.pending_replay_plan().unwrap().unwrap();
    assert_eq!(
        plan.stored_revision.as_deref(),
        Some(PREVIOUS_RELEASE_NORMALIZER_REVISION)
    );
    assert_eq!(plan.target_revision, NORMALIZER_REVISION);
    assert!(plan.streams.is_empty());
    // The full-database replay plan must include the two raw payloads in the fixture.
    assert_eq!(plan.raw_records, 2);

    db.reprocess_raw_records_if_needed().unwrap().unwrap();
    assert!(
        db.pending_replay_plan().unwrap().is_none(),
        "after the replay is done it must not say it still owes one"
    );
}

/// An empty database owes no replay.
///
/// A fresh install has no recorded revision. That is not "history stuck on an old parser", it just has no
/// history yet. Telling it "your data needs a replay" is an empty warning, and `status`
/// may be called by a scheduler script every minute.
#[test]
fn a_brand_new_library_is_not_told_it_owes_a_replay() {
    let db = Database::in_memory().unwrap();
    let plan = db.pending_replay_plan().unwrap().unwrap();
    assert_eq!(plan.raw_records, 0);
    assert!(
        !db.data_health(30, 0)
            .unwrap()
            .database
            .normalizer_replay_pending
    );

    // But it must still get the current revision stamp. Otherwise the first sync triggers a pointless replay.
    db.reprocess_raw_records_if_needed().unwrap().unwrap();
    assert_eq!(
        db.stored_normalizer_revision().unwrap().as_deref(),
        Some(NORMALIZER_REVISION)
    );
}

/// The health report must state the database's revision, not the program's own constant.
#[test]
fn health_reports_the_revision_the_library_actually_has() {
    let db = Database::in_memory().unwrap();
    // Same as above: it must land on a stream this version replays. Otherwise "owes a replay" does not apply.
    insert_workout_with_lap_detail(&db);
    db.conn
        .execute(
            "INSERT INTO app_meta(key, value, updated_at)
                 VALUES('normalizer_revision', ?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![PREVIOUS_RELEASE_NORMALIZER_REVISION, ts().to_rfc3339()],
        )
        .unwrap();

    let health = db.data_health(30, 0).unwrap();
    assert_eq!(health.database.normalizer_revision, NORMALIZER_REVISION);
    assert_eq!(
        health.database.stored_normalizer_revision.as_deref(),
        Some(PREVIOUS_RELEASE_NORMALIZER_REVISION)
    );
    assert!(health.database.normalizer_replay_pending);
    assert!(
        health.actions.iter().any(|action| action.id == "reprocess"),
        "when the database is stuck on an old parser, the health report must give that action"
    );
}

/// Batched commits must not miss records across batches.
///
/// Replay now wraps 64 payloads per transaction. Batch boundaries are new, and boundaries are exactly where
/// this kind of change most often goes wrong: one missed commit, or a last partial batch that is not
/// processed, and the user sees "the replay finished, but some records did not change".
#[test]
fn batched_replay_covers_every_record_across_batch_boundaries() {
    let db = Database::in_memory().unwrap();
    let total = REPLAY_BATCH_RECORDS * 2 + 5;
    for index in 0..total {
        let track = 1_700_000_000i64 + index as i64 * 7200;
        db.insert_raw_record(&RawRecord {
            stream: "workouts".into(),
            source_key: format!("sport_history:0:{index}"),
            source_scope: SourceScope::Device,
            device_id: None,
            start_utc: ts(),
            end_utc: None,
            payload: serde_json::json!({
                "data": [{ "trackid": track, "end_time": track + 3600, "type": 211 }]
            }),
            capability: CapabilityStatus::Verified,
        })
        .unwrap();
    }
    db.conn
        .execute(
            "INSERT INTO app_meta(key, value, updated_at)
                 VALUES('normalizer_revision', ?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params!["zepp-normalizer-before-selective-replay", ts().to_rfc3339()],
        )
        .unwrap();

    db.reprocess_raw_records_if_needed().unwrap().unwrap();

    assert_eq!(
        db.normalized_stream_count("workouts").unwrap(),
        Some(total as i64)
    );
    let workouts = db.get_recent_workouts(total + 10).unwrap();
    assert_eq!(workouts.len(), total);
    assert!(
        workouts
            .iter()
            .all(|workout| workout.workout_type == "road_cycling"),
        "the last partial batch must also go through the new rules"
    );
}

/// The upgrade path that replays only workouts must not delete workout detail as a side effect.
///
/// `workout_samples`, `route_points`, `workout_pauses` and `workout_splits`
/// all hang on `workouts` with `ON DELETE CASCADE`. If a workouts replay
/// first deleted summary rows by raw_record_id, the cascade would take the workout's per-second series, GPS track and
/// splits with it. This upgrade path does not replay workout_detail, so they would never come
/// back. The user would see "after the upgrade, the tracks of all my old workouts are gone", while the upgrade itself
/// reports success.
#[test]
fn replaying_workout_summaries_does_not_erase_the_detail_series() {
    let db = Database::in_memory().unwrap();
    db.persist_fetched_record(&RawRecord {
        stream: "workouts".into(),
        source_key: "sport_history:0:1".into(),
        source_scope: SourceScope::Device,
        device_id: None,
        start_utc: ts(),
        end_utc: None,
        payload: serde_json::json!({
            "data": [{ "trackid": 1_700_000_000i64, "end_time": 1_700_003_600i64, "type": 211 }]
        }),
        capability: CapabilityStatus::Verified,
    })
    .unwrap();
    db.persist_fetched_record(&RawRecord {
        stream: "workout_detail".into(),
        source_key: "workout_detail:1700000000:run.gps".into(),
        source_scope: SourceScope::Device,
        device_id: None,
        start_utc: ts(),
        end_utc: None,
        payload: serde_json::json!({
            "trackid": 1_700_000_000i64,
            "source": "run.gps",
            "time": "0;1;",
            "longitude_latitude": "4004663552,11629333504;16403,8392;",
            "heart_rate": "1,80;1,2;"
        }),
        capability: CapabilityStatus::Verified,
    })
    .unwrap();
    let samples_before = db.normalized_stream_count("workout_detail").unwrap();
    let route_before: i64 = db
        .conn
        .query_row("SELECT COUNT(*) FROM route_points", [], |row| row.get(0))
        .unwrap();
    assert!(samples_before.unwrap_or(0) > 0 && route_before > 0);

    db.reprocess_raw_records_for_stream(Some(&["workouts"]))
        .unwrap();

    assert_eq!(
        db.normalized_stream_count("workout_detail").unwrap(),
        samples_before,
        "replaying workout summaries must not take the per-second series with them"
    );
    let route_after: i64 = db
        .conn
        .query_row("SELECT COUNT(*) FROM route_points", [], |row| row.get(0))
        .unwrap();
    assert_eq!(route_after, route_before, "replaying workout summaries must not take the GPS track with them");
    // The summary itself must still be recomputed with the new rules.
    assert_eq!(
        db.get_recent_workouts(10).unwrap()[0].workout_type,
        "road_cycling"
    );
}

/// A bad raw payload must not be swallowed, must not turn the whole replay into Err, and above all must not cause a stamp.
#[test]
fn replay_quarantines_bad_raw_and_still_stamps_after_one_full_pass() {
    let db = Database::in_memory().unwrap();
    db.insert_raw_record(&RawRecord {
        stream: "workouts".into(),
        source_key: "sport_history:0:good".into(),
        source_scope: SourceScope::Device,
        device_id: None,
        start_utc: ts(),
        end_utc: None,
        payload: serde_json::json!({
            "data": [{ "trackid": 1_700_000_000i64, "end_time": 1_700_003_600i64, "type": 211 }]
        }),
        capability: CapabilityStatus::Verified,
    })
    .unwrap();
    let empty_id = db
        .insert_raw_record(&RawRecord {
            stream: "workouts".into(),
            source_key: "sport_history:0:empty".into(),
            source_scope: SourceScope::Device,
            device_id: None,
            start_utc: ts(),
            end_utc: None,
            payload: serde_json::json!({}),
            capability: CapabilityStatus::Verified,
        })
        .unwrap();
    db.conn
            .execute(
                "INSERT INTO raw_records
                    (stream, source_key, source_scope, device_id, start_utc, end_utc,
                     payload, payload_hash, fetched_at)
                 VALUES ('workouts', 'sport_history:0:not-json', 'device', NULL, ?1, NULL, '{', 'hash', ?1)",
                params![ts().to_rfc3339()],
            )
            .unwrap();
    let not_json_id = db.conn.last_insert_rowid();
    db.conn
        .execute(
            "INSERT INTO app_meta(key, value, updated_at)
                 VALUES('normalizer_revision', ?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params!["zepp-normalizer-ancient", ts().to_rfc3339()],
        )
        .unwrap();

    let counts = db
        .reprocess_raw_records_if_needed()
        .expect("a bad payload must not make the whole replay return Err")
        .expect("an old revision must trigger a replay");
    assert!(
        counts.get("workouts").copied().unwrap_or(0) >= 1,
        "a good payload must produce derived rows: {counts:?}"
    );
    let workouts = db.get_recent_workouts(10).unwrap();
    assert_eq!(workouts.len(), 1);
    assert_eq!(workouts[0].workout_type, "road_cycling");

    let quarantined: i64 = db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM raw_quarantine
                 WHERE raw_record_id IN (?1, ?2) AND revision = ?3",
            params![empty_id, not_json_id, NORMALIZER_REVISION],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(quarantined, 2);
    let failures: i64 = db
        .conn
        .query_row(
            "SELECT value FROM app_meta WHERE key = 'replay_last_failures'",
            [],
            |row| row.get::<_, String>(0),
        )
        .unwrap()
        .parse()
        .unwrap();
    assert!(failures >= 1, "failures = {failures}");
    // Running the same rules again gives the same result: after one full pass the revision advances, and failures stay in the quarantine table.
    // Before, it did not advance, so each revision bump replayed the full database twice (real 3.0.3 / 3.0.4 databases did this).
    assert_eq!(
        db.stored_normalizer_revision().unwrap().as_deref(),
        Some(NORMALIZER_REVISION),
        "one full pass advances the revision"
    );
    assert!(
        db.pending_replay_plan().unwrap().is_none(),
        "quarantined payloads must not make the next startup owe another replay"
    );
    let raw_kept: i64 = db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM raw_records WHERE id IN (?1, ?2)",
            params![empty_id, not_json_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(raw_kept, 2);
}

/// A normalization failure must return the error, but must not roll back the raw data we already have.
#[test]
fn persist_keeps_raw_when_normalization_fails() {
    let db = Database::in_memory().unwrap();
    let error = db
        .persist_fetched_record(&RawRecord {
            stream: "workouts".into(),
            source_key: "sport_history:0:unparseable".into(),
            source_scope: SourceScope::Device,
            device_id: None,
            start_utc: ts(),
            end_utc: None,
            payload: serde_json::json!({}),
            capability: CapabilityStatus::Verified,
        })
        .unwrap_err();
    assert!(
        matches!(
            error,
            ZeppBridgeError::ParseError(_) | ZeppBridgeError::DataUnavailable(_)
        ),
        "{error}"
    );
    let kept: i64 = db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM raw_records WHERE source_key = 'sport_history:0:unparseable'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(kept, 1);
    let quarantined: i64 = db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM raw_quarantine WHERE source_key = 'sport_history:0:unparseable'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(quarantined, 1);
}

/// After the upgrade, the old `unknown:211` is recognized again as road cycling.
///
/// The person who reported this cared about history: 199 records were already stored as `unknown:211`.
/// Adding the code to the catalog only fixes new records. No old record would change. So this test pins
/// not the catalog but the fact that "a catalog change triggers an automatic replay".
///
/// The database stores an older revision and takes the full replay path. **Do not use the previous version's
/// revision**: that path replays only the streams that version really changed, and that changes with each version. Pinning it here
/// would make this test fail falsely at each version bump.
#[test]
fn upgrading_replays_history_so_unknown_211_becomes_road_cycling() {
    let db = Database::in_memory().unwrap();
    db.insert_raw_record(&RawRecord {
        stream: "workouts".into(),
        source_key: "workouts-211".into(),
        source_scope: SourceScope::Device,
        device_id: None,
        start_utc: ts(),
        end_utc: None,
        payload: serde_json::json!({
            "data": [{
                "trackid": 1_700_000_000i64,
                "end_time": 1_700_003_600i64,
                "type": 211
            }]
        }),
        capability: CapabilityStatus::Verified,
    })
    .unwrap();
    db.conn
        .execute(
            "INSERT INTO app_meta(key, value, updated_at)
                 VALUES('normalizer_revision', ?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params!["zepp-normalizer-ancient", ts().to_rfc3339()],
        )
        .unwrap();

    db.reprocess_raw_records_if_needed().unwrap().unwrap();

    let workouts = db.get_recent_workouts(10).unwrap();
    assert_eq!(workouts.len(), 1);
    assert_eq!(workouts[0].workout_type, "road_cycling");
}

#[test]
fn workout_type_merge_is_order_independent_and_numeric_evidence_wins() {
    let numeric = workout_with_type(Some(105), "unknown:105", "unknown_code");
    let string = workout_with_type(None, "strength", "string_field");
    let first = Database::in_memory().unwrap();
    first.insert_workout(&string).unwrap();
    first.insert_workout(&numeric).unwrap();
    let second = Database::in_memory().unwrap();
    second.insert_workout(&numeric).unwrap();
    second.insert_workout(&string).unwrap();
    let a = first.get_workout_detail("same-workout").unwrap().unwrap();
    let b = second.get_workout_detail("same-workout").unwrap().unwrap();
    assert_eq!(a.normalized_type, "unknown:105");
    assert_eq!(a.normalized_type, b.normalized_type);
    assert_eq!(a.type_source, b.type_source);
    assert_eq!(a.zepp_type, b.zepp_type);
}

#[test]
fn workout_override_survives_replay_and_does_not_replace_raw_facts() {
    let db = Database::in_memory().unwrap();
    let workout = workout_with_type(Some(105), "unknown:105", "unknown_code");
    db.insert_workout(&workout).unwrap();
    db.set_workout_type_override("same-workout", Some("strength"))
        .unwrap();
    let mut replay = workout.clone();
    replay.synced_at = Some(ts() + chrono::Duration::days(1));
    db.insert_workout(&replay).unwrap();
    let stored = db.get_workout_detail("same-workout").unwrap().unwrap();
    assert_eq!(stored.zepp_type, Some(105));
    assert_eq!(stored.normalized_type, "unknown:105");
    assert_eq!(stored.user_override.as_deref(), Some("strength"));
    assert_eq!(stored.effective_type, "strength");
    assert_eq!(stored.synced_at, workout.synced_at);
    let export = parsed_export(&db, &["workouts"], ExportDetail::Summary);
    let exported = &export["data"]["workouts"][0];
    assert_eq!(exported["zepp_type"], 105);
    assert_eq!(exported["normalized_type"], "unknown:105");
    assert_eq!(exported["type_source"], "unknown_code");
    assert_eq!(exported["user_override"], "strength");
    assert_eq!(exported["effective_type"], "strength");
    assert_eq!(exported["workout_type"], "strength");
    db.set_workout_type_override("same-workout", None).unwrap();
    assert_eq!(
        db.get_workout_detail("same-workout")
            .unwrap()
            .unwrap()
            .effective_type,
        "unknown:105"
    );
}

#[test]
fn compressed_raw_payloads_survive_a_round_trip_and_still_replay() {
    // The raw payload is the only basis for replay. Compression may change only how many bytes it takes, not what it is.
    // One byte lost, and this record is destroyed forever.
    let db = Database::in_memory().unwrap();
    let payload = serde_json::json!({
        "items": (0..200).map(|index| serde_json::json!({
            "time": format!("2026-08-{:02}T00:00:00Z", (index % 28) + 1),
            "bpm": 60 + (index % 40),
        })).collect::<Vec<_>>()
    });
    db.insert_raw_record(&RawRecord {
        stream: "wellness".into(),
        source_key: "wellness:spo2:user_events:2026-08-01:2026-08-08".into(),
        source_scope: SourceScope::UserFused,
        device_id: None,
        start_utc: ts(),
        end_utc: Some(ts() + chrono::Duration::days(7)),
        payload: payload.clone(),
        capability: CapabilityStatus::Unverified,
    })
    .unwrap();

    // The stored form is compressed, and clearly smaller.
    let (stored_text, zipped): (String, Option<Vec<u8>>) = db
        .conn
        .query_row(
            "SELECT payload, payload_zip FROM raw_records ORDER BY id DESC LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(stored_text, "", "after compression no plain-text copy may remain");
    let zipped = zipped.expect("a newly written payload must be compressed");
    let expected = serde_json::to_string(&payload).unwrap();
    assert!(
        zipped.len() < expected.len(),
        "compression that makes it larger has no point: {} vs {}",
        zipped.len(),
        expected.len()
    );

    // Decompression must restore every byte exactly.
    assert_eq!(
        decode_raw_payload(String::new(), Some(zipped)).unwrap(),
        expected
    );

    // Plain-text rows in old databases still read as usual.
    assert_eq!(
        decode_raw_payload("{\"legacy\":true}".into(), None).unwrap(),
        "{\"legacy\":true}"
    );
    assert_eq!(
        decode_raw_payload("{\"legacy\":true}".into(), Some(Vec::new())).unwrap(),
        "{\"legacy\":true}"
    );
}

#[test]
fn decompress_payload_rejects_output_over_32_mib() {
    let oversized = "a".repeat(MAX_DECOMPRESSED_PAYLOAD_BYTES as usize + 1);
    let zipped = compress_payload(&oversized).unwrap();
    drop(oversized);
    let error = decompress_payload(&zipped).expect_err("going over the limit must be Err");
    assert!(matches!(error, ZeppBridgeError::ParseError(_)), "{error}");
}

#[test]
fn decompress_payload_rejects_corrupt_bytes_without_panic() {
    let error = decompress_payload(&[0xff, 0x00, 0x01, 0x02]).expect_err("corrupt bytes must be Err");
    assert!(matches!(error, ZeppBridgeError::ParseError(_)), "{error}");
}

#[test]
fn compacting_history_leaves_the_payload_readable() {
    // Existing payloads are plain text collected before this version. Compressing them must not change the content of any of them.
    let db = Database::in_memory().unwrap();
    let text = serde_json::to_string(&serde_json::json!({
        "summary": "x".repeat(4000),
    }))
    .unwrap();
    db.conn
        .execute(
            "INSERT INTO raw_records
                    (stream, source_key, source_scope, device_id, start_utc, end_utc,
                     payload, payload_hash, fetched_at)
                 VALUES ('wellness', 'legacy:1', 'device', NULL, ?1, NULL, ?2, 'hash', ?1)",
            params![ts().to_rfc3339(), text],
        )
        .unwrap();

    let report = db.compact_raw_payloads().unwrap();
    assert_eq!(report.compacted, 1);
    assert_eq!(report.skipped, 0);
    assert!(report.bytes_after < report.bytes_before);

    let (stored, zipped): (String, Option<Vec<u8>>) = db
        .conn
        .query_row(
            "SELECT payload, payload_zip FROM raw_records WHERE source_key = 'legacy:1'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(decode_raw_payload(stored, zipped).unwrap(), text);

    // A second compression finds nothing to compress, and must not count rows already compressed.
    let again = db.compact_raw_payloads().unwrap();
    assert_eq!(again.compacted, 0);
    // After compression they must not count as "pending compression". Otherwise every startup shows a pointless progress prompt.
    assert_eq!(db.pending_raw_payload_count().unwrap(), 0);
}

#[test]
fn tiny_payloads_are_never_counted_as_pending() {
    // An empty response `{"items":[]}` is only 12 bytes. Compressed, it is larger than the original, so the compression function skips it.
    // If the count function still counted it as "pending", every startup would decide there is work to do,
    // show "compressing", then finish at once with 0 rows. A real database did exactly this.
    let db = Database::in_memory().unwrap();
    for index in 0..3 {
        db.conn
                .execute(
                    "INSERT INTO raw_records
                        (stream, source_key, source_scope, device_id, start_utc, end_utc,
                         payload, payload_hash, fetched_at)
                     VALUES ('wellness', ?2, 'device', NULL, ?1, NULL, '{\"items\":[]}', 'hash', ?1)",
                    params![ts().to_rfc3339(), format!("empty:{index}")],
                )
                .unwrap();
    }
    assert_eq!(db.pending_raw_payload_count().unwrap(), 0);
    let report = db.compact_raw_payloads().unwrap();
    assert_eq!(report.compacted, 0);
    assert_eq!(report.skipped, 0, "payloads too small to compress must not be fetched at all");
}

#[test]
fn nested_replay_guards_keep_the_flag_until_the_outer_drops() {
    // Other tests may replay in parallel, so we cannot assert that the global count is exactly 0 on entry/exit.
    // This only pins: the inner Drop must not clear the flag the outer one still holds.
    let outer = ReplayGuard::enter();
    assert!(replay_in_progress());
    {
        let inner = ReplayGuard::enter();
        assert!(replay_in_progress());
        drop(inner);
        assert!(
            replay_in_progress(),
            "inner drop must not hide the outer wait-for-lock"
        );
    }
    drop(outer);
}

#[test]
fn replaying_strength_workout_extracts_sets_volume_and_rest_without_polluting_laps() {
    let db = Database::in_memory().unwrap();
    db.insert_raw_record(&RawRecord {
        stream: "workouts".into(),
        source_key: "sport_history:run:strength-1".into(),
        source_scope: SourceScope::Device,
        device_id: None,
        start_utc: ts(),
        end_utc: None,
        payload: serde_json::json!({
            "items": [{
                "workoutId": "str-1",
                "startTime": 1700000000,
                "endTime": 1700001800,
                "type": 52,
                "total_group": 2,
                "restTimeBetweenGroupsWithMillis": 95000,
                "strength_training_group": "[{\"actionType\":65,\"count\":12},{\"actionType\":1030,\"count\":10}]"
            }]
        }),
        capability: CapabilityStatus::Verified,
    })
    .unwrap();
    db.reprocess_raw_records().unwrap();

    // Before the detail is fetched, fall back to the history summary. Do not invent weights or per-set durations.
    let fallback = db.get_workout_series("str-1").unwrap().strength.unwrap();
    assert_eq!(fallback.total_sets, 2);
    assert_eq!(fallback.total_reps, 22);
    assert_eq!(fallback.total_volume_kg, None);
    assert_eq!(fallback.rest_seconds, Some(95));
    assert!(fallback.sets.is_empty());
    assert_eq!(fallback.exercise_groups.len(), 2);

    // Synthesize a minimal anonymized 62-column strength lap (set 1: 20 kg x 12 Biceps Curl, set 2: 20 lb x 10, unknown code 1030).
    let make_row = |idx: i32,
                    sec: i32,
                    hr: i32,
                    kcal: i32,
                    weight: &str,
                    reps: i32,
                    unit: i32,
                    code: i32,
                    ms: i32| {
        let mut cols = vec!["0".to_string(); 62];
        cols[0] = idx.to_string();
        cols[1] = sec.to_string();
        cols[2] = "0.0".into();
        cols[4] = hr.to_string();
        cols[5] = "0".into();
        cols[15] = kcal.to_string();
        cols[21] = weight.into();
        cols[22] = reps.to_string();
        cols[23] = unit.to_string();
        cols[28] = code.to_string();
        cols[43] = ms.to_string();
        cols.join(",")
    };
    let lap_str = format!(
        "{};{}",
        make_row(0, 22, 112, 4, "20.0", 12, 0, 65, 22400),
        make_row(1, 25, 118, 5, "20.0", 10, 1, 1030, 25100),
    );
    db.insert_raw_record(&RawRecord {
        stream: "workout_detail".into(),
        source_key: "workout_detail:str-1:test".into(),
        source_scope: SourceScope::Device,
        device_id: None,
        start_utc: ts(),
        end_utc: None,
        payload: serde_json::json!({
            "data": {
                "trackid": 1700000000,
                "type": 52,
                "lap": lap_str,
                "strengthSets": "[]",
                "strengthAssess": "[{\"eq\":[{\"sce\":80}]}]"
            }
        }),
        capability: CapabilityStatus::Verified,
    })
    .unwrap();
    db.reprocess_raw_records().unwrap();
    // Replay again to confirm that the workout_strength_sets replacement is idempotent and does not double.
    db.reprocess_raw_records().unwrap();

    let series = db.get_workout_series("str-1").unwrap();
    assert!(
        series.laps.is_empty(),
        "a 0 m strength set must not land in the running workout_laps"
    );
    let strength = series.strength.expect("strength detail must be produced");
    assert_eq!(strength.total_sets, 2);
    assert_eq!(strength.total_reps, 22);
    // 20 kg * 12 + (20 * 0.45359237 -> 9.072 kg) * 10 = 240 + 90.72 = 330.72 kg
    assert_eq!(strength.total_volume_kg, Some(330.72));
    assert_eq!(strength.rest_seconds, Some(95));
    assert_eq!(strength.sets.len(), 2);
    assert_eq!(
        strength.sets[0].exercise_key.as_deref(),
        Some("biceps_curl")
    );
    assert_eq!(strength.sets[0].muscle_group.as_deref(), Some("biceps"));
    assert_eq!(strength.sets[0].weight_unit, "kg");
    assert_eq!(strength.sets[1].action_code, 1030);
    assert_eq!(strength.sets[1].exercise_key, None);
    assert_eq!(strength.sets[1].weight_unit, "lb");
}
