use super::*;

#[test]
fn aliases_skip_missing_values_without_inventing_ids_or_losing_samples() {
    let raw = serde_json::json!({"items":[{"timestamp":null,"time":1800000000,"value":" ","heartRate":72,"device_id":null,"deviceId":"D85403FFFEE4D576"}]});
    let samples = Normalizer::normalize_heart_rate(&raw).unwrap();
    assert_eq!(samples.len(), 1);
    assert_eq!(samples[0].value, 72.0);
    assert_eq!(samples[0].device_id.as_deref(), Some("D85403FFFEE4D576"));
    let raw = serde_json::json!({"data":[{"workout_id":" ","workoutId":{},"trackid":"original-id","start_time":null,"startTime":1800000000,"end_time":1800003600,"type":null,"sport_mode":6}]});
    let workouts = Normalizer::normalize_workouts_with_sport(&raw, None).unwrap();
    assert_eq!(workouts[0].workout_id, "original-id");
    let outer = serde_json::json!({"value":null,"score":"bad","zero":0,"flag":false});
    let nested = serde_json::json!({"value":42});
    assert_eq!(
        first_number_from(
            outer.as_object().unwrap(),
            nested.as_object(),
            &["value", "score"]
        ),
        Some(42.0)
    );
    assert_eq!(
        first_value(outer.as_object().unwrap(), &["zero", "value"]),
        Some(&serde_json::json!(0))
    );
    assert_eq!(
        first_value(outer.as_object().unwrap(), &["flag", "value"]),
        Some(&serde_json::json!(false))
    );
}

/// A "not measured" sentinel must not become 0.
///
/// The cloud uses -1 for a missing item (`avg_cadence`, `average_power`). For a ride,
/// `total_step` is 0 and `avg_frequency` is "0.0". None of these may become a value.
#[test]
fn sentinels_do_not_become_zeroes() {
    let raw = serde_json::json!({
        "data": [{
            "trackid": "1787186615",
            "type": 9,
            "start_time": 1787186615_i64,
            "end_time": 1787187642_i64,
            "dis": 1803.0,
            "min_heart_rate": 94,
            "total_step": 0,
            "avg_frequency": "0.0",
            "max_frequency": 0,
            "avg_stride_length": 0,
            "average_power": -1.0,
            "avg_cadence": -1,
            "rpe": 2,
            "heart_range": "115,113;462,141;323,154;102,162;0,173;0,190"
        }]
    });

    let records = Normalizer::normalize_workouts_with_sport(&raw, None).expect("should parse");
    let workout = records.first().expect("should have one workout");

    assert_eq!(workout.min_hr, Some(94));
    assert_eq!(
        workout.total_steps, None,
        "0 steps on a ride means \"no step count\"; do not record it as 0 steps walked"
    );
    assert_eq!(workout.avg_cadence_spm, None);
    assert_eq!(workout.max_cadence_spm, None);
    assert_eq!(workout.avg_stride_cm, None);
    assert_eq!(workout.training_effect, None, "without te there must be no value");
    assert_eq!(workout.rpe, Some(2));
    // The ride really has heart rate zones
    assert_eq!(workout.hr_zones.len(), 6);
    assert_eq!(workout.hr_zones[3].seconds, 102);
}

/// An empty payload reports `DataUnavailable`, not a parse failure.
///
/// Some accounts return `{"items": []}` from the heart rate endpoint for the whole history. That clearly answers
/// "no heart rate in this period". Backfill uses `is_unavailable()` to record this chunk as
/// "nothing in the cloud", not a failure. This assertion is the basis of that check; do not change it to another error type.
#[test]
fn an_empty_items_payload_reports_unavailable_not_a_parse_failure() {
    let payload = serde_json::json!({ "items": [] });
    let error = Normalizer::normalize_heart_rate(&payload)
        .expect_err("empty items currently reports DataUnavailable");
    assert!(
        error.is_unavailable(),
        "backfill uses this to tell \"nothing in the cloud\" from \"we did not understand\"; if you change it, also change backfill_one_chunk"
    );
}

#[test]
fn empty_or_wrong_shape_is_not_success() {
    assert!(Normalizer::normalize_heart_rate(&json!({"items": []})).is_err());
    assert!(Normalizer::normalize_band_data(&json!({"data": "H4sI..."}))
        .map_or(true, |band| band.sleep_sessions.is_empty()));
}

/// When a flat sleep payload has no stage fields, deep/light/awake/rem must be None,
/// not 0. The duration must not be "whole span minus an invented 0 minutes awake" either.
#[test]
fn a_flat_sleep_record_without_stages_does_not_invent_zeros() {
    let sessions = Normalizer::normalize_band_data(&json!({
        "items": [{
            "sleep_id": "flat-no-stages",
            "start_time": 1_700_000_000i64,
            "end_time": 1_700_028_800i64,
            "score": 80
        }]
    }))
    .unwrap()
    .sleep_sessions;
    assert_eq!(sessions.len(), 1);
    let sleep = &sessions[0];
    assert_eq!(sleep.deep_minutes, None);
    assert_eq!(sleep.light_minutes, None);
    assert_eq!(sleep.rem_minutes, None);
    assert_eq!(sleep.awake_minutes, None);
    assert_eq!(sleep.duration_minutes, 480);
}

#[test]
fn bookkeeping_device_ids_are_not_devices() {
    // These are the placeholder shapes Zepp actually sends. Trimming
    // punctuation off "1," used to yield a device id of "1", which then
    // labelled account-level aggregates as if a watch had reported them.
    for placeholder in ["1,", "1", "1,-1", "1440,app", ""] {
        let payload = json!({ "deviceId": placeholder });
        assert_eq!(
            device_id(payload.as_object().unwrap()),
            None,
            "{placeholder} should not pass as a device id"
        );
    }
    let real = json!({"sourceDeviceId": "23229501001311"});
    assert_eq!(
        device_id(real.as_object().unwrap()).as_deref(),
        Some("23229501001311")
    );
    // A real id joined onto an index is recovered, not discarded.
    let joined = json!({"deviceId": "3,D85403FFFEE4D576"});
    assert_eq!(
        device_id(joined.as_object().unwrap()).as_deref(),
        Some("D85403FFFEE4D576")
    );
}

#[test]
fn account_level_events_are_user_fused_not_device() {
    // DailyHealth/Charge carry a bookkeeping deviceId that `device_id`
    // rejects; without the event-type rule they would fall to `Unknown`
    // and the export would lose the fact that these are fused totals.
    for event_type in ["DailyHealth", "Charge"] {
        let event = json!({ "eventType": event_type, "deviceId": "1," });
        assert_eq!(
            source_scope(event.as_object(), None),
            SourceScope::UserFused,
            "{event_type}"
        );
    }
    let device_event = json!({ "eventType": "readiness" });
    assert_eq!(
        source_scope(device_event.as_object(), Some("D8803CFFFEC19AC6")),
        SourceScope::Device
    );
}

#[test]
fn parse_timestamp_handles_compact_calendar_days() {
    // yyyyMMdd integers are calendar days, never epoch seconds.
    let date = parse_timestamp(&json!(20260812)).unwrap();
    assert_eq!(date.format("%Y-%m-%d").to_string(), "2026-08-12");
    // Epoch seconds keep working (2024-01-01T00:00:00Z).
    let epoch = parse_timestamp(&json!(1704067200)).unwrap();
    assert_eq!(epoch.format("%Y-%m-%d").to_string(), "2024-01-01");
    // Epoch milliseconds keep working.
    let millis = parse_timestamp(&json!(1704067200000i64)).unwrap();
    assert_eq!(millis.format("%Y-%m-%d").to_string(), "2024-01-01");
}

#[test]
fn missing_rem_is_not_invented_from_in_bed_subtraction() {
    let summary = json!({
        "tz": 28800,
        "slp": {
            "st": 1_700_000_000i64,
            "ed": 1_700_021_600i64,
            "ss": 70,
            "stage": [
                {"mode": 5, "start": 0, "stop": 60},
                {"mode": 4, "start": 61, "stop": 200}
            ]
        }
    });
    let result = Normalizer::normalize_band_data(&json!({
        "data": [{
            "uuid": "sleep-no-rem",
            "date_time": "2026-08-12",
            "summary": STANDARD.encode(serde_json::to_vec(&summary).unwrap())
        }]
    }))
    .unwrap();
    assert_eq!(result.sleep_sessions[0].rem_minutes, None);
    // No awake segment and no `wk`: awake is unknown, not 0. Asleep is deep 61 + light 140;
    // the 360 minutes in bed are not used instead.
    assert_eq!(result.sleep_sessions[0].awake_minutes, None);
    assert_eq!(result.sleep_sessions[0].duration_minutes, 61 + 140);
    assert_eq!(result.sleep_sessions[0].time_in_bed_minutes, None);
    assert_eq!(result.sleep_sessions[0].stages.len(), 2);
    assert_eq!(result.sleep_sessions[0].stages[0].stage, "deep");
    assert_eq!(result.sleep_sessions[0].stages[1].stage, "light");
}

#[test]
fn ebt_obt_are_not_treated_as_time_in_bed() {
    let summary = json!({
        "tz": 28800,
        "slp": {
            "st": 1_700_000_000i64,
            "ed": 1_700_021_600i64,
            "ss": 70,
            "ebt": 452,
            "obt": -31,
            "wk": 10,
            "dp": 80,
            "lt": 200
        }
    });
    let result = Normalizer::normalize_band_data(&json!({
        "data": [{
            "uuid": "sleep-ebt",
            "date_time": "2026-08-12",
            "device_id": "SN123",
            "summary": STANDARD.encode(serde_json::to_vec(&summary).unwrap())
        }]
    }))
    .unwrap();
    assert_eq!(result.sleep_sessions[0].time_in_bed_minutes, None);
    assert!(result.sleep_sessions[0].stages.is_empty());
}

/// Backend heart rate normalization currently knows only "fixed offsets". This case pins that contract
/// for the three time zones the user asked for.
///
/// `heart_rate_from_band_item` gets no source IANA time zone: the cloud summary gives only that day's
/// fixed `tz` seconds relative to UTC, and `data_hr` is a minute series from local midnight. So sample m
/// falls at `local midnight - tz + m minutes`. The table below asserts only this fixed-offset arithmetic.
/// It does not infer which offset the device/cloud used on DST switch days: London and New_York spring
/// and autumn switch days are stored with the offsets before and after the switch, because the raw fields cannot prove which side applies.
/// To give switch days DST semantics, first get the real `tz` from Zepp raw payloads of those days.
#[test]
fn heart_rate_from_band_item_uses_the_summary_fixed_utc_offset() {
    struct Case {
        name: &'static str,
        date: &'static str,
        tz: i64,
        // The UTC instant of local minute 0 at a fixed offset.
        utc_midnight: &'static str,
    }

    let cases = [
        // Europe/London: GMT in winter, BST(+3600) in summer.
        Case {
            name: "London winter GMT",
            date: "2026-01-15",
            tz: 0,
            utc_midnight: "2026-01-15T00:00:00Z",
        },
        Case {
            name: "London summer BST",
            date: "2026-07-15",
            tz: 3600,
            utc_midnight: "2026-07-14T23:00:00Z",
        },
        // America/New_York: EST(-18000) in winter, EDT(-14400) in summer.
        Case {
            name: "New York winter EST",
            date: "2026-01-15",
            tz: -18_000,
            utc_midnight: "2026-01-15T05:00:00Z",
        },
        Case {
            name: "New York summer EDT",
            date: "2026-07-15",
            tz: -14_400,
            utc_midnight: "2026-07-15T04:00:00Z",
        },
        // Asia/Shanghai (Beijing): CST(+28800), no DST.
        Case {
            name: "Beijing CST",
            date: "2026-07-15",
            tz: 28_800,
            utc_midnight: "2026-07-14T16:00:00Z",
        },
        // London spring switch day (2026-03-29) and autumn switch day (2026-10-25): offsets before/after the switch.
        Case {
            name: "London switch 2026-03-29 before GMT",
            date: "2026-03-29",
            tz: 0,
            utc_midnight: "2026-03-29T00:00:00Z",
        },
        Case {
            name: "London switch 2026-03-29 after BST",
            date: "2026-03-29",
            tz: 3600,
            utc_midnight: "2026-03-28T23:00:00Z",
        },
        Case {
            name: "London switch 2026-10-25 before BST",
            date: "2026-10-25",
            tz: 3600,
            utc_midnight: "2026-10-24T23:00:00Z",
        },
        Case {
            name: "London switch 2026-10-25 after GMT",
            date: "2026-10-25",
            tz: 0,
            utc_midnight: "2026-10-25T00:00:00Z",
        },
        // New York spring switch day (2026-03-08) and autumn switch day (2026-11-01): offsets before/after the switch.
        Case {
            name: "New York switch 2026-03-08 before EST",
            date: "2026-03-08",
            tz: -18_000,
            utc_midnight: "2026-03-08T05:00:00Z",
        },
        Case {
            name: "New York switch 2026-03-08 after EDT",
            date: "2026-03-08",
            tz: -14_400,
            utc_midnight: "2026-03-08T04:00:00Z",
        },
        Case {
            name: "New York switch 2026-11-01 before EDT",
            date: "2026-11-01",
            tz: -14_400,
            utc_midnight: "2026-11-01T04:00:00Z",
        },
        Case {
            name: "New York switch 2026-11-01 after EST",
            date: "2026-11-01",
            tz: -18_000,
            utc_midnight: "2026-11-01T05:00:00Z",
        },
    ];

    // One 1440-byte data_hr per local day: minutes 0/1/1439 each hold a value in 20..=240.
    // The rest are 0 and are filtered out in normalization. This checks minute index and value.
    let mut bytes = vec![0_u8; 1440];
    bytes[0] = 60;
    bytes[1] = 61;
    bytes[1439] = 62;
    let expected = [(0_i64, 60.0_f64), (1, 61.0), (1439, 62.0)];

    for case in cases {
        let summary = json!({ "tz": case.tz });
        let result = Normalizer::normalize_band_data(&json!({
            "data": [{
                "uuid": "hr-tz",
                "date_time": case.date,
                "device_id": "SN-HR",
                "data_hr": STANDARD.encode(&bytes),
                "summary": STANDARD.encode(serde_json::to_vec(&summary).unwrap())
            }]
        }))
        .unwrap();

        let midnight = DateTime::parse_from_rfc3339(case.utc_midnight)
            .unwrap()
            .with_timezone(&Utc);
        let samples = &result.heart_rate_samples;
        assert_eq!(samples.len(), 3, "{}: only 3 samples should remain", case.name);
        for (sample, (minute, value)) in samples.iter().zip(expected) {
            assert_eq!(sample.metric, "heart_rate", "{}", case.name);
            assert_eq!(sample.unit, "bpm", "{}", case.name);
            assert_eq!(sample.value, value, "{}: value at minute {minute}", case.name);
            assert_eq!(
                sample.timestamp,
                midnight + Duration::minutes(minute),
                "{}: minute {minute} should be minute {minute} from {}",
                case.name,
                case.utc_midnight
            );
        }
    }
}

#[test]
fn night_sleep_stages_anchor_to_previous_day_midnight() {
    // Real payload shape: date_time is the wake-up day, and stage minutes count from local midnight of the night
    // before (>= 1440 past midnight). Anchoring to that night shifts everything by +24h and the stage bar renders empty.
    let summary = json!({
        "tz": 28800,
        "slp": {
            "st": 1_786_897_200i64,   // 2026-08-17 00:20 +08
            "ed": 1_786_930_620i64,   // 2026-08-17 09:37 +08
            "ss": 80,
            "stage": [
                {"mode": 4, "start": 1460, "stop": 1471},
                {"mode": 5, "start": 1472, "stop": 1484},
                {"mode": 11, "start": 1485, "stop": 1492}
            ]
        }
    });
    let result = Normalizer::normalize_band_data(&json!({
        "data": [{
            "uuid": "sleep-night",
            "date_time": "2026-08-17",
            "summary": STANDARD.encode(serde_json::to_vec(&summary).unwrap())
        }]
    }))
    .unwrap();
    let session = &result.sleep_sessions[0];
    assert_eq!(session.stages.len(), 3);
    assert_eq!(session.stages[0].start_time, session.start_time);
    // New firmware REM code mode=11 must also be recognized
    assert_eq!(session.stages[2].stage, "rem");
    assert_eq!(session.rem_minutes, Some(8));
}

/// A huge stage minute count must not crash decoding; that segment is skipped.
#[test]
fn huge_sleep_stage_minutes_are_skipped_not_panicked() {
    let summary = json!({
        "tz": 28800,
        "slp": {
            "st": 1_786_897_200i64,
            "ed": 1_786_930_620i64,
            "ss": 80,
            "stage": [
                {"mode": 4, "start": 1e20, "stop": 1e20},
                {"mode": 5, "start": 1460, "stop": 1471}
            ]
        }
    });
    let result = Normalizer::normalize_band_data(&json!({
        "data": [{
            "uuid": "sleep-overflow",
            "date_time": "2026-08-17",
            "summary": STANDARD.encode(serde_json::to_vec(&summary).unwrap())
        }]
    }))
    .unwrap();
    let session = &result.sleep_sessions[0];
    assert_eq!(session.stages.len(), 1);
    assert_eq!(session.stages[0].stage, "deep");
}

/// A huge millisecond offset on an HRV sample must not panic; that sample is skipped.
#[test]
fn huge_hrv_sample_offset_is_skipped_not_panicked() {
    let raw = json!({
        "items": [{
            "value": {
                "startTime": 1_700_000_000i64,
                "samples": [
                    {"s": 1e20, "sdnn": 40.0},
                    {"offset": i64::MAX, "sdnn": 41.0},
                    {"s": 1000, "sdnn": 42.0}
                ]
            }
        }]
    });
    let batch = Normalizer::normalize_hrv_with_diagnostics(&raw).unwrap();
    assert_eq!(batch.records.len(), 1);
    assert_eq!(batch.records[0].value, 42.0);
    assert!(
        batch.diagnostics.iter().any(|line| line.contains("HRV")),
        "an out-of-range sample should add a diagnostic, not silently drop everything: {:?}",
        batch.diagnostics
    );
}

#[test]
fn generated_time_heart_rate_data_reads_the_observed_single_byte_shape() {
    // The second shape seen on a real account on 2026-10-06 (device ID replaced):
    // `generatedTime` is Unix seconds; `heartRateData` single byte [0x5E] = 94 bpm.
    let raw = serde_json::json!({ "items": [ {
        "deviceId": "D85403FFFEE4D576",
        "deviceSource": 10551552,
        "generatedTime": 1791095423_i64,
        "heartRateData": "Xg==",
        "timeZone": "8",
        "type": 2
    } ] });
    let samples = Normalizer::normalize_heart_rate(&raw).unwrap();
    assert_eq!(samples.len(), 1);
    assert_eq!(samples[0].value, 94.0);
    assert_eq!(samples[0].unit, "bpm");
    assert_eq!(
        samples[0].timestamp,
        DateTime::from_timestamp(1791095423, 0).unwrap(),
        "generatedTime is Unix seconds; read as milliseconds it lands in 1970"
    );
    assert_eq!(samples[0].device_id.as_deref(), Some("D85403FFFEE4D576"));
}

#[test]
fn multi_byte_heart_rate_data_is_reported_not_guessed() {
    let raw = serde_json::json!({ "items": [ {
        "generatedTime": 1791095423_i64,
        "heartRateData": "XgJA"
    } ] });
    let error = Normalizer::normalize_heart_rate(&raw)
        .expect_err("only single-byte readings are verified; multiple bytes must not parse as data");
    assert!(
        error.to_string().contains("only single-byte readings are verified"),
        "got {error}"
    );
}

#[test]
fn invalid_base64_heart_rate_data_is_reported() {
    let raw = serde_json::json!({ "items": [ {
        "generatedTime": 1791095423_i64,
        "heartRateData": "***"
    } ] });
    let error = Normalizer::normalize_heart_rate(&raw).expect_err("invalid base64 must not count as data");
    assert!(
        error.to_string().contains("is not valid base64"),
        "got {error}"
    );
}

#[test]
fn packed_heart_rate_data_still_rejects_the_zero_sentinel() {
    let raw = serde_json::json!({ "items": [ {
        "generatedTime": 1791095423_i64,
        "heartRateData": "AA=="
    } ] });
    let error = Normalizer::normalize_heart_rate(&raw).expect_err("0 is a sentinel, not a reading");
    assert!(error.to_string().contains("invalid value"), "got {error}");
}

#[test]
fn heart_rate_zero_is_a_sentinel_not_a_reading() {
    let batch = Normalizer::normalize_heart_rate_with_diagnostics(&json!({
        "items": [
            {"timestamp": 1_800_000_000i64, "value": 0},
            {"timestamp": 1_800_000_060i64, "value": 72}
        ]
    }))
    .unwrap();
    assert_eq!(batch.records.len(), 1);
    assert_eq!(batch.records[0].value, 72.0);
    assert!(
        batch
            .diagnostics
            .iter()
            .any(|line| line.contains("heart rate")),
        "0 bpm should add a diagnostic: {:?}",
        batch.diagnostics
    );
}

#[test]
fn sleep_stages_without_tz_are_not_staged_as_utc() {
    let summary = json!({
        "slp": {
            "st": 1_786_897_200i64,
            "ed": 1_786_930_620i64,
            "ss": 80,
            "stage": [
                {"mode": 4, "start": 1460, "stop": 1471},
                {"mode": 5, "start": 1472, "stop": 1484}
            ]
        }
    });
    let result = Normalizer::normalize_band_data(&json!({
        "data": [{
            "uuid": "sleep-no-tz",
            "date_time": "2026-08-17",
            "summary": STANDARD.encode(serde_json::to_vec(&summary).unwrap())
        }]
    }))
    .unwrap();
    assert_eq!(result.sleep_sessions.len(), 1);
    assert!(
        result.sleep_sessions[0].stages.is_empty(),
        "a missing tz must not time stages from UTC midnight"
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|line| line.contains("missing tz")),
        "a missing tz needs a diagnostic: {:?}",
        result.diagnostics
    );
    let with_explicit_utc = json!({
        "tz": 0,
        "slp": {
            "st": 1_786_897_200i64,
            "ed": 1_786_930_620i64,
            "ss": 80,
            "stage": [
                {"mode": 4, "start": 1460, "stop": 1471}
            ]
        }
    });
    let utc = Normalizer::normalize_band_data(&json!({
        "data": [{
            "uuid": "sleep-utc-tz",
            "date_time": "2026-08-17",
            "summary": STANDARD.encode(serde_json::to_vec(&with_explicit_utc).unwrap())
        }]
    }))
    .unwrap();
    assert_eq!(
        utc.sleep_sessions[0].stages.len(),
        1,
        "only a payload that says tz=0 is UTC"
    );
}

#[test]
fn date_only_daily_summary_is_not_overwritten_by_epoch_fallback() {
    let raw = json!({
        "items": [
            {
                "dateString": "2026-08-12",
                "totalSteps": 9999
            },
            {
                "timestamp": 1_786_492_800_000i64,
                "totalSteps": 1
            }
        ]
    });
    let rows = Normalizer::normalize_daily_summary(&raw).unwrap();
    let steps = rows
        .iter()
        .filter(|row| row.metric == "steps" && row.date == "2026-08-12")
        .map(|row| row.value)
        .collect::<Vec<_>>();
    assert_eq!(
        steps,
        vec![9999.0],
        "date-only must not lose to the epoch fallback: {rows:?}"
    );
}

#[test]
fn epoch_day_uses_payload_timezone_when_present() {
    assert_eq!(timezone_offset_seconds(&json!("GMT+08:00")), Some(8 * 3600));
    assert_eq!(
        timezone_offset_seconds(&json!("Asia/Shanghai")),
        Some(8 * 3600)
    );
    assert_eq!(
        timezone_offset_seconds(&json!("1,Asia/Shanghai")),
        Some(8 * 3600)
    );
    assert_eq!(timezone_offset_seconds(&json!("28800000")), Some(8 * 3600));
    assert_eq!(timezone_offset_seconds(&json!("32")), None);
    assert_eq!(timezone_offset_seconds(&json!(0)), Some(0));

    // 2026-08-12 00:00 UTC = 1786492800000; minus 8 hours = midnight of that day in Shanghai.
    let shanghai_midnight_utc_ms = 1_786_492_800_000i64 - 8 * 3_600_000;
    let shanghai_midnight = json!({
        "items": [{
            "timestamp": shanghai_midnight_utc_ms,
            "timeZone": "GMT+08:00",
            "totalSteps": 321
        }]
    });
    let rows = Normalizer::normalize_daily_summary(&shanghai_midnight).unwrap();
    let steps = rows.iter().find(|row| row.metric == "steps").unwrap();
    assert_eq!(steps.date, "2026-08-12");
    assert_eq!(steps.value, 321.0);

    let utc_only = json!({
        "items": [{
            "timestamp": shanghai_midnight_utc_ms,
            "totalSteps": 321
        }]
    });
    let rows = Normalizer::normalize_daily_summary(&utc_only).unwrap();
    let steps = rows.iter().find(|row| row.metric == "steps").unwrap();
    assert_eq!(
        steps.date, "2026-08-11",
        "a missing time zone keeps the old UTC day split; do not invent a zone"
    );
}

#[test]
fn epoch_day_follows_dst_zones_by_name() {
    // 2026-07-01 22:30 UTC = Berlin DST Jul 2 00:30; in winter the same instant is 23:30 (Jul 1).
    // Before, a zone name with DST had no fixed offset, so the whole batch fell back to the UTC day (Jul 1).
    let summer = json!({"timestamp": 1_782_945_000i64, "timeZone": "Europe/Berlin"});
    let object = summer.as_object().unwrap();
    assert_eq!(summary_date(object, None).as_deref(), Some("2026-07-02"));
    let winter = json!({"timestamp": 1_798_756_200i64, "timeZone": "1,Europe/Berlin"});
    // 2026-12-31 22:30 UTC = Berlin 23:30, still Dec 31.
    assert_eq!(
        summary_date(winter.as_object().unwrap(), None).as_deref(),
        Some("2026-12-31")
    );
    // A date string is not affected by the time zone field.
    let dated = json!({"date": "2026-07-01", "timeZone": "Europe/Berlin"});
    assert_eq!(
        summary_date(dated.as_object().unwrap(), None).as_deref(),
        Some("2026-07-01")
    );
}

/// During DST, if the summary `tz` is the standard offset, add the missing hour from the device time zone. If `tz` already includes DST,
/// the zone has no DST, or the device time zone is unknown, nothing changes (2026-10 "4 o'clock shows as 5 o'clock").
#[test]
fn band_offset_adds_the_missing_daylight_hour_only_when_tz_is_the_standard_offset() {
    use crate::normalizer::band_items::effective_band_offset;
    let day = |text: &str| NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap();
    let lisbon = Some("Europe/Lisbon");
    assert_eq!(effective_band_offset(0, day("2026-08-01"), lisbon), 3600);
    assert_eq!(effective_band_offset(3600, day("2026-08-01"), lisbon), 3600);
    assert_eq!(effective_band_offset(0, day("2026-01-15"), lisbon), 0);
    assert_eq!(effective_band_offset(0, day("2026-08-01"), None), 0);
    assert_eq!(
        effective_band_offset(0, day("2026-08-01"), Some("Not/AZone")),
        0
    );
    assert_eq!(
        effective_band_offset(28_800, day("2026-08-01"), Some("Asia/Shanghai")),
        28_800
    );
    // Southern hemisphere: January is DST; the standard offset is the smaller of the two.
    assert_eq!(
        effective_band_offset(36_000, day("2026-01-15"), Some("Australia/Sydney")),
        39_600
    );
    assert_eq!(
        effective_band_offset(36_000, day("2026-07-15"), Some("Australia/Sydney")),
        36_000
    );
}
