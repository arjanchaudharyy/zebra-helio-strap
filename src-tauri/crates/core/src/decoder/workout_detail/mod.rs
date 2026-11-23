use crate::models::error::*;
use crate::models::StrengthSetRow;

use chrono::{DateTime, TimeZone, Utc};

use serde::{Deserialize, Serialize};

use serde_json::Value;

mod parse;
mod splits;

use parse::*;
use splits::*;

const COORD_FACTOR: f64 = 100_000_000.0;

/// Zepp marks "no altitude fix" with a large negative sentinel, but real
/// payloads do not use one constant: observed leading values include
/// -2000000, -2002110 and -2003943. An equality test against -2000000 lets the
/// variants through and they land as ~-20000 m samples, so the guard is a
/// plausibility window instead. Bounds are deliberately generous (-1000 m ..
/// 10000 m) so genuine terrain is never discarded.
const MIN_PLAUSIBLE_ALTITUDE_CM: i64 = -100_000;

const MAX_PLAUSIBLE_ALTITUDE_CM: i64 = 1_000_000;

/// The longest duration accepted when decoding one workout.
///
/// This is a defensive limit. It stops a broken timestamp in the payload from stretching `from..to` into an absurd
/// range. It must not also cut real long workouts. The old 12 hours fell right in the middle of what people
/// really do: 100 km trail runs, full triathlons, 24-hour endurance races and long rides often take 12 to 36 hours,
/// and the track points and per-second samples after 12 hours were dropped while the export looked successful.
///
/// 48 hours is still far below the scale any broken timestamp produces (milliseconds read as seconds give tens of thousands of years),
/// so the guard still works after this increase.
const MAX_ACTIVITY_SECONDS: i64 = 48 * 60 * 60;

fn add_seconds(base: DateTime<Utc>, seconds: i64) -> Option<DateTime<Utc>> {
    chrono::Duration::try_seconds(seconds).and_then(|delta| base.checked_add_signed(delta))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RoutePoint {
    #[serde(default)]
    pub segment_start: bool,
    pub timestamp: DateTime<Utc>,
    pub latitude: f64,
    pub longitude: f64,
    pub altitude_m: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkoutSample {
    pub timestamp: DateTime<Utc>,
    pub heart_rate: Option<i32>,
    pub speed: Option<f64>,
    pub pace: Option<f64>,
    pub cadence: Option<f64>,
    pub stride_cm: Option<f64>,
    pub altitude_m: Option<f64>,
    /// Running power in watts, from `power_meter`.
    ///
    /// Verified against the workout summary rather than assumed: the mean of
    /// this series is 249.3 W where the summary reports `average_power` 249.0,
    /// and its maximum is 326 W against `max_power` 326.0 (second workout:
    /// 231.5 / 231.0 and 303 / 303).
    pub power_watts: Option<f64>,
    /// Ground contact time in milliseconds, `runPosture` field 1.
    ///
    /// Its mean is 263.5 ms against the summary's `averageGct` 263, and its
    /// minimum 232 ms against `minGct` 232. 65535 is the "not measured"
    /// sentinel and never reaches storage.
    pub ground_contact_ms: Option<f64>,
    /// Vertical oscillation in millimetres, `runPosture` field 2.
    ///
    /// Mean 88.3 against the summary's `averageVo` 88 and maximum 95 against
    /// `maxVo` 95. Millimetres rather than centimetres because field 3 equals
    /// this divided by the stride length in the same units (88 / 1010 = 8.7%).
    pub vertical_oscillation_mm: Option<f64>,
    /// Vertical stride ratio in percent, `runPosture` field 3.
    ///
    /// Stored as a percentage: the raw integers are tenths of a percent and
    /// their mean, 87.1, matches the summary's `avgVertStrideRatio` 87.
    /// The 255 sentinel never reaches storage.
    pub vertical_ratio_pct: Option<f64>,
    /// Grade-adjusted equivalent pace in seconds per kilometre, `equivPace`.
    ///
    /// Not the reciprocal of `speed` — comparing the two disagrees on a third
    /// of the samples. What does line up is the summary: the series minimum,
    /// 264 s/km, is `bestEquivPace` 264, and the distance-weighted mean
    /// (5428.6 s over `equivDistance` 15257 m = 355.8) is `avgEquivPace` 355.
    pub equivalent_pace_s_per_km: Option<f64>,
    /// The watch's own cumulative distance (`currentDistance`), in meters. Bad points (going back, jumps over
    /// 200 m per second) are removed. #139: FIT `record.distance` uses this value.
    /// It can differ a little from the summary `dis` (one trail run: 10769.7 m vs 10970 m).
    /// The session total distance still uses the summary. Neither value is rewritten here.
    pub distance_m: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PauseInterval {
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    pub kind: String,
}

/// One kilometre of a workout, measured rather than estimated.
///
/// Splits come from the server's own cumulative distance series. Integrating
/// the per-second speed instead lands within 0.15% on a run but 12.6% out on a
/// ride, so the speed series is not a substitute.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkoutSplit {
    /// 1-based kilometre number.
    pub index: i32,
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    pub distance_m: f64,
    pub duration_seconds: i64,
    /// Minutes per kilometre; `None` when the split covered no distance.
    pub pace_min_per_km: Option<f64>,
    pub avg_hr: Option<i32>,
    pub max_hr: Option<i32>,
    pub elevation_gain_m: Option<f64>,
    pub elevation_loss_m: Option<f64>,
    /// True for a trailing partial kilometre, so an 800 m remainder is never
    /// read as a slow full kilometre.
    pub partial: bool,
}

/// One lap the watch itself recorded.
///
/// Not a [`WorkoutSplit`]. A split is our own cut at every kilometre; a lap is
/// what the watch marked at the time — the lap button, an auto-lap by distance,
/// or the intervals of a structured session. A 5820 m run can carry five
/// kilometre splits *and* fourteen 415 m laps, and they mean different things.
///
/// Read from the `lap` field of the workout detail, which is populated on 32 of
/// the 336 stored details in a real library. A .fit comparison on GitHub asked
/// for exactly this: "I use the lap button often, if you could add lap data to
/// your .fit file that would be my first choice."
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkoutLap {
    /// 1-based lap number.
    pub index: i32,
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    pub distance_m: f64,
    pub duration_seconds: i64,
    pub avg_hr: Option<i32>,
    pub max_hr: Option<i32>,
}

/// Altitude has to move by at least this much before it counts as climbing.
/// Barometric drift of a few centimetres per second would otherwise accumulate
/// into hundreds of metres of imaginary ascent over an hour.
const ELEVATION_NOISE_FLOOR_M: f64 = 1.0;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct DecodedWorkout {
    pub track_id: i64,
    pub source: Option<String>,
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    pub route: Vec<RoutePoint>,
    /// Number of later track points dropped when the delta chain breaks. 0 means complete.
    ///
    /// Coordinates are cumulative deltas: after a delta that cannot be read, an overflow, or a result off the Earth's surface,
    /// the position of every later point is unknown. Going on would only give a fake track with a normal shape but a global
    /// offset, so the track is cut there and the number of points not output is stored here.
    pub route_dropped_points: usize,
    pub samples: Vec<WorkoutSample>,
    pub pauses: Vec<PauseInterval>,
    pub splits: Vec<WorkoutSplit>,
    /// Laps recorded by the watch. They live next to `splits` and do not replace them. See [`WorkoutLap`].
    pub laps: Vec<WorkoutLap>,
    /// Per-set strength exercise details (from the 62-column `lap`). Empty for non-strength workouts.
    pub strength_sets: Vec<StrengthSetRow>,
}

/// Decodes one workout detail.
///
/// `summary_end` is the end time from the summary, and `summary_distance_m` is the total distance from the summary.
/// The latter is used only on the `kilo_pace` fallback path: that data has only whole kilometers, and the length of the last
/// partial km can come only from the summary. Without it, a 4975 m run reports only 4 km and the other
/// 975 m disappear silently. Both are `Option` because indoor workouts can have neither.
pub fn decode_workout_detail(
    raw: &Value,
    summary_end: Option<DateTime<Utc>>,
    summary_distance_m: Option<f64>,
) -> Result<DecodedWorkout> {
    let data = detail_object(raw)
        .ok_or_else(|| ZeppBridgeError::ParseError("workout detail is missing the data object".into()))?;

    let track_id = parse_i64(data.get("trackid"))
        .ok_or_else(|| ZeppBridgeError::ParseError("workout detail is missing trackid".into()))?;
    if track_id <= 0 {
        return Err(ZeppBridgeError::ParseError(
            "workout detail trackid is invalid".into(),
        ));
    }

    let start_time = Utc
        .timestamp_opt(track_id, 0)
        .single()
        .ok_or_else(|| ZeppBridgeError::ParseError("workout detail trackid is not a valid time".into()))?;

    let source = data
        .get("source")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);

    let time_deltas = parse_int_list(data.get("time"));
    // Sum only up to the first unreadable delta: every timestamp after it is unknown,
    // and adding those seconds to the total duration would also invent data.
    let time_sum = time_deltas
        .iter()
        .take_while(|value| value.is_some())
        .map(|value| i64::from(value.unwrap_or_default().max(0)))
        .fold(0i64, |acc, value| acc.saturating_add(value))
        .clamp(0, MAX_ACTIVITY_SECONDS);
    let time_end = add_seconds(start_time, time_sum)
        .ok_or_else(|| ZeppBridgeError::ParseError("workout detail duration overflow".into()))?;
    let min_end = add_seconds(start_time, 1)
        .ok_or_else(|| ZeppBridgeError::ParseError("workout detail duration overflow".into()))?;
    let uncapped_end = match summary_end {
        Some(summary) if summary > time_end => summary,
        _ => time_end.max(min_end),
    };
    // The summary end time can be far after start. A DateTime subtraction follows, so clamp to 48h first.
    let end_time = match add_seconds(start_time, MAX_ACTIVITY_SECONDS) {
        Some(cap) => uncapped_end.min(cap).max(min_end),
        None => time_end.max(min_end),
    };

    let duration_secs = end_time
        .timestamp()
        .saturating_sub(start_time.timestamp())
        .clamp(1, MAX_ACTIVITY_SECONDS);
    let from = track_id;
    let to = track_id
        .checked_add(duration_secs)
        .ok_or_else(|| ZeppBridgeError::ParseError("workout detail end time overflow".into()))?;

    let (latitudes, longitudes) = parse_coordinate_deltas(data.get("longitude_latitude"));
    let altitudes_cm = parse_altitude_cm(data.get("altitude"));
    // `time_delta_altitude` carries its own `(dt, altitude_cm)` cursor, so it
    // is not tied to GPS fixes and, unlike `altitude`, carries no leading
    // sentinel. Prefer it and keep the index-aligned list as the fallback.
    let altitude_pairs = parse_delta_pairs(data.get("time_delta_altitude"), true);
    // `currentDistance` is `(dt, cumulative centimetres)`. Verified against a
    // 15 km run: its final value, 1521722 cm, matches the summary's 15217 m.
    let distance_pairs = parse_rounded_delta_pairs(data.get("currentDistance"), true);
    let hr_pairs = parse_delta_pairs(data.get("heart_rate"), true);
    let speed_pairs = parse_float_pairs(data.get("speed"));
    let gait = parse_gait(data.get("gait"));
    // Running power and equivalent pace carry the same `(dt, value)` shape as
    // speed, but a leading empty delta appears in real payloads, so they use
    // the lenient reader rather than dropping the sample.
    let power_pairs = parse_valued_pairs(data.get("power_meter"));
    let equiv_pace_pairs = parse_valued_pairs(data.get("equivPace"));
    let posture = parse_run_posture(data.get("runPosture"));
    let pauses = parse_pauses(data.get("pause"), from);

    let heart_rates = if hr_pairs.is_empty() {
        None
    } else {
        Some(timed_cumulative_i32(from, to, &hr_pairs))
    };
    let speeds = if speed_pairs.is_empty() {
        None
    } else {
        Some(timed_fixed_f64(from, to, &speed_pairs))
    };
    let (steps, strides, cadences) = if gait.is_empty() {
        (None, None, None)
    } else {
        let step_pairs: Vec<(i64, i32)> = gait.iter().map(|row| (row.0, row.1)).collect();
        let stride_pairs: Vec<(i64, f64)> =
            gait.iter().map(|row| (row.0, f64::from(row.2))).collect();
        let cadence_pairs: Vec<(i64, f64)> =
            gait.iter().map(|row| (row.0, f64::from(row.3))).collect();
        (
            Some(timed_cumulative_i32(from, to, &step_pairs)),
            Some(timed_fixed_f64(from, to, &stride_pairs)),
            Some(timed_fixed_f64(from, to, &cadence_pairs)),
        )
    };
    let _ = steps;

    let powers = (!power_pairs.is_empty()).then(|| timed_fixed_f64(from, to, &power_pairs));
    let equiv_paces =
        (!equiv_pace_pairs.is_empty()).then(|| timed_fixed_f64(from, to, &equiv_pace_pairs));
    let (ground_contacts, oscillations, vertical_ratios) = if posture.is_empty() {
        (None, None, None)
    } else {
        // Sentinels are carried through the fill as NaN so a "not measured"
        // second never inherits the previous second's reading.
        let column = |pick: fn(&PostureRow) -> (i64, f64)| -> Vec<(i64, f64)> {
            posture.iter().map(pick).collect()
        };
        (
            Some(timed_fixed_f64(
                from,
                to,
                &column(|row| (row.0, sentinel_free(row.1, 65535))),
            )),
            Some(timed_fixed_f64(
                from,
                to,
                &column(|row| (row.0, sentinel_free(row.2, 65535))),
            )),
            Some(timed_fixed_f64(
                from,
                to,
                // Raw units are tenths of a percent.
                &column(|row| (row.0, sentinel_free(row.3, 255) / 10.0)),
            )),
        )
    };

    let mut route = Vec::new();
    let mut altitude_by_second: std::collections::BTreeMap<i64, f64> =
        std::collections::BTreeMap::new();
    let has_pair_altitude = !altitude_pairs.is_empty();
    if has_pair_altitude {
        let mut cursor = from;
        for (delta, centimetres) in &altitude_pairs {
            cursor = cursor.saturating_add((*delta).max(0));
            if let Some(meters) = cm_to_meters(i64::from(*centimetres)) {
                altitude_by_second.insert(cursor, meters);
            }
        }
    }
    let mut route_dropped_points = 0usize;
    if !time_deltas.is_empty() && !latitudes.is_empty() && !longitudes.is_empty() {
        let mut unix_ts = from;
        let mut latitude = 0i64;
        let mut longitude = 0i64;
        let count = time_deltas.len().min(latitudes.len()).min(longitudes.len());
        for index in 0..count {
            // If any link in the delta chain cannot be read, "the position from this point on is unknown". Cut the track
            // instead of skipping ahead and drawing every later point with an offset.
            let Some(delta_seconds) = time_deltas[index] else {
                break;
            };
            unix_ts = unix_ts.saturating_add(i64::from(delta_seconds.max(0)));
            let (Some(lat_delta), Some(lon_delta)) = (latitudes[index], longitudes[index]) else {
                break;
            };
            let (Some(next_latitude), Some(next_longitude)) = (
                latitude.checked_add(lat_delta),
                longitude.checked_add(lon_delta),
            ) else {
                break;
            };
            let latitude_deg = next_latitude as f64 / COORD_FACTOR;
            let longitude_deg = next_longitude as f64 / COORD_FACTOR;
            if latitude_deg.abs() > 90.0 || longitude_deg.abs() > 180.0 {
                break;
            }
            let Some(timestamp) = Utc.timestamp_opt(unix_ts, 0).single() else {
                break;
            };
            latitude = next_latitude;
            longitude = next_longitude;
            let altitude_m = if has_pair_altitude {
                altitude_by_second.get(&unix_ts).copied()
            } else {
                let meters = altitudes_cm
                    .get(index)
                    .copied()
                    .flatten()
                    .and_then(cm_to_meters);
                if let Some(meters) = meters {
                    altitude_by_second.insert(unix_ts, meters);
                }
                meters
            };
            route.push(RoutePoint {
                segment_start: false,
                timestamp,
                latitude: latitude_deg,
                longitude: longitude_deg,
                altitude_m,
            });
        }
        route_dropped_points = count - route.len();
    }

    let mut distance_by_second: std::collections::BTreeMap<i64, f64> =
        std::collections::BTreeMap::new();
    {
        let mut cursor = from;
        // The last kept reading (seconds, meters). `currentDistance` is cumulative and must never
        // decrease: going back is a bad point. A jump of more than 200 m per second from the last valid reading is also
        // a bad point. One real jump once produced twenty thousand zero-duration splits in the same second.
        //
        // Treat the start as "distance 0 at workout start", not empty. Otherwise the first reading fully
        // skips this check, and one bad first reading can make the splits explode.
        let mut previous: Option<(i64, f64)> = Some((from, 0.0));
        for (delta, centimetres) in &distance_pairs {
            cursor = cursor.saturating_add((*delta).max(0));
            let meters = f64::from(*centimetres) / 100.0;
            if let Some((previous_ts, previous_m)) = previous {
                let step = meters - previous_m;
                let elapsed = (cursor - previous_ts).max(1) as f64;
                if step < 0.0 || step > 200.0 * elapsed {
                    continue;
                }
            }
            distance_by_second.insert(cursor, meters);
            previous = Some((cursor, meters));
        }
    }
    let mut samples = Vec::with_capacity(duration_secs as usize);
    let mut last_altitude = None;
    for offset in 0..=duration_secs {
        let unix_ts = from.saturating_add(offset);
        let Some(timestamp) = Utc.timestamp_opt(unix_ts, 0).single() else {
            continue;
        };
        if let Some(altitude) = altitude_by_second.get(&unix_ts).copied() {
            last_altitude = Some(altitude);
        }
        let speed = speeds.as_ref().and_then(|map| map.get(&unix_ts).copied());
        let pace = speed.filter(|value| *value > 0.0).map(|value| 1.0 / value);
        samples.push(WorkoutSample {
            timestamp,
            heart_rate: heart_rates
                .as_ref()
                .and_then(|map| map.get(&unix_ts).copied())
                // The same bound as lap 0..=250: a reading that jumps to 300+ is a sensor fault,
                // not anyone's heart rate.
                .filter(|value| (1..=250).contains(value)),
            speed,
            pace,
            cadence: cadences.as_ref().and_then(|map| map.get(&unix_ts).copied()),
            stride_cm: strides.as_ref().and_then(|map| map.get(&unix_ts).copied()),
            altitude_m: last_altitude,
            power_watts: finite_at(powers.as_ref(), unix_ts),
            ground_contact_ms: finite_at(ground_contacts.as_ref(), unix_ts),
            vertical_oscillation_mm: finite_at(oscillations.as_ref(), unix_ts),
            equivalent_pace_s_per_km: finite_at(equiv_paces.as_ref(), unix_ts)
                .filter(|value| *value > 0.0),
            vertical_ratio_pct: finite_at(vertical_ratios.as_ref(), unix_ts),
            // Write only at the second that has a reading, and do not carry it forward: nobody measured the distance between readings.
            distance_m: distance_by_second.get(&unix_ts).copied(),
        });
    }

    let mut splits = compute_splits(&samples, &distance_by_second);
    // Check: if the split count is more than "whole km the summary distance can hold + 2", or any split has 0 duration,
    // bad points in `currentDistance` still got in. Drop all splits as bad data
    // and use the kilo_pace fallback below.
    let splits_are_plausible = splits.iter().all(|split| split.duration_seconds > 0)
        && summary_distance_m
            .filter(|value| value.is_finite() && *value > 0.0)
            .is_none_or(|total| (splits.len() as f64) <= total / 1000.0 + 2.0);
    if !splits_are_plausible {
        splits = Vec::new();
    }
    // When `currentDistance` is empty (130 of 336 details in our data),
    // the step above returns nothing, and the workout has only one lap. The cloud's own `kilo_pace` is
    // present on exactly these records.
    if splits.is_empty() {
        if let Some(timing) = kilometre_seconds(data.get("kilo_pace"), duration_secs, &pauses) {
            let seconds = &timing.moving;
            // Each kilo_pace row is one km, so the row count must match the summary distance (+-1 for rounding
            // of the partial km). A mismatch means the columns were misread, so reject it, the same as the lap check.
            let count_agrees = summary_distance_m
                .filter(|value| value.is_finite() && *value > 0.0)
                .is_none_or(|total| {
                    let expected = (total / 1000.0).floor() as i64;
                    (seconds.len() as i64 - expected).abs() <= 1
                });
            if count_agrees {
                let total = summary_distance_m.or_else(|| {
                    // When the summary has no distance, fall back to the whole km count: each kilo_pace row is one km.
                    (!seconds.is_empty()).then_some(seconds.len() as f64 * 1000.0)
                });
                splits = splits_from_kilometre_seconds(
                    &samples, &timing, &pauses, start_time, end_time, total,
                );
            }
        }
    }

    let strength_sets = crate::decoder::strength::parse_strength_sets(data);

    // Laps recorded by the watch. After decoding, check them against the summary: if the distances do not add up,
    // or the last lap does not end at the workout end, the columns were misread and all laps are dropped.
    // The 62-column strength lap stores per-set exercises (distance and elapsed seconds are always 0) and does not go into the lap branch.
    let laps = if !strength_sets.is_empty() {
        Vec::new()
    } else {
        let candidate = parse_laps(data.get("lap"), start_time);
        if laps_agree_with_summary(&candidate, summary_distance_m, duration_secs) {
            candidate
        } else {
            Vec::new()
        }
    };

    Ok(DecodedWorkout {
        track_id,
        source,
        start_time,
        end_time,
        route,
        route_dropped_points,
        samples,
        pauses,
        splits,
        laps,
        strength_sets,
    })
}

/// Cuts whole-km splits from cumulative distance, so other sources (user-imported .fit, #140) reuse the same
/// method instead of a second copy. `distances` is `(Unix seconds, cumulative meters)`.
pub fn splits_from_cumulative_distance(
    samples: &[WorkoutSample],
    distances: &std::collections::BTreeMap<i64, f64>,
) -> Vec<WorkoutSplit> {
    let splits = compute_splits(samples, distances);
    // The same check as detail decoding: any split with 0 duration means a jump in the distance, so reject all of it.
    if splits.iter().all(|split| split.duration_seconds > 0) {
        splits
    } else {
        Vec::new()
    }
}

#[cfg(test)]
mod tests;
