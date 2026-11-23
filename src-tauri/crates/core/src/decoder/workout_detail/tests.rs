use super::*;
use serde_json::json;

#[test]
fn decodes_documented_gps_deltas() {
    let raw = json!({
        "trackid": 1_700_000_000i64,
        "source": "run.gps",
        "time": "0;2;2;",
        "longitude_latitude": "4004663552,11629333504;16403,8392;;;;14877,8392;",
        "altitude": "-2000000;7800;7772;",
        "heart_rate": "11,80;0,10;7,-6;",
        "speed": "2,1.20;4,2.45;",
        "gait": "2,0,71,160;2,2,74,164;",
        "pause": "1700000060,10,1,2,2;"
    });
    let decoded = decode_workout_detail(
        &raw,
        Some(Utc.timestamp_opt(1_700_000_030, 0).single().unwrap()),
        None,
    )
    .unwrap();
    assert_eq!(decoded.track_id, 1_700_000_000);
    assert_eq!(decoded.route.len(), 3);
    assert!((decoded.route[0].latitude - 40.04663552).abs() < 1e-8);
    assert!((decoded.route[0].longitude - 116.29333504).abs() < 1e-8);
    let second = &decoded.route[1];
    assert!((second.latitude - (40.04663552 + 16403.0 / COORD_FACTOR)).abs() < 1e-8);
    assert_eq!(decoded.route[0].altitude_m, Some(78.0));
    assert!(decoded
        .samples
        .iter()
        .any(|sample| sample.heart_rate == Some(80)));
    assert!(decoded
        .samples
        .iter()
        .any(|sample| sample.heart_rate == Some(84)));
    assert_eq!(decoded.pauses.len(), 1);
    assert_eq!(decoded.pauses[0].kind, "manual");
    assert!(decoded
        .samples
        .iter()
        .any(|sample| sample.stride_cm == Some(71.0)));
}

#[test]
fn altitude_sentinel_variants_are_never_terrain() {
    // Real payloads lead with -2000000 *plus a tail* (-2002110, -2003943
    // observed). An equality guard let those through as ~-20000 m samples.
    for sentinel in ["-2000000", "-2002110", "-2003943"] {
        let raw = json!({
            "trackid": 1_700_000_000i64,
            "time": "0;1;1;",
            "longitude_latitude": "4004663552,11629333504;1,1;1,1;",
            "altitude": format!("{sentinel};1451;1448;"),
        });
        let decoded = decode_workout_detail(&raw, None, None).unwrap();
        // The leading sentinel is backfilled from the first plausible
        // reading, never divided by 100 into terrain.
        assert_eq!(decoded.route[0].altitude_m, Some(14.51), "{sentinel}");
        assert!(
            decoded
                .samples
                .iter()
                .filter_map(|sample| sample.altitude_m)
                .all(|meters| (-1000.0..=10000.0).contains(&meters)),
            "{sentinel} leaked an implausible altitude"
        );
    }
}

#[test]
fn time_delta_altitude_wins_over_the_index_aligned_list() {
    let raw = json!({
        "trackid": 1_700_000_000i64,
        "time": "0;1;1;",
        "longitude_latitude": "4004663552,11629333504;1,1;1,1;",
        "altitude": "-2002110;9900;9900;",
        "time_delta_altitude": "1,3516;1,3518;1,3521;",
    });
    let decoded = decode_workout_detail(&raw, None, None).unwrap();
    let altitudes: Vec<Option<f64>> = decoded.route.iter().map(|point| point.altitude_m).collect();
    assert!(
        altitudes.contains(&Some(35.16)) || altitudes.contains(&Some(35.18)),
        "expected the pair series to supply altitude, got {altitudes:?}"
    );
    assert!(!altitudes.contains(&Some(99.0)));
}

#[test]
fn splits_come_from_the_servers_cumulative_distance() {
    // Integrating the per-second speed instead is 0.15% out on a run but
    // 12.6% out on a ride, so splits must read `currentDistance`.
    //
    // One reading every 5 seconds: a "jump" of 400 m per second is a bad point for the monotonic filter,
    // and that gate must pass first. The last reading, standing still, gives the partial km a real end time.
    let raw = json!({
        "trackid": 1_700_000_000i64,
        "time": "0;5;5;5;5;5;",
        "currentDistance": "0,0;5,40000;5,100000;5,160000;5,240000;5,240000;",
        "heart_rate": "0,150;5,10;5,0;5,-10;5,0;",
        "time_delta_altitude": "1,1000;5,1200;5,1100;5,1300;",
    });
    let decoded = decode_workout_detail(&raw, None, None).unwrap();
    let splits = &decoded.splits;
    assert_eq!(splits.len(), 3, "two full kilometres and a remainder");
    assert_eq!(splits[0].index, 1);
    assert_eq!(splits[1].index, 2);
    assert!(!splits[0].partial);
    assert!(!splits[1].partial);
    assert_eq!(splits[0].distance_m, 1000.0);
    assert_eq!(splits[1].distance_m, 1000.0);

    // The trailing 400 m is flagged, so it is never read as a slow
    // kilometre.
    assert!(splits[2].partial);
    assert_eq!(splits[2].distance_m, 400.0);

    // Pace is only defined where distance and time both moved.
    assert!(splits[0].pace_min_per_km.unwrap() > 0.0);
    assert!(splits[0].avg_hr.is_some());
    assert!(splits[0].max_hr.unwrap() >= splits[0].avg_hr.unwrap());
}

#[test]
fn a_workout_without_distance_reports_no_splits() {
    // Silence is the honest answer: an indoor session with no distance
    // series must not get kilometres invented for it.
    let raw = json!({
        "trackid": 1_700_000_100i64,
        "time": "1;1;1;",
        "heart_rate": "1,120;1,2;1,1;"
    });
    let decoded = decode_workout_detail(&raw, None, None).unwrap();
    assert!(decoded.splits.is_empty());
}

#[test]
fn indoor_without_gps_has_no_route() {
    let raw = json!({
        "trackid": 1_700_000_100i64,
        "time": "1;1;1;",
        "heart_rate": "1,120;1,2;1,-1;"
    });
    let decoded = decode_workout_detail(&raw, None, None).unwrap();
    assert!(decoded.route.is_empty());
    assert!(!decoded.samples.is_empty());
    assert!(decoded
        .samples
        .iter()
        .any(|sample| sample.heart_rate == Some(121)));
}

#[test]
fn leading_negative_delta_is_the_heart_rate_baseline() {
    // Shape of a real payload (one "AI-detected activity"): the first item has time delta -1 and the value is the baseline heart rate,
    // and the rest are deltas. If the baseline is dropped, the curve starts at 1 and peaks at only a few dozen.
    let raw = json!({
        "trackid": 1_700_000_300i64,
        "time": "1;1;1;1;",
        "heart_rate": "-1,105;,1;,1;,-1;,2;"
    });
    let decoded = decode_workout_detail(&raw, None, None).unwrap();
    let rates: Vec<i32> = decoded
        .samples
        .iter()
        .filter_map(|sample| sample.heart_rate)
        .collect();
    assert!(!rates.is_empty());
    assert_eq!(rates.first(), Some(&106));
    assert_eq!(rates.iter().max(), Some(&108));
    assert!(rates.iter().all(|rate| *rate >= 100), "{rates:?}");
}

#[test]
fn negative_delta_after_the_first_is_still_skipped() {
    // Only the first item is the baseline. A negative span in the middle is still a bad point and must not rewrite the base.
    let raw = json!({
        "trackid": 1_700_000_400i64,
        "time": "1;1;1;",
        "heart_rate": "0,120;1,2;-3,50;1,1;"
    });
    let decoded = decode_workout_detail(&raw, None, None).unwrap();
    let rates: Vec<i32> = decoded
        .samples
        .iter()
        .filter_map(|sample| sample.heart_rate)
        .collect();
    assert_eq!(rates.iter().max(), Some(&123));
}

#[test]
fn missing_trackid_is_an_error() {
    let raw = json!({ "time": "1;1;" });
    assert!(decode_workout_detail(&raw, None, None).is_err());
}

#[test]
fn empty_heart_rate_does_not_invent_zeros() {
    let raw = json!({
        "data": {
            "trackid": 1_700_000_200i64,
            "time": "1;1;",
            "longitude_latitude": "1,1;2,2;"
        }
    });
    let decoded = decode_workout_detail(&raw, None, None).unwrap();
    assert!(decoded
        .samples
        .iter()
        .all(|sample| sample.heart_rate.is_none()));
    assert_eq!(decoded.route.len(), 2);
}

/// A garbled delta must remove the whole row. It must not become 0 seconds.
///
/// The result of 0 is not "one sample fewer". It is that **this sample lands on the same
/// timestamp as the previous one**. The sample count looks right, but pace and power are wrong, and the UI shows
/// no sign of it.
#[test]
fn a_malformed_delta_drops_the_row_instead_of_becoming_zero() {
    // The delta of the second pair is `x`: unreadable.
    let pairs = parse_delta_pairs(Some(&json!("5,80;x,90;3,100;")), true);
    assert_eq!(pairs, vec![(5, 80), (3, 100)], "unreadable rows must be dropped");

    // An empty delta is still "the next second" per the protocol, and is not affected.
    let with_empty = parse_delta_pairs(Some(&json!("5,80;,90;")), true);
    assert_eq!(with_empty, vec![(5, 80), (1, 90)]);

    // The same for the float path.
    let valued = parse_valued_pairs(Some(&json!("2,180.5;oops,200.0;4,210.25;")));
    assert_eq!(valued, vec![(2, 180.5), (4, 210.25)]);
}
/// When `currentDistance` is empty, km splits come from the cloud's own `kilo_pace`.
///
/// 130 of 336 details in our data are like this: they have `kilo_pace` but no `currentDistance`,
/// so no split came out and the exported .fit had only one lap for the whole run. The GitHub comparison report
/// line "run.fit: 1 lap, Zepp: 8 laps" is about this.
#[test]
fn kilo_pace_fills_in_splits_when_cumulative_distance_is_missing() {
    // Three km at 300 / 310 / 290 seconds. Column 6 is cumulative seconds, which proves the columns were read correctly.
    //
    // `time` must cover the full time that kilo_pace claims: a workout that lasts only 4 seconds cannot
    // have 900 seconds of km, and such a fixture proves nothing. Here 0/300/310/290/100
    // add up to 1000 seconds, 100 seconds more than three km. Those 100 seconds are the last partial km.
    let raw = json!({
        "trackid": 1_700_000_000i64,
        "time": "0;300;310;290;100;",
        "currentDistance": "",
        "kilo_pace": "0,300,wtmkqm0228e3,1,-1,300,300266,69,0,109,0,4,0,164;\
    1,310,wtmkqnm0p17s,1,-1,610,310237,71,0,107,0,2,0,164;\
    2,290,wtmkqr93gqgr,1,-1,900,290246,69,0,105,0,1,0,160;",
        "heart_rate": "0,150;300,10;310,0;290,-10;100,0;",
    });
    let decoded = decode_workout_detail(&raw, None, Some(3400.0)).unwrap();
    let splits = &decoded.splits;
    assert_eq!(splits.len(), 4, "three whole km plus one partial");
    assert_eq!(splits[0].duration_seconds, 300);
    assert_eq!(splits[1].duration_seconds, 310);
    assert_eq!(splits[2].duration_seconds, 290);
    for split in splits.iter().take(3) {
        assert_eq!(split.distance_m, 1000.0);
        assert!(!split.partial);
    }
    // The partial km length comes from the summary distance, not a guess: 3400 - 3000 = 400.
    assert!(splits[3].partial, "the last segment must be marked partial");
    assert_eq!(splits[3].distance_m, 400.0);
    assert_eq!(
        splits.iter().map(|split| split.distance_m).sum::<f64>(),
        3400.0,
        "split distances must add up to the total distance, with no silent loss"
    );
}

/// If the columns do not match, reject the whole record instead of trusting half of it.
///
/// `kilo_pace` columns move between records: at the same width, column 4 is average
/// heart rate in some records and column 13 in others. So the check is not the width but self-consistency:
/// "row numbers are continuous and per-lap seconds add up to the cumulative column". If that fails, the columns were misread.
#[test]
fn a_kilo_pace_series_that_does_not_add_up_is_refused() {
    let inconsistent = json!({
        "trackid": 1_700_000_000i64,
        "time": "0;1;1;",
        "currentDistance": "",
        // The cumulative column says 300 / 999: row two is 300 + 310 = 610, which does not match 999.
        "kilo_pace": "0,300,geo,1,-1,300,300266,69,0,109,0,4,0,164;\
    1,310,geo,1,-1,999,310237,71,0,107,0,2,0,164;",
        "heart_rate": "0,150;1,10;1,0;",
    });
    let decoded = decode_workout_detail(&inconsistent, None, Some(2000.0)).unwrap();
    assert!(decoded.splits.is_empty(), "on a mismatch keep nothing; one lap for the whole workout is better");

    // The same for a gap in row numbers.
    let out_of_order = json!({
        "trackid": 1_700_000_000i64,
        "time": "0;1;1;",
        "currentDistance": "",
        "kilo_pace": "0,300,geo,1,-1,300,300266,69,0,109,0,4,0,164;\
    5,310,geo,1,-1,610,310237,71,0,107,0,2,0,164;",
        "heart_rate": "0,150;1,10;1,0;",
    });
    assert!(decode_workout_detail(&out_of_order, None, Some(2000.0))
        .unwrap()
        .splits
        .is_empty());
}

/// When `currentDistance` exists, keep it: that path is finer, cut per second, and the fallback must not replace it.
#[test]
fn cumulative_distance_still_wins_when_it_is_present() {
    // The same 5-second fixture as above: splits must still be cut from `currentDistance`,
    // and the 999 seconds from kilo_pace must not appear.
    let raw = json!({
        "trackid": 1_700_000_000i64,
        "time": "0;5;5;5;5;5;",
        "currentDistance": "0,0;5,40000;5,100000;5,160000;5,240000;5,240000;",
        // A kilo_pace that does not match the data above, on purpose: it must not be used.
        "kilo_pace": "0,999,geo,1,-1,999,999000,69,0,109,0,4,0,164;",
        "heart_rate": "0,150;5,10;5,0;5,-10;5,0;",
    });
    let decoded = decode_workout_detail(&raw, None, Some(2400.0)).unwrap();
    assert_eq!(decoded.splits.len(), 3);
    assert_ne!(decoded.splits[0].duration_seconds, 999);
}

/// When the summary has no distance (indoor, no GPS), add no partial km instead of inventing a length.
#[test]
fn without_a_summary_distance_the_remainder_is_left_out_rather_than_invented() {
    let raw = json!({
        "trackid": 1_700_000_000i64,
        "time": "0;1;1;",
        "currentDistance": "",
        "kilo_pace": "0,300,geo,1,-1,300,300266,69,0,109,0,4,0,164;",
        "heart_rate": "0,150;1,10;1,0;",
    });
    let decoded = decode_workout_detail(&raw, None, None).unwrap();
    assert_eq!(decoded.splits.len(), 1, "only the one whole km");
    assert!(!decoded.splits[0].partial);
    assert_eq!(decoded.splits[0].distance_m, 1000.0);
}
/// Laps recorded by the watch come from the `lap` field.
///
/// This field was always in the stored payloads (32 of 336 local details have it), but was never decoded.
/// The GitHub .fit comparison report asked for exactly this: "I press the lap button often. If your
/// .fit could carry lap data, that would be my first choice."
#[test]
fn watch_recorded_laps_are_read_from_the_lap_field() {
    let raw = json!({
        "trackid": 1_700_000_000i64,
        "time": "0;300;300;",
        "heart_rate": "0,150;300,2;300,-2;",
        // Two laps, 500 m / 300 s each. Column 6 is cumulative seconds.
        "lap": "0,300,500,s00000000000,150,300,-20000;\
    1,300,500,s00000000000,152,600,-20000;",
    });
    let decoded = decode_workout_detail(&raw, None, Some(1000.0)).unwrap();
    assert_eq!(decoded.laps.len(), 2);
    assert_eq!(decoded.laps[0].index, 1);
    assert_eq!(decoded.laps[0].distance_m, 500.0);
    assert_eq!(decoded.laps[0].duration_seconds, 300);
    assert_eq!(decoded.laps[0].avg_hr, Some(150));
    // The second lap starts where the first lap ended, not at the workout start.
    assert_eq!(decoded.laps[1].start_time, decoded.laps[0].end_time);
    assert_eq!(decoded.laps[1].duration_seconds, 300);
}

/// Laps and per-km splits are different things. Neither replaces the other.
///
/// One 5820 m run can have 5 km splits and 14 laps of 415 m at the same time. If they share one
/// table, or one overwrites the other, the "pace per km" chart suddenly shows something else.
#[test]
fn laps_and_kilometre_splits_coexist() {
    let raw = json!({
        "trackid": 1_700_000_000i64,
        "time": "0;300;300;",
        "currentDistance": "0,0;300,50000;300,100000;",
        "heart_rate": "0,150;300,2;300,-2;",
        "lap": "0,300,500,s00000000000,150,300,-20000;\
    1,300,500,s00000000000,152,600,-20000;",
    });
    let decoded = decode_workout_detail(&raw, None, Some(1000.0)).unwrap();
    assert_eq!(decoded.laps.len(), 2, "the watch laps");
    assert!(!decoded.splits.is_empty(), "our own km splits are still there");
}

/// If laps do not match the summary, drop all of them instead of trusting half.
///
/// These are the two strongest checks: lap distances must add up to the workout distance, and the last lap must end at the
/// workout end. Misread columns fail both checks, and the 32 real records pass both.
#[test]
fn laps_that_do_not_add_up_to_the_workout_are_refused() {
    // The lap distances add up to 1000 m, but the summary says this run was 5000 m.
    let wrong_distance = json!({
        "trackid": 1_700_000_000i64,
        "time": "0;300;300;",
        "heart_rate": "0,150;300,2;300,-2;",
        "lap": "0,300,500,s00000000000,150,300,-20000;\
    1,300,500,s00000000000,152,600,-20000;",
    });
    assert!(
        decode_workout_detail(&wrong_distance, None, Some(5000.0))
            .unwrap()
            .laps
            .is_empty(),
        "if the distance does not match, keep nothing"
    );

    // A gap in row numbers.
    let out_of_order = json!({
        "trackid": 1_700_000_000i64,
        "time": "0;300;300;",
        "heart_rate": "0,150;300,2;300,-2;",
        "lap": "0,300,500,s00000000000,150,300,-20000;\
    7,300,500,s00000000000,152,600,-20000;",
    });
    assert!(decode_workout_detail(&out_of_order, None, Some(1000.0))
        .unwrap()
        .laps
        .is_empty());

    // Cumulative seconds go backward.
    let backwards = json!({
        "trackid": 1_700_000_000i64,
        "time": "0;300;300;",
        "heart_rate": "0,150;300,2;300,-2;",
        "lap": "0,300,500,s00000000000,150,600,-20000;\
    1,300,500,s00000000000,152,300,-20000;",
    });
    assert!(decode_workout_detail(&backwards, None, Some(1000.0))
        .unwrap()
        .laps
        .is_empty());
}

/// No `lap` field is normal, not an error: only 32 of 336 details have it.
#[test]
fn a_workout_without_laps_is_not_an_error() {
    let raw = json!({
        "trackid": 1_700_000_000i64,
        "time": "0;300;",
        "heart_rate": "0,150;300,2;",
        "lap": "",
    });
    let decoded = decode_workout_detail(&raw, None, Some(1000.0)).unwrap();
    assert!(decoded.laps.is_empty());
}

/// Heart rate 0 means "not measured for this lap", not "heart rate is 0".
#[test]
fn a_zero_heart_rate_on_a_lap_is_absent_not_zero() {
    let raw = json!({
        "trackid": 1_700_000_000i64,
        "time": "0;300;",
        "heart_rate": "0,150;300,2;",
        "lap": "0,300,1000,s00000000000,0,300,-20000;",
    });
    let decoded = decode_workout_detail(&raw, None, Some(1000.0)).unwrap();
    assert_eq!(decoded.laps.len(), 1);
    assert_eq!(decoded.laps[0].avg_hr, None);
}

/// A trackid / time delta in a bad payload must not crash the decoder.
///
/// `Duration::seconds` and `DateTime + Duration` both panic when out of range. Here only
/// a ParseError or a duration clamped to 48 hours is allowed.
#[test]
fn huge_track_id_or_time_deltas_do_not_panic() {
    let huge_id = json!({
        "trackid": i64::MAX,
        "time": "1;1;",
    });
    let err = decode_workout_detail(&huge_id, None, None).expect_err("i64::MAX is not a valid unix time");
    assert!(
        matches!(err, ZeppBridgeError::ParseError(_)),
        "expected ParseError, got {err:?}"
    );

    let huge_time = ["2147483647"; 10].join(";");
    let huge_deltas = json!({
        "trackid": 1_700_000_000i64,
        "time": huge_time,
    });
    let decoded =
        decode_workout_detail(&huge_deltas, None, None).expect("duration must clamp to 48h instead of panicking");
    let span = decoded
        .end_time
        .timestamp()
        .saturating_sub(decoded.start_time.timestamp());
    assert!(span <= MAX_ACTIVITY_SECONDS, "duration {span} is over the 48h limit");

    let near_max = DateTime::<Utc>::MAX_UTC;
    let result = decode_workout_detail(
        &json!({
            "trackid": 1_700_000_000i64,
            "time": "1;",
        }),
        Some(near_max),
        None,
    );
    assert!(
        result.is_ok() || matches!(result, Err(ZeppBridgeError::ParseError(_))),
        "a very large summary end time must not panic: {result:?}"
    );
}

/// An unreadable segment in the delta chain means the position is unknown from there: cut, do not shift.
///
/// Coordinates are cumulative deltas. Skipping a bad segment and continuing gives a fake track with a normal shape
/// and a global offset. The map shows nothing wrong, but every point is wrong.
#[test]
fn a_broken_coordinate_delta_truncates_the_route_instead_of_shifting_it() {
    let raw = json!({
        "trackid": 1_700_000_000i64,
        "time": "0;2;2;2;",
        "longitude_latitude": "4004663552,11629333504;16403,8392;abc,8392;14877,8392;",
    });
    let decoded = decode_workout_detail(&raw, None, None).unwrap();
    assert_eq!(decoded.route.len(), 2, "no point after the bad segment may be output");
    assert_eq!(decoded.route_dropped_points, 2);
    // Point 2 is "origin + first delta" and was not shifted by the bad segment.
    let second = &decoded.route[1];
    assert!((second.latitude - (40.04663552 + 16403.0 / COORD_FACTOR)).abs() < 1e-8);
    assert!((second.longitude - (116.29333504 + 8392.0 / COORD_FACTOR)).abs() < 1e-8);
}

/// A sum that leaves the Earth's surface is also cut. Every later coordinate is fiction.
#[test]
fn a_coordinate_that_leaves_the_earth_truncates_the_route() {
    let raw = json!({
        "trackid": 1_700_000_000i64,
        "time": "0;1;1;",
        // The second delta pushes the latitude past 90 degrees.
        "longitude_latitude": "4004663552,11629333504;6000000000,0;1000,1000;",
    });
    let decoded = decode_workout_detail(&raw, None, None).unwrap();
    assert_eq!(decoded.route.len(), 1);
    assert_eq!(decoded.route_dropped_points, 2);
    assert!((decoded.route[0].latitude - 40.04663552).abs() < 1e-8);
}

/// The same for an unreadable delta in `time`: after it even the time is unknown, so the duration must not grow.
#[test]
fn a_broken_time_delta_truncates_both_route_and_duration() {
    let raw = json!({
        "trackid": 1_700_000_000i64,
        "time": "0;2;oops;2;",
        "longitude_latitude": "4004663552,11629333504;16403,8392;100,100;100,100;",
    });
    let decoded = decode_workout_detail(&raw, None, None).unwrap();
    assert_eq!(decoded.route.len(), 2);
    assert_eq!(decoded.route_dropped_points, 2);
    let span = decoded.end_time.timestamp() - decoded.start_time.timestamp();
    assert_eq!(span, 2, "duration sums only up to the first bad delta");
}

/// Per-second heart rate uses the lap bound 0..=250: 300 is poor contact or a sensor fault, not a heart rate.
#[test]
fn a_heart_rate_sample_above_the_plausible_ceiling_is_dropped() {
    let raw = json!({
        "trackid": 1_700_000_000i64,
        "time": "0;1;1;",
        // Cumulative deltas: 300 -> 150 -> 151.
        "heart_rate": "0,300;1,-150;1,1;",
    });
    let decoded = decode_workout_detail(&raw, None, None).unwrap();
    assert_eq!(decoded.samples[0].heart_rate, None, "300 must be dropped");
    assert_eq!(decoded.samples[1].heart_rate, Some(150));
    assert_eq!(decoded.samples[2].heart_rate, Some(151));
}

/// Going back and huge jumps in cumulative distance are bad points: drop the point, never cut tens of thousands of 0-duration splits.
#[test]
fn corrupt_cumulative_distance_points_do_not_create_thousands_of_empty_splits() {
    let raw = json!({
        "trackid": 1_700_000_000i64,
        "time": "0;300;300;300;300;300;",
        // One step back (1500 -> 800) and one huge jump (800 -> 21,000,000 m).
        // Both bad points must be dropped, and neither may cut a 0-duration split.
        "currentDistance": "0,0;300,150000;300,80000;300,2100000000;300,240000;",
        // Two whole km, 300 seconds each. The row count matches the summary 2400 m.
        "kilo_pace": "0,300,geo,1,-1,300,300266,69,0,109,0,4,0,164;\
    1,300,geo,1,-1,600,300266,69,0,109,0,4,0,164;",
        "heart_rate": "0,150;300,2;300,-2;300,1;300,0;",
    });
    let decoded = decode_workout_detail(&raw, None, Some(2400.0)).unwrap();
    assert!(
        !decoded.splits.is_empty(),
        "after currentDistance is judged bad, use the kilo_pace fallback"
    );
    assert!(
        decoded
            .splits
            .iter()
            .all(|split| split.duration_seconds > 0),
        "0-duration splits are not allowed: {:?}",
        decoded
            .splits
            .iter()
            .map(|split| split.duration_seconds)
            .collect::<Vec<_>>()
    );
    assert!(
        decoded.splits.len() <= 2400 / 1000 + 2,
        "split count must be on the scale of the summary distance: {}",
        decoded.splits.len()
    );
}

/// If the kilo_pace row count does not match the summary distance, reject all of it, the same as the lap check.
#[test]
fn a_kilo_pace_row_count_that_disagrees_with_the_summary_is_refused() {
    let raw = json!({
        "trackid": 1_700_000_000i64,
        "time": "0;300;300;",
        "currentDistance": "",
        // 3 whole km, but the summary has only 1500 m. The row count does not match the distance, so trust none.
        "kilo_pace": "0,300,geo,1,-1,300,300266,69,0,109,0,4,0,164;\
    1,300,geo,1,-1,600,300266,69,0,109,0,4,0,164;\
    2,300,geo,1,-1,900,300266,69,0,109,0,4,0,164;",
        "heart_rate": "0,150;300,2;300,-2;",
    });
    let decoded = decode_workout_detail(&raw, None, Some(1500.0)).unwrap();
    assert!(
        decoded.splits.is_empty(),
        "if the row count does not match the summary distance, keep nothing: {:?}",
        decoded.splits.len()
    );
}

/// Altitude and GPS share an index: an unreadable segment in the middle must stay empty, and later altitudes must not shift forward.
#[test]
fn an_unparseable_altitude_keeps_index_alignment() {
    let raw = json!({
        "trackid": 1_700_000_000i64,
        "time": "0;1;1;",
        "longitude_latitude": "4004663552,11629333504;16403,8392;14877,8392;",
        "altitude": "7800;oops;7700;",
    });
    let decoded = decode_workout_detail(&raw, None, None).unwrap();
    assert_eq!(decoded.route.len(), 3, "bad altitude does not mean missing track points");
    assert_eq!(decoded.route[0].altitude_m, Some(78.0));
    assert_eq!(decoded.route[1].altitude_m, None, "the unreadable segment is a missing reading");
    assert_eq!(
        decoded.route[2].altitude_m,
        Some(77.0),
        "later altitudes must not shift onto the bad segment"
    );
}

/// A coordinate sum overflow is the same as leaving the Earth: later positions are unknown, so cut.
#[test]
fn a_coordinate_cursor_overflow_truncates_the_route() {
    let raw = json!({
        "trackid": 1_700_000_000i64,
        "time": "0;1;1;",
        "longitude_latitude": format!(
            "4004663552,11629333504;{},0;1000,1000;",
            i64::MAX
        ),
    });
    let decoded = decode_workout_detail(&raw, None, None).unwrap();
    assert_eq!(decoded.route.len(), 1);
    assert_eq!(decoded.route_dropped_points, 2);
    assert!((decoded.route[0].latitude - 40.04663552).abs() < 1e-8);
}

/// #139: one Balance 2 user had 127 details with `pause`, and not one pause was stored.
/// Rows with a kind other than 2 / 3 used to be dropped. Now the kind is only a label and the range is kept.
#[test]
fn pauses_of_any_kind_are_kept() {
    let raw = json!({
        "trackid": 1_700_000_000i64,
        "time": "0;600;",
        "pause": "1700000060,10,-1,-1,1;1700000100,20,-1,-1,4;1700000200,30,-1,-1,2;",
    });
    let decoded = decode_workout_detail(&raw, None, None).unwrap();
    let kinds: Vec<&str> = decoded.pauses.iter().map(|p| p.kind.as_str()).collect();
    assert_eq!(kinds, vec!["other", "other", "manual"]);
    let total: i64 = decoded
        .pauses
        .iter()
        .map(|p| (p.end_time - p.start_time).num_seconds())
        .sum();
    assert_eq!(total, 60);
}

/// A start written in milliseconds, or in seconds since the start, must land on the same time.
#[test]
fn pause_start_in_millis_or_relative_seconds_lands_on_the_same_moment() {
    for pause in ["1700000060000,10000,-1,-1,2;", "60,10,-1,-1,2;"] {
        let raw = json!({ "trackid": 1_700_000_000i64, "time": "0;600;", "pause": pause });
        let decoded = decode_workout_detail(&raw, None, None).unwrap();
        assert_eq!(decoded.pauses.len(), 1, "{pause}");
        assert_eq!(
            decoded.pauses[0].start_time.timestamp(),
            1_700_000_060,
            "{pause}"
        );
        assert_eq!(
            decoded.pauses[0].end_time.timestamp(),
            1_700_000_070,
            "{pause}"
        );
    }
}

/// A pause that falls outside this workout means a misread column, so it is rejected.
#[test]
fn a_pause_before_the_workout_is_dropped() {
    let raw = json!({
        "trackid": 1_700_000_000i64,
        "time": "0;600;",
        "pause": "1600000000,10,-1,-1,2;",
    });
    assert!(decode_workout_detail(&raw, None, None)
        .unwrap()
        .pauses
        .is_empty());
}

/// kilo_pace where `[5]` is cumulative wall clock (with pauses): place boundaries by `[5]`, and count the pause in that km.
#[test]
fn kilo_pace_with_wall_clock_elapsed_places_boundaries_after_pauses() {
    let raw = json!({
        "trackid": 1_700_000_000i64,
        "time": "0;400;400;",
        "currentDistance": "",
        // A 60-second stop in the second km: moving 310 s, wall clock 300 -> 670.
        "pause": "1700000400,60,-1,-1,2;",
        "kilo_pace": "0,300,geo,1,-1,300,300266,69,0,109,0,4,0,164;\
    1,310,geo,1,-1,670,310237,71,0,107,0,2,0,164;",
    });
    let decoded = decode_workout_detail(&raw, None, Some(2000.0)).unwrap();
    assert_eq!(decoded.splits.len(), 2);
    assert_eq!(decoded.splits[0].duration_seconds, 300);
    assert_eq!(decoded.splits[1].end_time.timestamp(), 1_700_000_670);
}

/// When `[5]` is cumulative moving time, push boundaries later by the pauses, so a 60-second stop does not make every boundary a minute early.
#[test]
fn kilo_pace_moving_time_is_shifted_by_earlier_pauses() {
    let raw = json!({
        "trackid": 1_700_000_000i64,
        "time": "0;400;400;",
        "currentDistance": "",
        "pause": "1700000100,60,-1,-1,2;",
        "kilo_pace": "0,300,geo,1,-1,300,300266,69,0,109,0,4,0,164;\
    1,310,geo,1,-1,610,310237,71,0,107,0,2,0,164;",
    });
    let decoded = decode_workout_detail(&raw, None, Some(2000.0)).unwrap();
    assert_eq!(decoded.splits.len(), 2);
    assert_eq!(decoded.splits[0].end_time.timestamp(), 1_700_000_360);
    assert_eq!(decoded.splits[1].end_time.timestamp(), 1_700_000_670);
}

/// The watch's cumulative distance lands only on seconds with a reading. FIT `record.distance` uses it.
#[test]
fn samples_carry_the_watchs_cumulative_distance_only_where_it_was_read() {
    let raw = json!({
        "trackid": 1_700_000_000i64,
        "time": "0;10;",
        "currentDistance": "5,1500;5,3000;",
    });
    let decoded = decode_workout_detail(&raw, None, None).unwrap();
    let with_distance: Vec<(i64, f64)> = decoded
        .samples
        .iter()
        .filter_map(|s| {
            s.distance_m
                .map(|d| (s.timestamp.timestamp() - 1_700_000_000, d))
        })
        .collect();
    assert_eq!(with_distance, vec![(5, 15.0), (10, 30.0)]);
}

/// #159: `currentDistance` written with decimals (`100000.00000`) must also be read,
/// or there is no cumulative distance reading and every km split is lost.
#[test]
fn a_cumulative_distance_written_with_decimals_is_kept() {
    let raw = json!({
        "trackid": 1_700_000_000i64,
        "time": "0;5;5;5;5;5;",
        "currentDistance": "0,0.00000;5,40000.00000;5,100000.00000;5,160000.00000;5,240000.00000;5,240000.00000;",
        "heart_rate": "0,150;5,10;5,0;5,-10;5,0;",
    });
    let decoded = decode_workout_detail(&raw, None, Some(2400.0)).unwrap();
    assert_eq!(decoded.splits.len(), 3, "two whole km plus one partial");
    assert!(decoded
        .samples
        .iter()
        .any(|sample| sample.distance_m == Some(1000.0)));
}

/// #159: `[5]` is 1 second more than `[1]` per km (rounding). Each km is within tolerance,
/// so the record must not be rejected because the total is off by 3 seconds at the third km.
#[test]
fn kilo_pace_one_second_slower_per_kilometre_still_splits() {
    let raw = json!({
        "trackid": 1_700_000_000i64,
        "time": "0;400;400;400;400;",
        "currentDistance": "",
        "kilo_pace": "0,300,geo,1,-1,301,300266,69,0,109,0,4,0,164;\
    1,300,geo,1,-1,602,300266,69,0,109,0,4,0,164;\
    2,300,geo,1,-1,903,300266,69,0,109,0,4,0,164;\
    3,300,geo,1,-1,1204,300266,69,0,109,0,4,0,164;",
    });
    let decoded = decode_workout_detail(&raw, None, Some(4500.0)).unwrap();
    assert_eq!(decoded.splits.len(), 5, "four whole km plus one partial");
    assert!(decoded
        .splits
        .iter()
        .take(4)
        .all(|split| split.duration_seconds == 300));
}
