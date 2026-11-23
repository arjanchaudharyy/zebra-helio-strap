use super::*;
use rustyfit::Decoder;
use serde_json::json;

fn export_with(data: serde_json::Value) -> Value {
    json!({ "generated_at": "2026-09-02T10:00:00+08:00", "data": data })
}

fn decode(bytes: &[u8]) -> FIT {
    let mut cursor = std::io::Cursor::new(bytes);
    let mut decoder = Decoder::new();
    decoder
        .decode(&mut FromStd::new(&mut cursor))
        .expect("FIT should decode back")
        .expect("FIT should not be empty")
}

fn raw(mesg: &Message, num: u8) -> Option<&FitValue> {
    mesg.fields
        .iter()
        .find(|field| field.num == num)
        .map(|field| &field.value)
}

/// `rustyfit::proto::Value` does not implement `PartialEq`, so assertions always extract the integer first
/// and then compare. When extraction fails, return `None`, so "field missing" and "wrong value" look different in the failure
/// message.
fn int_of(mesg: &Message, num: u8) -> Option<i64> {
    match raw(mesg, num)? {
        FitValue::Uint8(value) => Some(i64::from(*value)),
        FitValue::Uint16(value) => Some(i64::from(*value)),
        FitValue::Uint32(value) => Some(i64::from(*value)),
        FitValue::Int8(value) => Some(i64::from(*value)),
        FitValue::Int16(value) => Some(i64::from(*value)),
        FitValue::Int32(value) => Some(i64::from(*value)),
        _ => None,
    }
}

fn messages_of(fit: &FIT, num: typedef::MesgNum) -> Vec<&Message> {
    fit.messages.iter().filter(|m| m.num == num).collect()
}

fn running_export() -> Value {
    export_with(json!({
        "workouts": [
            {
                "workout_id": "w1",
                "effective_type": "run",
                "start_time": "2026-08-24T06:00:00+08:00",
                "end_time": "2026-08-24T06:00:02+08:00",
                "distance_meters": 15217.0,
                "calories": 900.0,
                "avg_hr": 141,
                "max_hr": 173,
                "total_steps": 8998,
                "device_label": "Amazfit Balance",
                "route": [
                    { "timestamp": "2026-08-24T06:00:00+08:00", "latitude": 31.2304,
                      "longitude": 121.4737, "altitude_m": 12.4 },
                    { "timestamp": "2026-08-24T06:00:01+08:00", "latitude": 31.2305,
                      "longitude": 121.4738, "altitude_m": 12.6 }
                ],
                "samples": [
                    { "timestamp": "2026-08-24T06:00:00+08:00", "heart_rate": 132,
                      "speed": 3.5, "power_watts": 249.0, "ground_contact_ms": 263.0,
                      "vertical_oscillation_mm": 88.0, "cadence": 170.0 },
                    { "timestamp": "2026-08-24T06:00:01+08:00", "heart_rate": 134,
                      "speed": 3.6 }
                ],
                "splits": [
                    { "index": 1, "start_time": "2026-08-24T06:00:00+08:00",
                      "end_time": "2026-08-24T06:00:02+08:00", "distance_m": 1000.0,
                      "duration_seconds": 2, "avg_hr": 133, "max_hr": 134,
                      "elevation_gain_m": 3.0, "elevation_loss_m": 1.0, "partial": false }
                ],
                "pauses": []
            }
        ]
    }))
}

#[test]
fn issue_24_cloud_trail_run_exports_as_trail_in_json_gpx_and_fit() {
    let workouts = crate::normalizer::Normalizer::normalize_workouts_with_sport(
        &json!({"data": [{
            "trackid": 1_700_000_000i64, "end_time": 1_700_000_600i64, "type": 7
        }]}),
        None,
    )
    .unwrap();
    let mut workout = serde_json::to_value(&workouts[0]).unwrap();
    assert_eq!(workout["workout_type"], "trail_running");
    workout["route"] = json!([{
        "timestamp": "2023-11-14T22:13:20Z", "latitude": 0.0, "longitude": 0.0
    }]);
    let export = export_with(json!({"workouts": [workout]}));
    let (gpx, points) = crate::export_formats::to_gpx(&export).unwrap();
    assert_eq!(points, 1);
    assert!(gpx.contains("<type>trail_running</type>"));
    assert!(!gpx.contains("swimming"));
    let (files, _) = to_fit(&export).unwrap();
    assert!(files[0].0.ends_with("-trail-running.fit"));
    let fit = decode(&files[0].1);
    let session = messages_of(&fit, typedef::MesgNum::SESSION);
    assert_eq!(
        int_of(session[0], mesgdef::Session::SPORT),
        Some(i64::from(typedef::Sport::RUNNING.0))
    );
    assert_eq!(
        int_of(session[0], mesgdef::Session::SUB_SPORT),
        Some(i64::from(typedef::SubSport::TRAIL.0))
    );
}

#[test]
fn writes_one_file_per_workout_and_decodes_back() {
    let (files, records) = to_fit(&running_export()).unwrap();

    assert_eq!(files.len(), 1);
    assert_eq!(records, 2, "one record for each of the two points in time");
    assert_eq!(files[0].0, "20260824-060000-run.fit");

    let fit = decode(&files[0].1);

    // file_id must be first, and the manufacturer is Zepp: the data really comes from the Zepp cloud.
    let first = &fit.messages[0];
    assert_eq!(first.num, typedef::MesgNum::FILE_ID);
    assert_eq!(
        int_of(first, mesgdef::FileId::MANUFACTURER),
        Some(i64::from(typedef::Manufacturer::ZEPP.0))
    );

    let records = messages_of(&fit, typedef::MesgNum::RECORD);
    assert_eq!(records.len(), 2);

    // Coordinates round-trip through semicircles, with 1 lowest bit of rounding allowed.
    let lat = int_of(records[0], mesgdef::Record::POSITION_LAT).expect("latitude should be written");
    let expected = (31.2304 * SEMICIRCLES_PER_DEGREE).round() as i64;
    assert!((lat - expected).abs() <= 1, "lat={lat} expected={expected}");

    assert_eq!(int_of(records[0], mesgdef::Record::HEART_RATE), Some(132));
    // Speed scale 1000: 3.5 m/s -> 3500
    assert_eq!(int_of(records[0], mesgdef::Record::SPEED), Some(3500));
    assert_eq!(int_of(records[0], mesgdef::Record::POWER), Some(249));
    // Ground contact time scale 10: 263 ms -> 2630
    assert_eq!(int_of(records[0], mesgdef::Record::STANCE_TIME), Some(2630));
    // Vertical oscillation scale 10: 88 mm -> 880
    assert_eq!(
        int_of(records[0], mesgdef::Record::VERTICAL_OSCILLATION),
        Some(880)
    );
    // Altitude (12.4 + 500) * 5 = 2562
    assert_eq!(int_of(records[0], mesgdef::Record::ALTITUDE), Some(2562));

    let session = messages_of(&fit, typedef::MesgNum::SESSION);
    assert_eq!(session.len(), 1);
    assert_eq!(
        int_of(session[0], mesgdef::Session::SPORT),
        Some(i64::from(typedef::Sport::RUNNING.0))
    );
    assert_eq!(
        int_of(session[0], mesgdef::Session::SUB_SPORT),
        Some(i64::from(typedef::SubSport::STREET.0))
    );
    // Distance scale 100: 15217 m -> 1521700
    assert_eq!(
        int_of(session[0], mesgdef::Session::TOTAL_DISTANCE),
        Some(1_521_700)
    );

    assert_eq!(messages_of(&fit, typedef::MesgNum::LAP).len(), 1);
    assert_eq!(messages_of(&fit, typedef::MesgNum::ACTIVITY).len(), 1);
}

#[test]
fn raw_power_survives_storage_and_single_workout_fit_export() {
    use crate::models::types::{ExportDetail, ExportScope, ExportSelection, Workout};
    use crate::storage::Database;
    let db = Database::in_memory().unwrap();
    let start = DateTime::from_timestamp(1_700_000_000, 0).unwrap();
    let id = "1700000000";
    db.insert_workout(&Workout {
        workout_id: id.into(),
        workout_type: "run".into(),
        normalized_type: "run".into(),
        effective_type: "run".into(),
        start_time: start,
        end_time: start + chrono::Duration::seconds(3),
        ..Default::default()
    })
    .unwrap();
    let payload = json!({
        "trackid": 1_700_000_000i64,
        "time": "0;1;1;1;",
        "heart_rate": "0,120;1,1;1,1;1,1;",
        "power_meter": "0,200;,250;,0;,300;"
    });
    db.persist_fetched_record(&crate::models::RawRecord {
        stream: "workout_detail".into(),
        source_key: "workout_detail:1700000000:run.gps".into(),
        source_scope: crate::models::SourceScope::Device,
        device_id: None,
        start_utc: start,
        end_utc: None,
        payload,
        capability: crate::models::CapabilityStatus::Verified,
    })
    .unwrap();
    let selection = ExportSelection {
        scope: Some(ExportScope::Workout {
            workout_id: id.into(),
        }),
        start_date: None,
        end_date: None,
        data_types: vec!["workouts".into()],
        detail: ExportDetail::Full,
    };
    let (encoded, _) = db.build_ai_export(&selection).unwrap();
    let (files, _) = to_fit(&serde_json::from_str(&encoded).unwrap()).unwrap();
    assert_eq!(files.len(), 1);
    let fit = decode(&files[0].1);
    let powers: Vec<_> = messages_of(&fit, typedef::MesgNum::RECORD)
        .iter()
        .map(|record| int_of(record, mesgdef::Record::POWER))
        .collect();
    assert_eq!(powers, vec![Some(200), Some(250), Some(0), Some(300)]);
}

/// A workout without splits must still have a lap, and `num_laps >= 1`.
///
/// Indoor workouts (treadmill, rowing machine, yoga, free training) and outdoor workouts shorter than 1 km
/// get no `splits` from the server. The FIT spec requires at least one lap per session. Without it,
/// Garmin Connect and Strava reject the whole file when they check the message hierarchy.
#[test]
fn workout_without_splits_still_gets_one_lap() {
    let treadmill = export_with(json!({
        "workouts": [{
            "workout_id": "w1", "effective_type": "treadmill",
            "start_time": "2026-08-24T06:00:00+08:00",
            "distance_meters": 830.0, "calories": 62.0,
            "avg_hr": 128, "max_hr": 141,
            "route": [], "splits": [], "pauses": [],
            "samples": [
                { "timestamp": "2026-08-24T06:00:00+08:00", "heart_rate": 120, "speed": 2.5 },
                { "timestamp": "2026-08-24T06:05:00+08:00", "heart_rate": 141, "speed": 2.8 }
            ]
        }]
    }));
    let (files, _) = to_fit(&treadmill).unwrap();
    let fit = decode(&files[0].1);

    let laps = messages_of(&fit, typedef::MesgNum::LAP);
    assert_eq!(laps.len(), 1, "without splits, one lap that covers the whole activity must still be written");
    assert_eq!(
        int_of(laps[0], mesgdef::Lap::LAP_TRIGGER),
        Some(i64::from(typedef::LapTrigger::SESSION_END.0)),
        "the trigger of the whole-activity lap is session_end, not distance"
    );
    // 300 s -> 300000 ms
    assert_eq!(
        int_of(laps[0], mesgdef::Lap::TOTAL_ELAPSED_TIME),
        Some(300_000)
    );
    // Distance scale 100: 830 m -> 83000
    assert_eq!(int_of(laps[0], mesgdef::Lap::TOTAL_DISTANCE), Some(83_000));
    assert_eq!(int_of(laps[0], mesgdef::Lap::MAX_HEART_RATE), Some(141));

    let session = messages_of(&fit, typedef::MesgNum::SESSION);
    assert_eq!(
        int_of(session[0], mesgdef::Session::NUM_LAPS),
        Some(1),
        "num_laps = 0 is rejected outright by Garmin / Strava"
    );
}

/// `activity.local_timestamp` must carry the offset of the watch's time zone at that time.
///
/// Without it, Garmin Connect falls back to the account's default time zone, and a 06:00 run in Beijing time
/// is marked as the evening before. The offset comes from the RFC3339 suffix of `start_time`. It is not guessed.
#[test]
fn activity_carries_local_timestamp_from_the_recorded_offset() {
    let (files, _) = to_fit(&running_export()).unwrap();
    let fit = decode(&files[0].1);
    let activity = messages_of(&fit, typedef::MesgNum::ACTIVITY);
    assert_eq!(activity.len(), 1);

    let utc = int_of(activity[0], mesgdef::Activity::TIMESTAMP).expect("UTC timestamp should be written");
    let local =
        int_of(activity[0], mesgdef::Activity::LOCAL_TIMESTAMP).expect("local timestamp should be written");
    // The fixture is +08:00
    assert_eq!(local - utc, 8 * 3600);
}

#[test]
fn utc_normalized_workouts_do_not_claim_a_local_timezone() {
    for start in ["2026-08-23T22:00:00Z", "2026-08-23T22:00:00+00:00"] {
        let mut export = running_export();
        export["data"]["workouts"][0]["start_time"] = json!(start);
        let (files, _) = to_fit(&export).unwrap();
        let fit = decode(&files[0].1);
        let activity = messages_of(&fit, typedef::MesgNum::ACTIVITY);
        assert_eq!(activity.len(), 1);
        assert!(int_of(activity[0], mesgdef::Activity::TIMESTAMP).is_some());
        assert!(raw(activity[0], mesgdef::Activity::LOCAL_TIMESTAMP).is_none());
    }
}

/// A point at longitude exactly 180.0 degrees must keep its coordinates.
#[test]
fn antimeridian_longitude_is_clamped_not_dropped() {
    assert_eq!(semicircles(180.0), Some(i32::MAX));
    assert_eq!(semicircles(-180.0), Some(i32::MIN));
    assert_eq!(semicircles(180.5), None, "out of range is still rejected");
    assert_eq!(semicircles(f64::NAN), None);
}

/// Cadence: the source is steps/min, FIT `cadence` is rpm, and running/walking divide by two.
///
/// The unit is not guessed. It was checked against this workout's own cloud summary. See
/// the table above `steps_per_minute_to_fit_cadence`.
#[test]
fn cadence_is_halved_for_foot_sports_and_left_alone_for_cycling() {
    // The first sample of the run in the fixture is 170 steps/min -> 85 rpm
    let (files, _) = to_fit(&running_export()).unwrap();
    let fit = decode(&files[0].1);
    let records = messages_of(&fit, typedef::MesgNum::RECORD);
    assert_eq!(
        int_of(records[0], mesgdef::Record::CADENCE),
        Some(85),
        "run: 170 steps/min should be written as 85 rpm; readers multiply by two to show 170"
    );

    let session = messages_of(&fit, typedef::MesgNum::SESSION);
    assert_eq!(int_of(session[0], mesgdef::Session::AVG_CADENCE), Some(85));
    assert_eq!(int_of(session[0], mesgdef::Session::MAX_CADENCE), Some(85));

    // In cycling one cycle is one crank turn, already rpm, so do not divide again.
    let ride = export_with(json!({
        "workouts": [{
            "workout_id": "w1", "effective_type": "road_cycling",
            "start_time": "2026-08-24T06:00:00+08:00",
            "route": [], "splits": [], "pauses": [],
            "samples": [
                { "timestamp": "2026-08-24T06:00:00+08:00", "cadence": 90.0 },
                { "timestamp": "2026-08-24T06:00:01+08:00", "cadence": 90.0 }
            ]
        }]
    }));
    let (files, _) = to_fit(&ride).unwrap();
    let fit = decode(&files[0].1);
    let records = messages_of(&fit, typedef::MesgNum::RECORD);
    assert_eq!(
        int_of(records[0], mesgdef::Record::CADENCE),
        Some(90),
        "ride: 90 rpm is written unchanged"
    );
}

/// Steps are written as session `TOTAL_CYCLES`, divided by two for walking-type workouts.
///
/// Without this field, importers can only estimate from distance: OPPO Health estimated
/// 1274 steps from 0.83 km. The cloud summary already has the real step count, and it is in the database since PR #34.
#[test]
fn total_steps_become_session_cycles() {
    // total_steps in the fixture is 8998 -> 4499 full strides
    let (files, _) = to_fit(&running_export()).unwrap();
    let fit = decode(&files[0].1);
    let session = messages_of(&fit, typedef::MesgNum::SESSION);
    assert_eq!(
        int_of(session[0], mesgdef::Session::TOTAL_CYCLES),
        Some(4499),
        "run: 8998 steps should be written as 4499 cycles; readers multiply by two to show 8998"
    );

    // In cycling a cycle is one crank turn and has nothing to do with steps, so nothing must be written.
    let ride = export_with(json!({
        "workouts": [{
            "workout_id": "w1", "effective_type": "road_cycling",
            "start_time": "2026-08-24T06:00:00+08:00",
            "total_steps": 8998,
            "route": [], "splits": [], "pauses": [],
            "samples": [
                { "timestamp": "2026-08-24T06:00:00+08:00", "heart_rate": 120 },
                { "timestamp": "2026-08-24T06:00:01+08:00", "heart_rate": 121 }
            ]
        }]
    }));
    let (files, _) = to_fit(&ride).unwrap();
    let fit = decode(&files[0].1);
    let session = messages_of(&fit, typedef::MesgNum::SESSION);
    assert_eq!(
        int_of(session[0], mesgdef::Session::TOTAL_CYCLES),
        None,
        "ride: steps are not a total pedal count, so TOTAL_CYCLES is not written"
    );

    // A record without steps is still not filled with zero.
    let bare = export_with(json!({
        "workouts": [{
            "workout_id": "w1", "effective_type": "run",
            "start_time": "2026-08-24T06:00:00+08:00",
            "route": [], "splits": [], "pauses": [],
            "samples": [
                { "timestamp": "2026-08-24T06:00:00+08:00", "heart_rate": 120 },
                { "timestamp": "2026-08-24T06:00:01+08:00", "heart_rate": 121 }
            ]
        }]
    }));
    let (files, _) = to_fit(&bare).unwrap();
    let fit = decode(&files[0].1);
    let session = messages_of(&fit, typedef::MesgNum::SESSION);
    assert_eq!(
        int_of(session[0], mesgdef::Session::TOTAL_CYCLES),
        None,
        "no steps means this field is not written, not filled with zero"
    );
}

/// Average/max speed and total ascent must be written on the session.
///
/// Importers (tested: OPPO Health) read session fields and do not compute them from records.
/// Without them, "average speed", "max speed" and "total ascent" in the overview are all 0, even when
/// the splits and per-second series clearly have values.
#[test]
fn the_session_carries_average_speed_max_speed_and_elevation() {
    // The running_export() fixture is only 2 s long but has 15217 m, so the computed average
    // speed overflows u16. That is an artificial fixture setting, not a real case. Here we use
    // a separate record with a reasonable duration.
    let export = export_with(json!({
        "workouts": [{
            "workout_id": "w1", "effective_type": "run",
            "start_time": "2026-08-24T06:00:00+08:00",
            "distance_meters": 1000.0,
            "route": [], "pauses": [],
            "samples": [
                { "timestamp": "2026-08-24T06:00:00+08:00", "speed": 3.0 },
                { "timestamp": "2026-08-24T06:08:20+08:00", "speed": 5.0 }
            ],
            "splits": [{
                "index": 1,
                "start_time": "2026-08-24T06:00:00+08:00",
                "end_time": "2026-08-24T06:08:20+08:00",
                "distance_m": 1000.0, "duration_seconds": 500,
                "elevation_gain_m": 9.35, "elevation_loss_m": 1.16,
                "partial": false
            }]
        }]
    }));
    let (files, _) = to_fit(&export).unwrap();
    let fit = decode(&files[0].1);
    let session = messages_of(&fit, typedef::MesgNum::SESSION);

    // 1000 m / 500 s = 2 m/s -> scale 1000 -> 2000
    assert_eq!(int_of(session[0], mesgdef::Session::AVG_SPEED), Some(2000));
    // Series max 5 m/s -> 5000
    assert_eq!(int_of(session[0], mesgdef::Session::MAX_SPEED), Some(5000));
    // 9.35 m rounds to 9
    assert_eq!(int_of(session[0], mesgdef::Session::TOTAL_ASCENT), Some(9));
    assert_eq!(int_of(session[0], mesgdef::Session::TOTAL_DESCENT), Some(1));

    let lap = messages_of(&fit, typedef::MesgNum::LAP);
    assert_eq!(int_of(lap[0], mesgdef::Lap::AVG_SPEED), Some(2000));
    assert_eq!(int_of(lap[0], mesgdef::Lap::MAX_SPEED), Some(5000));
}

#[test]
fn an_indoor_workout_without_gps_still_produces_a_file() {
    // FIT does not require GPS. A treadmill record with heart rate only is fully valid FIT,
    // so the empty check must be "no route and no samples".
    let export = export_with(json!({
        "workouts": [
            {
                "workout_id": "w1", "effective_type": "treadmill",
                "start_time": "2026-08-24T06:00:00+08:00",
                "route": [],
                "samples": [
                    { "timestamp": "2026-08-24T06:00:00+08:00", "heart_rate": 120 },
                    { "timestamp": "2026-08-24T06:00:01+08:00", "heart_rate": 122 }
                ],
                "splits": [], "pauses": []
            }
        ]
    }));

    let (files, records) = to_fit(&export).unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(records, 2);

    let fit = decode(&files[0].1);
    let session = messages_of(&fit, typedef::MesgNum::SESSION);
    assert_eq!(
        int_of(session[0], mesgdef::Session::SUB_SPORT),
        Some(i64::from(typedef::SubSport::TREADMILL.0))
    );
    for record in messages_of(&fit, typedef::MesgNum::RECORD) {
        assert!(raw(record, mesgdef::Record::POSITION_LAT).is_none());
    }
}

#[test]
fn a_pause_becomes_timer_stop_and_start() {
    let export = export_with(json!({
        "workouts": [
            {
                "workout_id": "w1", "effective_type": "run",
                "start_time": "2026-08-24T06:00:00+08:00",
                "route": [
                    { "timestamp": "2026-08-24T06:00:00+08:00", "latitude": 31.0, "longitude": 121.0 },
                    { "timestamp": "2026-08-24T06:10:00+08:00", "latitude": 31.1, "longitude": 121.1 }
                ],
                "samples": [], "splits": [],
                "pauses": [
                    { "start_time": "2026-08-24T06:02:00+08:00",
                      "end_time": "2026-08-24T06:05:00+08:00", "kind": "manual" }
                ]
            }
        ]
    }));

    let (files, _) = to_fit(&export).unwrap();
    let fit = decode(&files[0].1);
    let events = messages_of(&fit, typedef::MesgNum::EVENT);

    // start + pause stop + resume start + end stop
    assert_eq!(events.len(), 4, "timer stop / start must be written on both sides of a pause");
    let types: Vec<Option<i64>> = events
        .iter()
        .map(|event| int_of(event, mesgdef::Event::EVENT_TYPE))
        .collect();
    let start = Some(i64::from(typedef::EventType::START.0));
    let stop = Some(i64::from(typedef::EventType::STOP.0));
    assert_eq!(types, vec![start, stop, start, stop]);
    let session = messages_of(&fit, typedef::MesgNum::SESSION)[0];
    assert_eq!(
        int_of(session, mesgdef::Session::TOTAL_ELAPSED_TIME),
        Some(600_000)
    );
    assert_eq!(
        int_of(session, mesgdef::Session::TOTAL_TIMER_TIME),
        Some(420_000)
    );
    let lap = messages_of(&fit, typedef::MesgNum::LAP)[0];
    assert_eq!(int_of(lap, mesgdef::Lap::TOTAL_TIMER_TIME), Some(420_000));
    let activity = messages_of(&fit, typedef::MesgNum::ACTIVITY)[0];
    assert_eq!(
        int_of(activity, mesgdef::Activity::TOTAL_TIMER_TIME),
        Some(420_000)
    );
}

#[test]
fn overlapping_and_out_of_bounds_pauses_are_counted_once() {
    let workout = json!({"pauses": [
        {"start_time": "2026-09-11T00:01:00Z", "end_time": "2026-09-11T00:04:00Z"},
        {"start_time": "2026-09-11T00:02:00Z", "end_time": "2026-09-11T00:05:00Z"},
        {"start_time": "2026-09-10T23:59:00Z", "end_time": "2026-09-11T00:00:30Z"},
        {"start_time": "2026-09-11T00:09:30Z", "end_time": "2026-09-11T00:12:00Z"},
        {"start_time": "2026-09-11T00:08:00Z", "end_time": "2026-09-11T00:07:00Z"}
    ]});
    let start = parse_unix("2026-09-11T00:00:00Z").unwrap();
    assert_eq!(timer_seconds(&workout, start, start + 600), 300.0);
}

#[test]
fn cloud_moving_time_and_full_activity_bounds_survive_sparse_samples() {
    let export = export_with(json!({"workouts": [{
        "workout_id": "moving-time", "effective_type": "run",
        "start_time": "2026-09-11T00:00:00Z", "end_time": "2026-09-11T00:10:00Z",
        "moving_seconds": 420, "distance_meters": 1400,
        "samples": [
            {"timestamp": "2026-09-11T00:00:10Z", "heart_rate": 100},
            {"timestamp": "2026-09-11T00:08:00Z", "heart_rate": 110}
        ],
        "route": [], "pauses": [], "splits": []
    }]}));
    let (files, _) = to_fit(&export).unwrap();
    let fit = decode(&files[0].1);
    let session = messages_of(&fit, typedef::MesgNum::SESSION)[0];
    assert_eq!(
        int_of(session, mesgdef::Session::TOTAL_ELAPSED_TIME),
        Some(600_000)
    );
    assert_eq!(
        int_of(session, mesgdef::Session::TOTAL_TIMER_TIME),
        Some(420_000)
    );
    assert_eq!(int_of(session, mesgdef::Session::AVG_SPEED), Some(3333));
    let lap = messages_of(&fit, typedef::MesgNum::LAP)[0];
    assert_eq!(int_of(lap, mesgdef::Lap::TOTAL_TIMER_TIME), Some(420_000));
}

/// When the cloud gives ascent, use the cloud value. Do not overwrite it with the sum of splits.
///
/// The measured difference is large: one 6.37 km walk had 59 m from the cloud and 37 m as the sum of splits. The user
/// sees 59 in the Zepp App.
#[test]
fn cloud_elevation_wins_over_the_sum_of_splits() {
    let export = export_with(json!({
        "workouts": [{
            "workout_id": "w1", "effective_type": "walking",
            "start_time": "2026-08-28T15:23:37+08:00",
            "distance_meters": 6377.0,
            "elevation_gain_m": 59.35,
            "elevation_loss_m": 59.36,
            "route": [], "pauses": [],
            "samples": [
                { "timestamp": "2026-08-28T15:23:37+08:00", "heart_rate": 105 },
                { "timestamp": "2026-08-28T16:53:40+08:00", "heart_rate": 121 }
            ],
            // The sum of splits is only 16, clearly different from the cloud's 59
            "splits": [
                { "index": 1, "start_time": "2026-08-28T15:23:37+08:00",
                  "end_time": "2026-08-28T15:39:33+08:00", "distance_m": 1000.0,
                  "duration_seconds": 956, "elevation_gain_m": 9.7,
                  "elevation_loss_m": 12.01, "partial": false },
                { "index": 2, "start_time": "2026-08-28T15:39:33+08:00",
                  "end_time": "2026-08-28T15:53:51+08:00", "distance_m": 1000.0,
                  "duration_seconds": 858, "elevation_gain_m": 7.19,
                  "elevation_loss_m": 3.03, "partial": false }
            ]
        }]
    }));

    let (files, _) = to_fit(&export).unwrap();
    let fit = decode(&files[0].1);
    let session = messages_of(&fit, typedef::MesgNum::SESSION);
    assert_eq!(
        int_of(session[0], mesgdef::Session::TOTAL_ASCENT),
        Some(59),
        "with 59.35 from the cloud, write 59, not the sum of splits 17"
    );
    assert_eq!(
        int_of(session[0], mesgdef::Session::TOTAL_DESCENT),
        Some(59)
    );
}

#[test]
fn refuses_to_write_an_empty_file() {
    let export = export_with(json!({
        "workouts": [
            { "workout_id": "w1", "effective_type": "yoga",
              "route": [], "samples": [], "splits": [], "pauses": [] }
        ]
    }));
    let error = to_fit(&export).unwrap_err();
    assert!(error.contains("no workout details to export"), "actual error: {error}");
}

#[test]
fn an_unmapped_sport_falls_back_to_generic_rather_than_a_near_miss() {
    assert_eq!(map_sport("tug_of_war").0 .0, typedef::Sport::GENERIC.0);
    assert_eq!(map_sport("").0 .0, typedef::Sport::GENERIC.0);
    // Trail running has a real match and must not fall to GENERIC. This is the one that issue #24
    // reported as wrongly detected as open water swimming.
    assert_eq!(map_sport("trail_running").1 .0, typedef::SubSport::TRAIL.0);
    // The types new in the 3.1.1 catalog all have their own FIT match.
    assert_eq!(map_sport("padel").1 .0, typedef::SubSport::PADEL.0);
    assert_eq!(
        map_sport("alpine_skiing").0 .0,
        typedef::Sport::ALPINE_SKIING.0
    );
    assert_eq!(
        map_sport("stair_climbing").0 .0,
        typedef::Sport::FLOOR_CLIMBING.0
    );
    assert_eq!(map_sport("mtb").1 .0, typedef::SubSport::MOUNTAIN.0);
}

#[test]
fn two_workouts_starting_in_the_same_second_do_not_overwrite_each_other() {
    let one = json!({
        "workout_id": "w1", "effective_type": "run",
        "start_time": "2026-08-24T06:00:00+08:00",
        "route": [], "splits": [], "pauses": [],
        "samples": [{ "timestamp": "2026-08-24T06:00:00+08:00", "heart_rate": 120 }]
    });
    let export = export_with(json!({ "workouts": [one.clone(), one] }));

    let (files, _) = to_fit(&export).unwrap();
    assert_eq!(files.len(), 2);
    assert_ne!(files[0].0, files[1].0, "files with the same name must not overwrite each other");
}

#[test]
fn an_out_of_range_altitude_is_dropped_instead_of_clamped() {
    assert_eq!(encode_altitude(12.4), Some(2562));
    assert_eq!(encode_altitude(-20000.0), None, "the altitude sentinel must not be written as a fake altitude");
    assert_eq!(encode_altitude(f64::NAN), None);
}
/// Pins each item in the comparison report's list of "the Zepp file carries more".
///
/// The data for these fields was always in the database but was not written to the file. Each has a different scale / offset.
/// A wrong one gives no error; the reader only sees a number of the wrong magnitude that still looks plausible. So this test
/// does not assert "has a value". It asserts "decodes back to the real reading we put in".
#[test]
fn the_session_carries_stride_power_altitude_effect_and_hr_zones() {
    let export = export_with(json!({
        "workouts": [{
            "workout_id": "1",
            "effective_type": "run",
            "start_time": "2026-08-26T01:57:30+00:00",
            "end_time": "2026-08-26T02:00:30+00:00",
            "distance_meters": 900.0,
            "avg_stride_cm": 106.0,
            "max_altitude_m": 58.2,
            "min_altitude_m": 28.4,
            "training_effect": 5.0,
            "anaerobic_training_effect": 2.5,
            "hr_zones": [
                { "index": 0, "upper_bound_bpm": 113, "seconds": 0 },
                { "index": 1, "upper_bound_bpm": 141, "seconds": 31 },
                { "index": 2, "upper_bound_bpm": 154, "seconds": 346 },
                { "index": 3, "upper_bound_bpm": 162, "seconds": 4734 }
            ],
            "samples": [
                { "timestamp": "2026-08-26T01:57:30+00:00", "heart_rate": 150,
                  "power_watts": 200.0, "stride_cm": 100.0, "altitude_m": 30.0 },
                { "timestamp": "2026-08-26T01:57:31+00:00", "heart_rate": 152,
                  "power_watts": 300.0, "stride_cm": 110.0, "altitude_m": 50.0 }
            ],
            "route": []
        }]
    }));
    let (files, _) = to_fit(&export).expect("export should succeed");
    let fit = decode(&files[0].1);
    let session = messages_of(&fit, typedef::MesgNum::SESSION);
    assert_eq!(session.len(), 1);

    // avg_step_length: u16, scale 10, unit mm -> 106 cm = 1060 mm -> 10600
    assert_eq!(
        int_of(session[0], mesgdef::Session::AVG_STEP_LENGTH),
        Some(10600),
        "stride length is stored as mm times ten"
    );
    // Power is in watts, no scale.
    assert_eq!(int_of(session[0], mesgdef::Session::AVG_POWER), Some(250));
    assert_eq!(int_of(session[0], mesgdef::Session::MAX_POWER), Some(300));
    // Altitude: (m + 500) x 5
    assert_eq!(
        int_of(session[0], mesgdef::Session::MAX_ALTITUDE),
        Some(((58.2_f64 + 500.0) * 5.0).round() as i64)
    );
    assert_eq!(
        int_of(session[0], mesgdef::Session::MIN_ALTITUDE),
        Some(((28.4_f64 + 500.0) * 5.0).round() as i64)
    );
    // Sample mean altitude (30 + 50) / 2 = 40
    assert_eq!(
        int_of(session[0], mesgdef::Session::AVG_ALTITUDE),
        Some(((40.0_f64 + 500.0) * 5.0).round() as i64)
    );
    // Training effect: u8, scale 10
    assert_eq!(
        int_of(session[0], mesgdef::Session::TOTAL_TRAINING_EFFECT),
        Some(50)
    );
    assert_eq!(
        int_of(
            session[0],
            mesgdef::Session::TOTAL_ANAEROBIC_TRAINING_EFFECT
        ),
        Some(25)
    );
}

/// Heart rate zones must be placed by `index`.
///
/// If one zone in the middle is missing, flat order shifts every later zone one forward, and the shifted numbers
/// still look fully plausible. Nothing reports an error. So this case has its own test.
#[test]
fn hr_zone_buckets_land_on_their_own_index_even_with_a_gap() {
    let export = export_with(json!({
        "workouts": [{
            "workout_id": "1",
            "effective_type": "run",
            "start_time": "2026-08-26T01:57:30+00:00",
            "end_time": "2026-08-26T01:58:30+00:00",
            "hr_zones": [
                { "index": 0, "upper_bound_bpm": 113, "seconds": 10 },
                { "index": 3, "upper_bound_bpm": 162, "seconds": 40 }
            ],
            "samples": [
                { "timestamp": "2026-08-26T01:57:30+00:00", "heart_rate": 150 },
                { "timestamp": "2026-08-26T01:57:31+00:00", "heart_rate": 152 }
            ],
            "route": []
        }]
    }));
    let (files, _) = to_fit(&export).expect("export should succeed");
    let fit = decode(&files[0].1);
    let session = messages_of(&fit, typedef::MesgNum::SESSION);
    let zones = session[0]
        .fields
        .iter()
        .find(|field| field.num == mesgdef::Session::TIME_IN_HR_ZONE)
        .expect("heart rate zones should be written");
    // The unit is ms (scale 1000). Zones 1 and 2 have no reading, so they are 0, not filled by 40.
    match &zones.value {
        FitValue::VecUint32(values) => {
            assert_eq!(values.as_slice(), &[10_000, 0, 0, 40_000]);
        }
        other => panic!("heart rate zones should be a u32 array, got {other:?}"),
    }
}

/// Stride length on each record. The database has it for every second, but FIT always had it empty before.
#[test]
fn each_record_carries_its_own_step_length() {
    let export = export_with(json!({
        "workouts": [{
            "workout_id": "1",
            "effective_type": "run",
            "start_time": "2026-08-26T01:57:30+00:00",
            "end_time": "2026-08-26T01:58:30+00:00",
            "samples": [
                { "timestamp": "2026-08-26T01:57:30+00:00", "heart_rate": 150, "stride_cm": 98.5 }
            ],
            "route": []
        }]
    }));
    let (files, _) = to_fit(&export).expect("export should succeed");
    let fit = decode(&files[0].1);
    let records = messages_of(&fit, typedef::MesgNum::RECORD);
    assert_eq!(records.len(), 1);
    // step_length: u16, scale 10, unit mm -> 98.5 cm = 985 mm -> 9850
    assert_eq!(int_of(records[0], mesgdef::Record::STEP_LENGTH), Some(9850));
}

/// A workout without heart rate zones does not write this field, instead of writing an empty array.
#[test]
fn a_workout_without_hr_zones_writes_no_zone_field() {
    let export = export_with(json!({
        "workouts": [{
            "workout_id": "1",
            "effective_type": "run",
            "start_time": "2026-08-26T01:57:30+00:00",
            "end_time": "2026-08-26T01:58:30+00:00",
            "samples": [
                { "timestamp": "2026-08-26T01:57:30+00:00", "heart_rate": 150 }
            ],
            "route": []
        }]
    }));
    let (files, _) = to_fit(&export).expect("export should succeed");
    let fit = decode(&files[0].1);
    let session = messages_of(&fit, typedef::MesgNum::SESSION);
    assert!(session[0]
        .fields
        .iter()
        .all(|field| field.num != mesgdef::Session::TIME_IN_HR_ZONE));
}

/// lat=999 is not a point on Earth: the record has no position fields (heart rate is still written),
/// and the session/lap start position can only come from the valid point.
#[test]
fn out_of_domain_coordinates_are_dropped_before_semicircle_conversion() {
    let export = export_with(json!({
        "workouts": [{
            "workout_id": "w1",
            "effective_type": "run",
            "start_time": "2026-08-24T06:00:00+08:00",
            "end_time": "2026-08-24T06:00:02+08:00",
            "route": [
                { "timestamp": "2026-08-24T06:00:00+08:00", "latitude": 999.0,
                  "longitude": 121.0 },
                { "timestamp": "2026-08-24T06:00:01+08:00", "latitude": 31.0,
                  "longitude": 121.0 }
            ],
            "samples": [
                { "timestamp": "2026-08-24T06:00:00+08:00", "heart_rate": 132 },
                { "timestamp": "2026-08-24T06:00:01+08:00", "heart_rate": 134 }
            ],
            "splits": [],
            "pauses": []
        }]
    }));
    let (files, _) = to_fit(&export).expect("export should succeed");
    let fit = decode(&files[0].1);
    let records = messages_of(&fit, typedef::MesgNum::RECORD);
    assert_eq!(records.len(), 2, "each of the two samples still writes one record");
    assert_eq!(
        int_of(records[0], mesgdef::Record::POSITION_LAT),
        None,
        "lat=999 must not enter a record"
    );
    assert_eq!(int_of(records[0], mesgdef::Record::POSITION_LONG), None);
    assert_eq!(
        int_of(records[0], mesgdef::Record::HEART_RATE),
        Some(132),
        "bad coordinates do not mean the whole sample is gone"
    );
    let expected = (31.0 * SEMICIRCLES_PER_DEGREE).round() as i64;
    assert!(
        (int_of(records[1], mesgdef::Record::POSITION_LAT).unwrap() - expected).abs() <= 1,
        "valid point coordinates are written as usual"
    );
    let session = messages_of(&fit, typedef::MesgNum::SESSION);
    assert_eq!(
        int_of(session[0], mesgdef::Session::START_POSITION_LAT),
        Some(expected),
        "the start position must come from the valid point, not from lat=999"
    );
    let lap = messages_of(&fit, typedef::MesgNum::LAP);
    assert_eq!(
        int_of(lap[0], mesgdef::Lap::START_POSITION_LAT),
        Some(expected)
    );
}
