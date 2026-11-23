use super::*;

/// `all_day_stress` carries a full-day curve with one point per five minutes each day. Before, the whole curve was dropped.
///
/// The values are from the 2026-09-02 day of a real payload (synced only up to 01:00, so exactly
/// 13 points, which fit whole in a test). Only `userId` was removed.
/// A real `watch_score` payload, copied field by field from the 2026-08-12 entry in the local database.
fn readiness_item() -> Value {
    json!({
        "eventType": "readiness",
        "subType": "watch_score",
        "timestamp": 1786493641000i64,
        "value": {
            "afibBaseLine": 0, "afibInsight": 18, "afibScore": 255,
            "ahiBaseline": 0.3827273, "ahiInsight": 100, "ahiScore": 100,
            "algSubVer": 4, "algVer": 4,
            "deviceId": "app", "deviceSource": 2,
            "hrvBaseline": 101, "hrvInsight": 0, "hrvScore": 71,
            "insightId": 9,
            "mentBaseLine": 96, "mentInsight": 0, "mentScore": 96,
            "phyBaseline": 64, "phyInsight": 64, "phyScore": 64,
            "rdnsInsight": 5, "rdnsScore": 80,
            "rhrBaseline": 49, "rhrInsight": 240, "rhrScore": 74,
            "skinTempBaseLine": -7, "skinTempCalibrated": 11,
            "skinTempInsight": 5, "skinTempScore": 97,
            "sleepHRV": 88, "sleepRHR": 53,
            "status": 200,
            "timestamp": 1786464000000i64,
            "timestampUpdate": 1786493641000i64,
            "timezoneId": "Asia/Shanghai"
        }
    })
}

/// Sleep HRV / resting heart rate, two baselines, AHI baseline: always in the payload, but before v20
/// none of them reached the database.
#[test]
fn readiness_carries_sleep_hrv_and_the_personal_baselines() {
    let rows =
        Normalizer::normalize_daily_summary(&json!({ "items": [readiness_item()] })).unwrap();
    let daily = |metric: &str| {
        rows.iter()
            .find(|row| row.metric == metric)
            .map(|row| (row.value, row.unit.as_str()))
    };

    assert_eq!(daily("sleep_hrv"), Some((88.0, "ms")));
    assert_eq!(daily("sleep_rhr"), Some((53.0, "bpm")));
    assert_eq!(daily("hrv_baseline"), Some((101.0, "ms")));
    assert_eq!(daily("rhr_baseline"), Some((49.0, "bpm")));
    assert_eq!(daily("ahi_baseline"), Some((0.3827273, "events/h")));

    // The items already in the database must not change with this change.
    assert_eq!(daily("hrv_readiness"), Some((71.0, "score")));
    assert_eq!(daily("rhr_readiness"), Some((74.0, "score")));
}

/// `phyBaseline` / `mentBaseLine` are not read.
///
/// In 25,348 local readiness records they equal `phyScore` / `mentScore`
/// **exactly in every one**. They are not baselines; they are the same score under another name. In this fixture
/// they are also 64==64 and 96==96. Reading them would only add two identical columns to the database.
#[test]
fn the_physical_and_mental_baselines_are_not_stored_because_they_echo_the_score() {
    let rows =
        Normalizer::normalize_daily_summary(&json!({ "items": [readiness_item()] })).unwrap();
    assert!(!rows.iter().any(|row| row.metric == "physical_baseline"));
    assert!(!rows.iter().any(|row| row.metric == "mental_baseline"));
    assert_eq!(
        rows.iter()
            .find(|row| row.metric == "physical_readiness")
            .map(|row| row.value),
        Some(64.0)
    );
}

/// 255 is "not measured" in this stream. `afibScore` is 255 in all 25,348 local entries,
/// and `hrvBaseline` / `rhrBaseline` each have 7 entries of 255.
#[test]
fn readiness_sentinels_are_absent_but_valid_boundaries_and_charge_survive() {
    for value in [0, 100, 255] {
        let mut item = readiness_item();
        for field in [
            "rdnsScore",
            "phyScore",
            "mentScore",
            "hrvScore",
            "rhrScore",
            "skinTempScore",
            "afibScore",
            "ahiScore",
        ] {
            item["value"][field] = json!(value);
        }
        item["value"]["hybridCharge"] = json!(78);
        let rows = Normalizer::normalize_daily_summary(&json!({"items": [item]})).unwrap();
        let scores: Vec<_> = rows
            .iter()
            .filter(|row| row.metric == "readiness" || row.metric.ends_with("_readiness"))
            .collect();
        assert_eq!(scores.len(), if value == 255 { 0 } else { 8 });
        assert!(scores.iter().all(|row| row.value == value as f64));
        assert!(rows
            .iter()
            .any(|row| row.metric == "hybrid_charge" && row.value == 78.0));
    }
}

#[test]
fn a_baseline_of_255_is_dropped_rather_than_stored_as_a_reading() {
    let mut item = readiness_item();
    item["value"]["hrvBaseline"] = json!(255);
    item["value"]["rhrBaseline"] = json!(255);
    // The AHI baseline sentinel is -1, not 255.
    item["value"]["ahiBaseline"] = json!(-1.0);

    let rows = Normalizer::normalize_daily_summary(&json!({ "items": [item] })).unwrap();
    assert!(!rows.iter().any(|row| row.metric == "hrv_baseline"));
    assert!(!rows.iter().any(|row| row.metric == "rhr_baseline"));
    assert!(!rows.iter().any(|row| row.metric == "ahi_baseline"));
    // Values in the same record that no sentinel hides must still be written.
    assert!(rows.iter().any(|row| row.metric == "sleep_hrv"));
}

/// The three goal values come from `DailyHealth` samples; the payload is copied from the 2026-08-12 entry.
#[test]
fn the_daily_goals_are_read_from_the_summary_samples() {
    let raw = json!({ "items": [{
            "eventType": "DailyHealth",
            "subType": "summary",
            "timestamp": 1786492800000i64,
            "value": {
                "deviceId": "1,", "deviceSN": "1,",
                "deviceSource": "1,-1", "deviceType": "1,-1",
                "samples": [{
                    "burningDurationGoal": 30, "calorieGoal": 300,
                    "dateString": "2026-08-12", "s": 0, "stepGoal": 8000,
                    "totalBurningDuration": 0, "totalCalories": 12,
                    "totalSteps": 189, "u": 50755973
                }],
                "startTime": 1786492800000i64,
                "timeZone": "1,Asia/Shanghai"
            }
        }] });
    let rows = Normalizer::normalize_daily_summary(&raw).unwrap();
    let daily = |metric: &str| {
        rows.iter()
            .find(|row| row.metric == metric)
            .map(|row| (row.value, row.unit.as_str()))
    };

    assert_eq!(daily("step_goal"), Some((8000.0, "steps")));
    assert_eq!(daily("calorie_goal"), Some((300.0, "kcal")));
    assert_eq!(daily("active_minutes_goal"), Some((30.0, "min")));
    // The actual values of the day stay the same.
    assert_eq!(daily("steps"), Some((189.0, "steps")));
}

/// With no goal set, the payload has 0. 0 is not "a goal of 0 steps", so it is not written.
#[test]
fn a_goal_of_zero_means_no_goal_was_set_and_is_not_stored() {
    let raw = json!({ "items": [{
            "eventType": "DailyHealth", "subType": "summary",
            "timestamp": 1786492800000i64,
            "value": { "samples": [{
                "dateString": "2026-08-12", "stepGoal": 0, "calorieGoal": 0,
                "burningDurationGoal": 0, "totalSteps": 189
            }] }
        }] });
    let rows = Normalizer::normalize_daily_summary(&raw).unwrap();
    assert!(!rows.iter().any(|row| row.metric == "step_goal"));
    assert!(!rows.iter().any(|row| row.metric == "calorie_goal"));
    assert!(!rows.iter().any(|row| row.metric == "active_minutes_goal"));
    assert!(rows.iter().any(|row| row.metric == "steps"));
}

/// A real PAI payload, copied from the Shanghai 2026-08-20 entry in the local database (two array fields removed).
fn pai_item() -> Value {
    json!({
        "age": "20", "dailyPai": "11.707199",
        "deviceId": "D8803CFFFEC19AC6", "deviceSource": "8716544",
        "eventType": "PaiHealthInfo", "gender": "0",
        "highZoneLowerLimit": "158", "highZoneMinutes": "1",
        "highZonePai": "0.88804626", "index": "4",
        "lowZoneLowerLimit": "105", "lowZoneMinutes": "119",
        "lowZonePai": "2.0", "maxHr": "198",
        "mediumZoneLowerLimit": "119", "mediumZoneMinutes": "66",
        "mediumZonePai": "9.277756", "restHr": "65",
        "sn": "23229501001311", "subType": "PaiHealthInfo",
        "time": "1787155200000", "timeZone": "32",
        "timestamp": 1787155200000i64, "totalPai": "50.944435",
        "uploadTimestamp": "1787300767871", "userId": "1181735661",
        "version": "5"
    })
}

/// Seven-day PAI total, minutes in the three zones and their heart rate floors.
///
/// `totalPai` is the big number in the Zepp app, which we never read before. The heart rate floors of the three zones
/// are computed by the watch from the user's max/resting heart rate; we do not set them.
#[test]
fn pai_carries_the_total_and_the_three_zones() {
    let batch = Normalizer::normalize_wellness(
        "wellness:pai:user_events:2026-05-18:2026-05-19",
        &json!({ "items": [pai_item()] }),
    );
    let daily = |metric: &str| {
        batch
            .daily_metrics
            .iter()
            .find(|row| row.metric == metric)
            .map(|row| (row.value, row.unit.as_str()))
    };

    assert_eq!(daily("pai_total"), Some((50.944435, "pai")));
    assert_eq!(daily("pai_low_zone_minutes"), Some((119.0, "min")));
    assert_eq!(daily("pai_medium_zone_minutes"), Some((66.0, "min")));
    assert_eq!(daily("pai_high_zone_minutes"), Some((1.0, "min")));
    assert_eq!(daily("pai_low_zone_lower_hr"), Some((105.0, "bpm")));
    assert_eq!(daily("pai_medium_zone_lower_hr"), Some((119.0, "bpm")));
    assert_eq!(daily("pai_high_zone_lower_hr"), Some((158.0, "bpm")));

    // The items that were already there must not change.
    assert_eq!(daily("pai_daily"), Some((11.707199, "pai")));
    assert_eq!(daily("device_max_hr"), Some((198.0, "bpm")));
}

/// When no minute was spent in a zone, the payload has 0. That is a real 0 minutes and must be written.
///
/// It differs from the 0 of a goal value: a goal of 0 means "no goal set".
#[test]
fn zero_minutes_in_a_pai_zone_is_a_real_reading() {
    let mut item = pai_item();
    item["highZoneMinutes"] = json!("0");
    let batch = Normalizer::normalize_wellness(
        "wellness:pai:user_events:2026-05-18:2026-05-19",
        &json!({ "items": [item] }),
    );
    assert_eq!(
        batch
            .daily_metrics
            .iter()
            .find(|row| row.metric == "pai_high_zone_minutes")
            .map(|row| row.value),
        Some(0.0)
    );
}

/// The PAI `timestamp` is local midnight, and `timeZone` counts in 15 minutes. Before, days were split by UTC,
/// so the eastern hemisphere was one day early: a 4-hour ride on Sep 19 put the full score of 75 on Sep 18.
#[test]
fn pai_lands_on_the_local_day_it_scores() {
    let daily_dates = |item: Value| {
        Normalizer::normalize_wellness(
            "wellness:pai:user_events:2026-08-19:2026-08-21",
            &json!({ "items": [item] }),
        )
        .daily_metrics
        .iter()
        .filter(|row| row.metric == "pai_daily")
        .map(|row| row.date.clone())
        .collect::<Vec<_>>()
    };
    // 2026-08-19 16:00 UTC, Shanghai "32" = UTC+8 -> midnight of Aug 20.
    assert_eq!(daily_dates(pai_item()), vec!["2026-08-20"]);

    // Moscow UTC+3 is "12": midnight of Sep 19 = Sep 18 21:00 UTC.
    let mut moscow = pai_item();
    moscow["timestamp"] = json!(1_789_765_200_000i64);
    moscow["time"] = json!("1789765200000");
    moscow["timeZone"] = json!("12");
    assert_eq!(daily_dates(moscow), vec!["2026-09-19"]);

    // Not on midnight after conversion, so this is not that encoding: fall back to the old day split; do not force it.
    let mut odd = pai_item();
    odd["timeZone"] = json!("31");
    assert_eq!(daily_dates(odd), vec!["2026-08-19"]);
}

/// When the watch, the phone merge and the fallback each give an entry for the same day, write one row from the entry uploaded last.
/// Before, rows were split by device ID: two `pai_daily` rows per day, one with a half-day value from mid-day.
#[test]
fn pai_keeps_one_row_per_day_from_the_latest_upload() {
    let mut watch = pai_item();
    watch["dailyPai"] = json!("29.269928");
    watch["uploadTimestamp"] = json!("1787200000000");
    let mut fused = pai_item();
    fused["deviceId"] = json!("fused");
    fused["dailyPai"] = json!("75");
    fused["uploadTimestamp"] = json!("1787300000000");
    let mut fallback = pai_item();
    fallback["deviceId"] = json!("max-device-fallback");
    fallback["dailyPai"] = json!("0");
    fallback["uploadTimestamp"] = json!("1787190000000");

    let batch = Normalizer::normalize_wellness(
        "wellness:pai:user_events:2026-08-19:2026-08-21",
        &json!({ "items": [fused, watch, fallback] }),
    );
    let rows: Vec<_> = batch
        .daily_metrics
        .iter()
        .filter(|row| row.metric == "pai_daily")
        .map(|row| (row.date.as_str(), row.value, row.device_id.clone()))
        .collect();
    assert_eq!(rows, vec![("2026-08-20", 75.0, None)]);
}

fn all_day_stress_item() -> Value {
    json!({
        "avgStress": "22",
        "data": "[{\"time\":1788307200000,\"value\":32},{\"time\":1788307500000,\"value\":25},{\"time\":1788307800000,\"value\":32},{\"time\":1788308100000,\"value\":48},{\"time\":1788308400000,\"value\":20},{\"time\":1788308700000,\"value\":32},{\"time\":1788309000000,\"value\":33},{\"time\":1788309300000,\"value\":10},{\"time\":1788309600000,\"value\":28},{\"time\":1788309900000,\"value\":6},{\"time\":1788310200000,\"value\":4},{\"time\":1788310500000,\"value\":7},{\"time\":1788310800000,\"value\":11}]",
        "deviceId": "D85403FFFEE4D576",
        "deviceMac": "",
        "deviceSn": "2445B138005129",
        "deviceSource": "10289410",
        "deviceType": "0",
        "eventType": "all_day_stress",
        "highProportion": "0",
        "maxStress": "48",
        "mediumProportion": "0",
        "minStress": "4",
        "normalProportion": "8",
        "relaxProportion": "92",
        "subType": "all_day_stress",
        "timestamp": 1788307200000i64
    })
}

#[test]
fn all_day_stress_yields_the_whole_days_curve() {
    let batch = Normalizer::normalize_wellness(
        "wellness:all_day_stress:user_events:2026-09-02:2026-09-03",
        &json!({ "items": [all_day_stress_item()] }),
    );

    // All 13 points, none missing. This is what "stress is not 24/7" lacked.
    assert_eq!(batch.metric_samples.len(), 13);
    assert!(batch
        .metric_samples
        .iter()
        .all(|sample| sample.metric == "stress" && sample.unit == "score"));
    assert!(batch
        .metric_samples
        .iter()
        .all(|sample| sample.device_id.as_deref() == Some("D85403FFFEE4D576")));

    let first = &batch.metric_samples[0];
    assert_eq!(first.value, 32.0);
    assert_eq!(first.timestamp.to_rfc3339(), "2026-09-02T00:00:00+00:00");
    let last = batch.metric_samples.last().unwrap();
    assert_eq!(last.value, 11.0);
    assert_eq!(last.timestamp.to_rfc3339(), "2026-09-02T01:00:00+00:00");

    // The server's daily extremes are the extremes of this curve: both are the same measurement,
    // not two streams that happen to match.
    let values: Vec<f64> = batch.metric_samples.iter().map(|s| s.value).collect();
    assert_eq!(values.iter().copied().reduce(f64::min), Some(4.0));
    assert_eq!(values.iter().copied().reduce(f64::max), Some(48.0));
}

#[test]
fn all_day_stress_band_proportions_are_read() {
    let batch = Normalizer::normalize_wellness(
        "wellness:all_day_stress:user_events:2026-09-02:2026-09-03",
        &json!({ "items": [all_day_stress_item()] }),
    );

    let daily = |metric: &str| {
        batch
            .daily_metrics
            .iter()
            .find(|row| row.metric == metric)
            .map(|row| row.value)
    };

    assert_eq!(daily("stress"), Some(22.0));
    assert_eq!(daily("stress_min"), Some(4.0));
    assert_eq!(daily("stress_max"), Some(48.0));
    // The payload name is `relaxProportion`. Before, only `relaxPct` was accepted, and that name
    // never appeared in 1,104 real records, so these four items never reached the database.
    assert_eq!(daily("stress_relaxed_pct"), Some(92.0));
    assert_eq!(daily("stress_normal_pct"), Some(8.0));
    assert_eq!(daily("stress_medium_pct"), Some(0.0));
    assert_eq!(daily("stress_high_pct"), Some(0.0));
}

#[test]
fn all_day_stress_drops_zero_readings() {
    // Zepp stress starts at 1. 0 never appears in 62,626 real readings in the database,
    // and a 0 in these payloads always means "not measured". Drawn as 0, it would look like a moment
    // of deep relaxation.
    let batch = Normalizer::normalize_wellness(
        "wellness:all_day_stress:user_events:2026-09-02:2026-09-03",
        &json!({ "items": [{
                "eventType": "all_day_stress",
                "timestamp": 1788307200000i64,
                "data": "[{\"time\":1788307200000,\"value\":0},{\"time\":1788307500000,\"value\":25}]"
            }] }),
    );

    assert_eq!(batch.metric_samples.len(), 1);
    assert_eq!(batch.metric_samples[0].value, 25.0);
}

#[test]
fn parse_number_rejects_nan_and_infinity() {
    for raw in ["NaN", "nan", "Infinity", "-Infinity", "inf", "+inf"] {
        assert_eq!(parse_number(&json!(raw)), None, "{raw}");
    }
    assert_eq!(parse_number(&json!(72)), Some(72.0));
    assert_eq!(parse_number(&json!("72.5")), Some(72.5));
    let batch = Normalizer::normalize_heart_rate_with_diagnostics(&json!({
        "items": [
            {"timestamp": 1_800_000_000i64, "value": "NaN"},
            {"timestamp": 1_800_000_060i64, "heartRate": "Infinity"},
            {"timestamp": 1_800_000_120i64, "value": 68}
        ]
    }))
    .unwrap();
    assert_eq!(batch.records.len(), 1);
    assert_eq!(batch.records[0].value, 68.0);
    assert!(
        batch.records.iter().all(|sample| sample.value.is_finite()),
        "NaN/Infinity must not enter heart rate records"
    );
}
