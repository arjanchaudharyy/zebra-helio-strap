use super::*;

#[test]
fn mock_blood_pressure_reaches_storage_series_export_and_capabilities() {
    let db = Database::in_memory().unwrap();
    let at = DateTime::<Utc>::from_naive_utc_and_offset(
        (Utc::now() - Duration::days(1))
            .date_naive()
            .and_hms_opt(12, 0, 0)
            .unwrap(),
        Utc,
    );
    let raw = RawRecord {
        stream: "wellness".into(),
        source_key: "wellness:blood_pressure:mock:1".into(),
        source_scope: SourceScope::UserFused,
        device_id: None,
        start_utc: at,
        end_utc: None,
        capability: CapabilityStatus::Unverified,
        payload: serde_json::json!({"items":[
            {"timestamp":at.timestamp_millis(),"systolic":115,"diastolic":77,"pulse":53},
            {"timestamp":(at + Duration::minutes(5)).timestamp_millis(),"systolic":114,"diastolic":76}
        ]}),
    };
    db.persist_fetched_record(&raw).unwrap();
    db.persist_fetched_record(&raw).unwrap();
    let health = db.data_health(30, 0).unwrap();
    assert_eq!(
        health
            .streams
            .iter()
            .find(|s| s.stream == "wellness")
            .unwrap()
            .canonical_records,
        5
    );
    assert_eq!(
        health
            .streams
            .iter()
            .find(|s| s.stream == "daily_summary")
            .unwrap()
            .canonical_records,
        0
    );
    let series = db
        .metric_series(
            &[
                "blood_pressure_systolic".into(),
                "blood_pressure_diastolic".into(),
                "blood_pressure_pulse".into(),
            ],
            7,
        )
        .unwrap();
    assert_eq!(series.len(), 3);
    assert_eq!(series[0].points.len(), 1);
    assert_eq!(series[0].points[0].value, 114.5);
    assert_eq!(series[0].minimum, Some(114.0));
    assert_eq!(series[0].maximum, Some(115.0));
    let item = db
        .capability_overview(Local::now().date_naive())
        .unwrap()
        .items
        .into_iter()
        .find(|i| i.stream == "blood_pressure")
        .unwrap();
    assert!(item.ingested);
    assert_eq!(item.records, 2);
    let date = at.with_timezone(&Local).date_naive();
    let export = db
        .build_ai_export(&ExportSelection {
            scope: Some(ExportScope::date_range(date.to_string(), date.to_string())),
            start_date: None,
            end_date: None,
            data_types: vec!["blood_pressure".into()],
            detail: ExportDetail::Full,
        })
        .unwrap();
    let export: serde_json::Value = serde_json::from_str(&export.0).unwrap();
    let samples = export["data"]["metric_samples"].as_array().unwrap();
    assert_eq!(
        samples.len(),
        5,
        "no duplicate readings and no made-up pulse"
    );
    assert!(samples
        .iter()
        .any(|s| s["timestamp"].as_str().unwrap() == at.to_rfc3339()));
    assert_eq!(
        export["capabilities"]["blood_pressure"]["status"],
        "available"
    );
    let unknown = RawRecord {
        source_key: "wellness:blood_pressure:mock:unknown".into(),
        payload: serde_json::json!({"items":[{"opaque":"encoded"}]}),
        ..raw
    };
    assert!(db.persist_fetched_record(&unknown).is_err());
    assert_eq!(
        db.conn
            .query_row("SELECT COUNT(*) FROM raw_records", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        2,
        "unknown raw survives for replay"
    );
}

#[test]
fn metric_sample_types_export_without_an_unrelated_selection() {
    let db = Database::in_memory().unwrap();
    for (metric, value, unit) in [
        ("hrv_rmssd", 42.0, "ms"),
        ("respiratory_rate", 16.0, "brpm"),
    ] {
        db.insert_metric_sample(&MetricSample {
            metric: metric.into(),
            timestamp: ts(),
            value,
            unit: unit.into(),
            source_scope: SourceScope::Device,
            device_id: None,
        })
        .unwrap();
    }

    for detail in [ExportDetail::Summary, ExportDetail::Full] {
        for (metric, expected) in [("hrv_rmssd", 42.0), ("respiratory_rate", 16.0)] {
            let export = parsed_export(&db, &[metric], detail);
            let samples = export["data"]["metric_samples"].as_array().unwrap();
            assert_eq!(samples.len(), 1, "{metric}, {detail:?}");
            assert_eq!(samples[0]["metric"], metric);
            assert_eq!(samples[0]["value"], expected);
            assert_eq!(export["record_count"], 1);
            assert_eq!(export["capabilities"][metric]["status"], "available");
            assert_eq!(export["capabilities"][metric]["source_records"], 1);
            assert_eq!(export["capabilities"][metric]["rows_in_export"], 1);
        }
    }
}

#[test]
fn daily_metric_types_export_alone_but_stay_outside_workout_scope() {
    let db = Database::in_memory().unwrap();
    let workout = workout_with_type(None, "run", "string_field");
    let date = workout
        .start_time
        .with_timezone(&Local)
        .format("%Y-%m-%d")
        .to_string();
    db.insert_workout(&workout).unwrap();
    let cases = [
        ("respiratory_rate", "respiratory_rate", 16.0, "brpm"),
        ("lactate_threshold", "lactate_threshold_hr", 170.0, "bpm"),
        ("pai", "pai_daily", 20.0, "pai"),
        ("hrv_rmssd", "hrv_rmssd", 42.0, "ms"),
    ];
    for (_, metric, value, unit) in cases {
        db.insert_daily_metric(&DailyMetric {
            date: date.clone(),
            metric: metric.into(),
            value,
            unit: unit.into(),
            source_scope: SourceScope::Device,
            device_id: None,
        })
        .unwrap();
    }

    for detail in [ExportDetail::Summary, ExportDetail::Full] {
        for (selected_type, metric, value, _) in cases {
            let export = parsed_export(&db, &[selected_type], detail);
            let daily = export["data"]["daily_metrics"].as_array().unwrap();
            assert_eq!(daily.len(), 1, "{selected_type}, {detail:?}");
            assert_eq!(daily[0]["metric"], metric);
            assert_eq!(daily[0]["value"], value);
            assert_eq!(export["record_count"], 1);
            let capability = &export["capabilities"][selected_type];
            assert_eq!(capability["status"], "available");
            assert_eq!(capability["source_records"], 1);
            assert_eq!(capability["rows_in_export"], 1);

            let mut selection = export_selection(&[selected_type], detail);
            selection.scope = Some(ExportScope::Workout {
                workout_id: workout.workout_id.clone(),
            });
            let (encoded, records) = db.build_ai_export(&selection).unwrap();
            let scoped: serde_json::Value = serde_json::from_str(&encoded).unwrap();
            assert_eq!(records, 0, "daily {metric} is outside workout scope");
            assert!(scoped["data"]["daily_metrics"]
                .as_array()
                .unwrap()
                .is_empty());
            if matches!(selected_type, "lactate_threshold" | "pai") {
                assert_eq!(
                    scoped["capabilities"][selected_type]["status"],
                    "excluded_by_scope"
                );
            }
        }
    }
}

#[test]
fn workout_export_excludes_weight_samples_that_overlap_the_workout() {
    let db = Database::in_memory().unwrap();
    let workout = workout_with_type(None, "run", "string_field");
    db.insert_workout(&workout).unwrap();
    for (metric, value, unit) in [
        ("weight", 70.0, "kg"),
        ("body_fat_rate", 18.5, "%"),
        ("heart_rate", 120.0, "bpm"),
    ] {
        db.insert_metric_sample(&MetricSample {
            metric: metric.into(),
            timestamp: workout.start_time + chrono::Duration::minutes(5),
            value,
            unit: unit.into(),
            source_scope: SourceScope::Device,
            device_id: None,
        })
        .unwrap();
    }

    for detail in [ExportDetail::Summary, ExportDetail::Full] {
        let date_export = parsed_export(&db, &["weight", "heart_rate"], detail);
        let samples = date_export["data"]["metric_samples"].as_array().unwrap();
        assert_eq!(samples.len(), 3);
        for (metric, value) in [("weight", 70.0), ("body_fat_rate", 18.5)] {
            let sample = samples.iter().find(|row| row["metric"] == metric).unwrap();
            assert_eq!(sample["value"], value);
        }
        assert_eq!(date_export["capabilities"]["weight"]["status"], "available");
        assert_eq!(date_export["capabilities"]["weight"]["source_records"], 2);
        assert_eq!(date_export["capabilities"]["weight"]["rows_in_export"], 2);

        let mut selection = export_selection(&["weight", "heart_rate"], detail);
        selection.scope = Some(ExportScope::Workout {
            workout_id: workout.workout_id.clone(),
        });
        let (encoded, records) = db.build_ai_export(&selection).unwrap();
        let export: serde_json::Value = serde_json::from_str(&encoded).unwrap();
        let samples = export["data"]["metric_samples"].as_array().unwrap();
        assert_eq!(records, 1);
        assert_eq!(samples.len(), 1);
        assert_eq!(samples[0]["metric"], "heart_rate");
        assert_eq!(export["capabilities"]["heart_rate"]["status"], "available");
        assert_eq!(export["capabilities"]["heart_rate"]["source_records"], 1);
        assert_eq!(export["capabilities"]["heart_rate"]["rows_in_export"], 1);
        assert_eq!(
            export["capabilities"]["weight"]["status"],
            "excluded_by_scope"
        );
        assert_eq!(export["capabilities"]["weight"]["rows_in_export"], 0);
    }
}

/// Food data can be exported, and the "daily activity" fallback does not sweep it up.
///
/// `food` was once only listed in the capability table and never in the export's allowed types: `--types food`
/// was dropped silently. Intake data entered the database and could be charted, but none of it could be exported. Worse, the
/// `daily_activity` fallback branch took every unclaimed daily metric, including
/// `intake_*`. Calories eaten and calories burned by activity are opposite things. Mixing them into one type
/// counts what was eaten as what was burned.
#[test]
fn food_is_exportable_and_never_folded_into_daily_activity() {
    let db = Database::in_memory().unwrap();
    for (metric, value, unit) in [
        ("intake_calories", 2100.0, "kcal"),
        ("intake_protein_g", 120.0, "g"),
        ("intake_fat_g", 70.0, "g"),
        ("intake_carbs_g", 210.0, "g"),
    ] {
        db.insert_daily_metric(&DailyMetric {
            date: "2023-11-05".into(),
            metric: metric.into(),
            value,
            unit: unit.into(),
            source_scope: SourceScope::UserFused,
            device_id: None,
        })
        .unwrap();
    }
    // Activity calories on the same day, to prove the two are not mixed.
    db.insert_daily_metric(&DailyMetric {
        date: "2023-11-05".into(),
        metric: "active_calories".into(),
        value: 480.0,
        unit: "kcal".into(),
        source_scope: SourceScope::UserFused,
        device_id: None,
    })
    .unwrap();

    let names = |export: &serde_json::Value| -> Vec<String> {
        export["data"]["daily_metrics"]
            .as_array()
            .map(|rows| {
                rows.iter()
                    .map(|row| row["metric"].as_str().unwrap_or_default().to_string())
                    .collect()
            })
            .unwrap_or_default()
    };

    let food = parsed_export(&db, &["food"], ExportDetail::Summary);
    let exported = names(&food);
    for metric in [
        "intake_calories",
        "intake_protein_g",
        "intake_fat_g",
        "intake_carbs_g",
    ] {
        assert!(
            exported.iter().any(|name| name == metric),
            "--types food did not export {metric}; exported: {exported:?}"
        );
    }
    assert!(
        !exported.iter().any(|name| name == "active_calories"),
        "food was selected but activity calories were exported too"
    );
    assert_eq!(
        food["capabilities"]["food"]["status"], "available",
        "food must be available when intake records exist"
    );

    let activity = names(&parsed_export(
        &db,
        &["daily_activity"],
        ExportDetail::Summary,
    ));
    assert!(
        activity.iter().any(|name| name == "active_calories"),
        "daily activity itself must still export"
    );
    assert!(
        !activity.iter().any(|name| name.starts_with("intake_")),
        "intake was swept into daily_activity by the fallback: {activity:?}"
    );
}

#[test]
fn summary_export_aggregates_heart_rate_and_drops_the_per_second_series() {
    let db = Database::in_memory().unwrap();
    for (offset, value) in [(0, 60.0), (60, 70.0), (120, 80.0), (3600, 100.0)] {
        db.insert_metric_sample(&MetricSample {
            metric: "heart_rate".into(),
            timestamp: ts() + chrono::Duration::seconds(offset),
            value,
            unit: "bpm".into(),
            source_scope: SourceScope::Device,
            device_id: Some("SN-ONE".into()),
        })
        .unwrap();
    }
    db.insert_workout(&Workout {
        workout_id: "1700000000".into(),
        workout_type: "run".into(),
        normalized_type: "run".into(),
        type_source: "string_field".into(),
        user_override: None,
        effective_type: "run".into(),
        custom_label: None,
        start_time: ts(),
        end_time: ts() + chrono::Duration::minutes(10),
        distance_meters: Some(1000.0),
        calories: Some(80),
        avg_hr: Some(140),
        max_hr: Some(160),
        training_load: Some(20.0),
        vo2max: None,
        source_scope: SourceScope::Device,
        device_id: Some("SN-ONE".into()),
        synced_at: None,
        gps_available: false,
        sample_count: 0,
        zepp_source: None,
        zepp_type: None,
        ..Default::default()
    })
    .unwrap();

    let summary = parsed_export(&db, &["heart_rate", "workouts"], ExportDetail::Summary);
    let samples = summary["data"]["metric_samples"].as_array().unwrap();
    // Three samples inside one hour collapse to one row; the fourth starts
    // the next hour.
    assert_eq!(samples.len(), 2);
    let first = &samples[0];
    assert_eq!(first["min"], 60.0);
    assert_eq!(first["max"], 80.0);
    assert_eq!(first["avg"], 70.0);
    assert_eq!(first["samples"], 3);
    assert!(
        first.get("timestamp").is_none(),
        "aggregated rows have hours"
    );

    let workout = &summary["data"]["workouts"][0];
    assert!(
        workout.get("samples").is_none(),
        "summary must not carry the per-second series"
    );
    assert!(workout.get("route").is_none());
    assert!(workout.get("sample_count").is_some());

    let full = parsed_export(&db, &["heart_rate", "workouts"], ExportDetail::Full);
    assert_eq!(full["data"]["metric_samples"].as_array().unwrap().len(), 4);
    assert!(full["data"]["workouts"][0].get("samples").is_some());
    assert!(full["data"]["workouts"][0].get("route").is_some());
}

#[test]
fn wake_count_survives_the_round_trip_and_is_not_awake_minutes() {
    // Ten one-minute wakings and one ten-minute waking are the same
    // duration but not the same night, so `wc` is its own field.
    let db = Database::in_memory().unwrap();
    db.insert_sleep_session(&SleepSession {
        sleep_id: "sleep-wc".into(),
        start_time: ts(),
        end_time: ts() + chrono::Duration::minutes(400),
        score: Some(80),
        duration_minutes: 380,
        deep_minutes: Some(80),
        light_minutes: Some(240),
        rem_minutes: Some(40),
        awake_minutes: Some(20),
        source_scope: SourceScope::Device,
        device_id: None,
        synced_at: None,
        time_in_bed_minutes: None,
        stages: Vec::new(),
        wake_count: Some(4),
    })
    .unwrap();
    assert_eq!(
        db.get_sleep_detail("sleep-wc").unwrap().unwrap().wake_count,
        Some(4)
    );
    let export = parsed_export(&db, &["sleep"], ExportDetail::Summary);
    let session = &export["data"]["sleep_sessions"][0];
    assert_eq!(session["wake_count"], 4);
    assert_eq!(session["awake_minutes"], 20);
}

#[test]
fn export_carries_the_sleep_stage_timeline() {
    let db = Database::in_memory().unwrap();
    let start = ts();
    db.insert_sleep_session(&SleepSession {
        sleep_id: "sleep-export".into(),
        start_time: start,
        end_time: start + chrono::Duration::minutes(400),
        score: Some(80),
        duration_minutes: 380,
        deep_minutes: Some(80),
        light_minutes: Some(240),
        rem_minutes: Some(40),
        awake_minutes: Some(20),
        source_scope: SourceScope::Device,
        device_id: Some("SN-ONE".into()),
        synced_at: None,
        time_in_bed_minutes: None,
        wake_count: None,
        stages: vec![
            SleepStageSlice {
                stage: "light".into(),
                start_time: start,
                end_time: start + chrono::Duration::minutes(30),
                raw_mode: None,
            },
            SleepStageSlice {
                stage: "deep".into(),
                start_time: start + chrono::Duration::minutes(30),
                end_time: start + chrono::Duration::minutes(90),
                raw_mode: None,
            },
        ],
    })
    .unwrap();

    for detail in [ExportDetail::Summary, ExportDetail::Full] {
        let export = parsed_export(&db, &["sleep"], detail);
        let stages = export["data"]["sleep_sessions"][0]["stages"]
            .as_array()
            .unwrap();
        assert_eq!(stages.len(), 2, "{detail:?}");
        assert_eq!(stages[0]["stage"], "light");
        assert_eq!(stages[1]["stage"], "deep");
    }
}

#[test]
fn export_says_why_a_selected_type_is_missing() {
    let db = Database::in_memory().unwrap();
    db.insert_metric_sample(&MetricSample {
        metric: "hrv".into(),
        timestamp: ts(),
        value: 45.0,
        unit: "ms".into(),
        source_scope: SourceScope::Device,
        device_id: Some("SN-ONE".into()),
    })
    .unwrap();

    let export = parsed_export(&db, &["hrv", "spo2", "sleep"], ExportDetail::Summary);
    let capabilities = &export["capabilities"];
    assert_eq!(capabilities["hrv"]["status"], "available");
    assert_eq!(capabilities["hrv"]["source_records"], 1);
    assert_eq!(capabilities["hrv"]["rows_in_export"], 1);
    // Nothing fetched and nothing stored: genuinely empty for this window.
    assert_eq!(capabilities["spo2"]["status"], "empty_in_range");
    assert_eq!(capabilities["sleep"]["status"], "empty_in_range");
}

#[test]
fn the_index_window_never_hides_a_day_inside_the_range() {
    // This timestamp bound only helps the index find rows. It must not change the result.
    // The bound must be at least one day wider than the local date range. Otherwise in some time zones the samples of the first or
    // last day are silently dropped.
    let (lower, upper) = local_day_range_utc_bounds("2026-08-23", "2026-08-29").unwrap();
    assert!(
        lower.as_str() < "2026-08-23T00:00:00",
        "lower bound must be before the first day of the range: {lower}"
    );
    assert!(
        upper.as_str() > "2026-08-30T00:00:00",
        "upper bound must be after the end of the last day of the range: {upper}"
    );
    // The same holds for a single-day range.
    let (lower, upper) = local_day_range_utc_bounds("2026-08-28", "2026-08-28").unwrap();
    assert!(lower.as_str() < "2026-08-28T00:00:00");
    assert!(upper.as_str() > "2026-08-29T00:00:00");
    // An invalid date returns None, so the caller falls back to a query without bounds instead of an empty query.
    assert!(local_day_range_utc_bounds("not-a-date", "2026-08-29").is_none());
}

#[test]
fn workout_export_keeps_samples_after_local_midnight_within_its_window() {
    let db = Database::in_memory().unwrap();
    let start = NaiveDate::from_ymd_opt(2023, 11, 15)
        .unwrap()
        .and_hms_opt(23, 50, 0)
        .unwrap()
        .and_local_timezone(Local)
        .single()
        .unwrap()
        .with_timezone(&Utc);
    let mut workout = workout_with_type(None, "run", "string_field");
    workout.start_time = start;
    workout.end_time = start + chrono::Duration::minutes(20);
    db.insert_workout(&workout).unwrap();
    for (minutes, value) in [(-5, 60.0), (5, 100.0), (15, 110.0), (25, 70.0)] {
        db.insert_metric_sample(&MetricSample {
            metric: "heart_rate".into(),
            timestamp: start + chrono::Duration::minutes(minutes),
            value,
            unit: "bpm".into(),
            source_scope: SourceScope::Device,
            device_id: None,
        })
        .unwrap();
    }

    for detail in [ExportDetail::Summary, ExportDetail::Full] {
        let mut selection = export_selection(&["heart_rate"], detail);
        selection.scope = Some(ExportScope::Workout {
            workout_id: workout.workout_id.clone(),
        });
        let (encoded, _) = db.build_ai_export(&selection).unwrap();
        let export: serde_json::Value = serde_json::from_str(&encoded).unwrap();
        assert_eq!(export["capabilities"]["heart_rate"]["source_records"], 2);
        let samples = export["data"]["metric_samples"].as_array().unwrap();
        if detail.is_full() {
            assert_eq!(samples.len(), 2);
            assert_eq!(samples[0]["value"], 100.0);
            assert_eq!(samples[1]["value"], 110.0);
        } else {
            assert_eq!(
                samples
                    .iter()
                    .map(|sample| sample["samples"].as_u64().unwrap())
                    .sum::<u64>(),
                2
            );
            for sample in samples {
                assert!(sample["min"].as_f64().unwrap() >= 100.0);
                assert!(sample["max"].as_f64().unwrap() <= 110.0);
            }
        }
    }

    let day = start.with_timezone(&Local).format("%Y-%m-%d").to_string();
    let mut selection = export_selection(&["heart_rate"], ExportDetail::Full);
    selection.scope = Some(ExportScope::date_range(&day, &day));
    let (encoded, _) = db.build_ai_export(&selection).unwrap();
    let export: serde_json::Value = serde_json::from_str(&encoded).unwrap();
    let samples = export["data"]["metric_samples"].as_array().unwrap();
    assert_eq!(samples.len(), 2);
    assert_eq!(samples[0]["value"], 60.0);
    assert_eq!(samples[1]["value"], 100.0);
}

#[test]
fn a_single_workout_export_carries_only_that_workout() {
    // When the user taps "give to AI" in workout details, the UI says "export only this workout".
    // An earlier version resolved the scope to "the day of this workout", so the heart rate, sleep and steps of the whole day
    // were sent too. What the UI said and what was sent did not match. This test pins it down.
    let db = Database::in_memory().unwrap();
    let start = ts();
    let end = start + chrono::Duration::minutes(30);
    db.insert_workout(&Workout {
        workout_id: "target-workout".into(),
        workout_type: "run".into(),
        normalized_type: "run".into(),
        type_source: "string_field".into(),
        user_override: None,
        effective_type: "run".into(),
        custom_label: None,
        start_time: start,
        end_time: end,
        distance_meters: Some(5000.0),
        calories: Some(300),
        avg_hr: Some(150),
        max_hr: Some(170),
        training_load: Some(40.0),
        vo2max: None,
        source_scope: SourceScope::Device,
        device_id: Some("SN-ONE".into()),
        synced_at: None,
        gps_available: false,
        sample_count: 0,
        zepp_source: None,
        zepp_type: None,
        ..Default::default()
    })
    .unwrap();
    // Another workout on the same day: a date range includes it, a single-workout scope must not.
    db.insert_workout(&Workout {
        workout_id: "other-workout".into(),
        workout_type: "walk".into(),
        normalized_type: "walk".into(),
        type_source: "string_field".into(),
        user_override: None,
        effective_type: "walk".into(),
        custom_label: None,
        start_time: end + chrono::Duration::hours(2),
        end_time: end + chrono::Duration::hours(3),
        distance_meters: Some(2000.0),
        calories: Some(90),
        avg_hr: Some(100),
        max_hr: Some(110),
        training_load: None,
        vo2max: None,
        source_scope: SourceScope::Device,
        device_id: Some("SN-ONE".into()),
        synced_at: None,
        gps_available: false,
        sample_count: 0,
        zepp_source: None,
        zepp_type: None,
        ..Default::default()
    })
    .unwrap();
    for (moment, value) in [
        (start + chrono::Duration::minutes(5), 152.0),
        (end + chrono::Duration::hours(4), 61.0),
    ] {
        db.insert_metric_sample(&MetricSample {
            metric: "heart_rate".into(),
            timestamp: moment,
            value,
            unit: "bpm".into(),
            source_scope: SourceScope::Device,
            device_id: Some("SN-ONE".into()),
        })
        .unwrap();
    }
    db.insert_daily_metric(&DailyMetric {
        date: start.with_timezone(&Local).format("%Y-%m-%d").to_string(),
        metric: "steps".into(),
        value: 9000.0,
        unit: "steps".into(),
        source_scope: SourceScope::Device,
        device_id: Some("SN-ONE".into()),
    })
    .unwrap();
    db.insert_sleep_session(&SleepSession {
        sleep_id: "sleep-1".into(),
        start_time: start - chrono::Duration::hours(8),
        end_time: start - chrono::Duration::hours(1),
        score: Some(80),
        duration_minutes: 420,
        deep_minutes: Some(90),
        light_minutes: Some(280),
        rem_minutes: Some(50),
        awake_minutes: Some(10),
        source_scope: SourceScope::Device,
        device_id: Some("SN-ONE".into()),
        synced_at: None,
        time_in_bed_minutes: None,
        stages: Vec::new(),
        wake_count: Some(1),
    })
    .unwrap();

    let selection = ExportSelection {
        scope: Some(ExportScope::Workout {
            workout_id: "target-workout".into(),
        }),
        start_date: None,
        end_date: None,
        data_types: ["workouts", "heart_rate", "steps", "sleep"]
            .iter()
            .map(|value| value.to_string())
            .collect(),
        detail: ExportDetail::Summary,
    };
    let (encoded, _) = db.build_ai_export(&selection).unwrap();
    let export: serde_json::Value = serde_json::from_str(&encoded).unwrap();

    // Only this one workout.
    let workouts = export["data"]["workouts"].as_array().unwrap();
    assert_eq!(workouts.len(), 1);
    assert_eq!(workouts[0]["workout_id"], "target-workout");

    // Per-point heart rate only includes samples during the workout. The one four hours later is not in it.
    let samples = export["data"]["metric_samples"].as_array().unwrap();
    assert_eq!(samples.len(), 1, "heart rate outside the workout must not be included");

    // Daily data is excluded as a block, and the export says it is out of scope, not "no data in this period".
    assert!(export["data"]["daily_metrics"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(export["data"]["sleep_sessions"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(
        export["capabilities"]["steps"]["status"],
        "excluded_by_scope"
    );
    assert_eq!(
        export["capabilities"]["sleep"]["status"],
        "excluded_by_scope"
    );

    // A reader must be able to check that the scope itself points to this exact workout.
    assert_eq!(export["scope"]["kind"], "workout");
    assert_eq!(export["scope"]["workout_id"], "target-workout");
}

/// H1 regression guard: the sample query must use uq_metric_sample_key, not a full table scan.
/// If `metric IN` is ever lost, this fails at once. Both scope forms are checked.
#[test]
fn ai_export_samples_where_tail_seeks_the_metric_timestamp_index() {
    let db = Database::in_memory().unwrap();
    for single_workout in [false, true] {
        let tail = ai_export_samples_where_tail(3, single_workout);
        let mut stmt = db
            .conn
            .prepare(&format!(
                "EXPLAIN QUERY PLAN SELECT metric FROM metric_samples WHERE {tail}"
            ))
            .unwrap();
        // EXPLAIN also needs all parameters bound (3+2 for the workout form, 3+4 for the date range
        // form). The values do not affect the plan shape.
        let dummies = vec!["x"; if single_workout { 5 } else { 7 }];
        let plan = stmt
            .query_map(rusqlite::params_from_iter(dummies.iter()), |row| {
                row.get::<_, String>(3)
            })
            .unwrap()
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap()
            .join("\n");
        assert!(
            plan.contains("SEARCH metric_samples"),
            "single_workout={single_workout} must use the index:\n{plan}"
        );
        assert!(
            plan.contains("uq_metric_sample_key"),
            "single_workout={single_workout} must use the uq leading column:\n{plan}"
        );
        assert!(
            !plan.contains("SCAN metric_samples"),
            "single_workout={single_workout} must not do a full table scan:\n{plan}"
        );
    }
}

/// Selected types -> metric names in the database: this is the reverse expansion that makes `metric IN` work.
/// If the expansion is wrong, the export either gives too little data or sweeps in metrics that were not selected.
#[test]
fn export_sample_metric_names_maps_selected_types_to_stored_metrics() {
    let db = Database::in_memory().unwrap();
    for metric in [
        "heart_rate",
        "spo2",
        "spo2_apnea_low",
        "weight",
        "bmi",
        "unrelated_reading",
    ] {
        db.insert_metric_sample(&MetricSample {
            metric: metric.into(),
            timestamp: ts(),
            value: 1.0,
            unit: "u".into(),
            source_scope: SourceScope::Device,
            device_id: None,
        })
        .unwrap();
    }
    let selected: BTreeSet<String> = ["spo2", "weight"]
        .iter()
        .map(|value| value.to_string())
        .collect();
    // The spo2 family + all body composition metrics. ORDER BY metric keeps the order stable.
    let names = db.export_sample_metric_names(&selected, false).unwrap();
    assert_eq!(names, vec!["bmi", "spo2", "spo2_apnea_low", "weight"]);
    // Single-workout scope: weight is a daily type and is excluded, spo2 is not.
    let names = db.export_sample_metric_names(&selected, true).unwrap();
    assert_eq!(names, vec!["spo2", "spo2_apnea_low"]);
    // A selected type with no matching metric in the database -> empty list -> the main query is skipped.
    let selected: BTreeSet<String> = ["stress"].iter().map(|v| v.to_string()).collect();
    assert!(db
        .export_sample_metric_names(&selected, false)
        .unwrap()
        .is_empty());
}

/// End to end: the export reads only the metrics that the selected types map to. No row from an unselected type gets in.
#[test]
fn ai_export_reads_only_the_metrics_the_selection_maps_to() {
    let db = Database::in_memory().unwrap();
    for (metric, value) in [("heart_rate", 60.0), ("spo2", 97.0), ("unrelated", 1.0)] {
        db.insert_metric_sample(&MetricSample {
            metric: metric.into(),
            timestamp: ts(),
            value,
            unit: "u".into(),
            source_scope: SourceScope::Device,
            device_id: None,
        })
        .unwrap();
    }
    let export = parsed_export(&db, &["heart_rate"], ExportDetail::Full);
    let samples = export["data"]["metric_samples"].as_array().unwrap();
    assert_eq!(samples.len(), 1);
    assert_eq!(samples[0]["metric"], "heart_rate");
    assert_eq!(export["record_count"], 1);
}

/// Resting calories (bmr) are included when only daily activity is selected. When both are selected, they stay under weight / body composition and are not exported twice.
#[test]
fn resting_energy_rides_along_with_daily_activity() {
    use crate::storage::ai_export::export_sample_matched_type;
    use std::collections::BTreeSet;

    let only_activity: BTreeSet<String> = ["daily_activity".to_string()].into();
    assert_eq!(
        export_sample_matched_type("bmr", &only_activity).as_deref(),
        Some("daily_activity")
    );
    // Other body composition metrics do not follow into daily activity.
    assert_eq!(
        export_sample_matched_type("body_fat_rate", &only_activity),
        None
    );
    let both: BTreeSet<String> = ["daily_activity".to_string(), "weight".to_string()].into();
    assert_eq!(
        export_sample_matched_type("bmr", &both).as_deref(),
        Some("weight")
    );
}
