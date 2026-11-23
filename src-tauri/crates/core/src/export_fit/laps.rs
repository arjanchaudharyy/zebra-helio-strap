//! Laps, the whole-activity lap and the session message (split out of export_fit.rs, logic unchanged).

use super::*;

/// Which array of the export JSON this group of `lap` comes from, and which trigger to report.
#[derive(Debug, Clone, Copy)]
pub(super) struct LapSource {
    pub(super) field: &'static str,
    pub(super) trigger: typedef::LapTrigger,
}

/// One group of `lap`, from the `laps.field` array of the export JSON.
///
/// `source` has two values with different meanings:
///
/// * `laps`: **laps the watch recorded itself**. Lap button, auto lap by distance, or each
///   segment of an interval session. This is what really happened during the workout, so it wins.
/// * `splits`: our per-kilometer splits, from the server's cumulative distance (not
///   estimated from integrated speed). Used when the watch recorded no laps.
///
/// With neither, return 0, and the caller adds one lap that covers the whole activity
/// (`push_whole_activity_lap`). A file must never have zero laps.
pub(super) fn push_laps(
    messages: &mut Vec<Message>,
    workout: &Value,
    laps: LapSource,
    sport: typedef::Sport,
    sub_sport: typedef::SubSport,
    fallback_start: i64,
    points: &[(i64, Point)],
) -> u16 {
    let mut count = 0u16;
    for split in array(workout, laps.field) {
        let start = parse_unix(text(split.get("start_time"))).unwrap_or(fallback_start);
        let end = parse_unix(text(split.get("end_time"))).unwrap_or(start);
        let duration = split
            .get("duration_seconds")
            .and_then(Value::as_f64)
            .unwrap_or((end - start).max(0) as f64);
        let moving_seconds = (duration - paused_seconds(workout, start, end)).max(0.0);

        let mut fields = vec![
            u16_field(mesgdef::Lap::MESSAGE_INDEX, count),
            u32_field(mesgdef::Lap::TIMESTAMP, fit_timestamp(end)),
            u32_field(mesgdef::Lap::START_TIME, fit_timestamp(start)),
            enum_field(mesgdef::Lap::EVENT, typedef::Event::LAP.0),
            enum_field(mesgdef::Lap::EVENT_TYPE, typedef::EventType::STOP.0),
            enum_field(mesgdef::Lap::SPORT, sport.0),
            enum_field(mesgdef::Lap::SUB_SPORT, sub_sport.0),
            enum_field(mesgdef::Lap::INTENSITY, typedef::Intensity::ACTIVE.0),
            enum_field(mesgdef::Lap::LAP_TRIGGER, laps.trigger.0),
            u32_field(
                mesgdef::Lap::TOTAL_ELAPSED_TIME,
                (duration * 1000.0).max(0.0) as u32,
            ),
            u32_field(
                mesgdef::Lap::TOTAL_TIMER_TIME,
                (moving_seconds * 1000.0) as u32,
            ),
        ];

        if let Some(distance) = split.get("distance_m").and_then(Value::as_f64) {
            fields.push(u32_field(
                mesgdef::Lap::TOTAL_DISTANCE,
                (distance * 100.0).max(0.0) as u32,
            ));
        }
        // Average speed is "distance of this segment / time of this segment". Both values are measured values
        // just written to the file above, not estimates. Leaving it out has a real cost: importers usually read
        // this field directly and do not compute it from records, so the whole split column shows 0.
        if let (Some(distance), true) = (
            split.get("distance_m").and_then(Value::as_f64),
            moving_seconds > 0.0,
        ) {
            if let Some(speed) = encode_speed(distance / moving_seconds) {
                fields.push(u16_field(mesgdef::Lap::AVG_SPEED, speed));
            }
        }
        if let Some(speed) = max_speed_between(points, start, end).and_then(encode_speed) {
            fields.push(u16_field(mesgdef::Lap::MAX_SPEED, speed));
        }
        if let Some(avg_hr) = split
            .get("avg_hr")
            .and_then(Value::as_i64)
            .filter(|value| (1..=255).contains(value))
        {
            fields.push(u8_field(mesgdef::Lap::AVG_HEART_RATE, avg_hr as u8));
        }
        if let Some(max_hr) = split
            .get("max_hr")
            .and_then(Value::as_i64)
            .filter(|value| (1..=255).contains(value))
        {
            fields.push(u8_field(mesgdef::Lap::MAX_HEART_RATE, max_hr as u8));
        }
        if let Some(gain) = split.get("elevation_gain_m").and_then(Value::as_f64) {
            if gain.is_finite() && gain >= 0.0 && gain <= f64::from(u16::MAX) {
                fields.push(u16_field(mesgdef::Lap::TOTAL_ASCENT, gain as u16));
            }
        }
        if let Some(loss) = split.get("elevation_loss_m").and_then(Value::as_f64) {
            if loss.is_finite() && loss >= 0.0 && loss <= f64::from(u16::MAX) {
                fields.push(u16_field(mesgdef::Lap::TOTAL_DESCENT, loss as u16));
            }
        }

        messages.push(Message {
            num: typedef::MesgNum::LAP,
            fields,
            ..Default::default()
        });
        count += 1;
    }
    count
}

/// The one lap added when there are no `splits`. It covers the whole activity.
///
/// The FIT spec requires at least one lap per session, and `session.num_laps >= 1`. But
/// the server cuts `splits` by full kilometers: indoor workouts (treadmill, rowing machine, yoga, free
/// training, jump rope) and outdoor workouts shorter than 1 km never have them. Files for these workouts used to
/// have no lap and `num_laps` = 0, and Garmin Connect and Strava rejected the whole file when they checked
/// the message hierarchy. So the FIT export that issue #28 asked for did not work for this whole class of workouts,
/// while locally the file looked like a "successful export".
///
/// All values added here come from measured values already written elsewhere in this file. No new estimates: a missing value
/// means a missing field, as everywhere in this export.
#[allow(clippy::too_many_arguments)]
pub(super) fn push_whole_activity_lap(
    messages: &mut Vec<Message>,
    workout: &Value,
    sport: typedef::Sport,
    sub_sport: typedef::SubSport,
    start_unix: i64,
    end_unix: i64,
    elapsed_seconds: f64,
    points: &[(i64, Point)],
) {
    let duration_ms = (elapsed_seconds * 1000.0).max(0.0) as u32;
    let moving_seconds = timer_seconds(workout, start_unix, end_unix);
    let mut fields = vec![
        u16_field(mesgdef::Lap::MESSAGE_INDEX, 0),
        u32_field(mesgdef::Lap::TIMESTAMP, fit_timestamp(end_unix)),
        u32_field(mesgdef::Lap::START_TIME, fit_timestamp(start_unix)),
        enum_field(mesgdef::Lap::EVENT, typedef::Event::LAP.0),
        enum_field(mesgdef::Lap::EVENT_TYPE, typedef::EventType::STOP.0),
        enum_field(mesgdef::Lap::SPORT, sport.0),
        enum_field(mesgdef::Lap::SUB_SPORT, sub_sport.0),
        enum_field(mesgdef::Lap::INTENSITY, typedef::Intensity::ACTIVE.0),
        // This lap is the "whole activity", not a cut by distance, so the trigger is
        // session_end, not distance. Writing distance makes importers think there really
        // was a full-kilometer split point here.
        enum_field(
            mesgdef::Lap::LAP_TRIGGER,
            typedef::LapTrigger::SESSION_END.0,
        ),
        u32_field(mesgdef::Lap::TOTAL_ELAPSED_TIME, duration_ms),
        u32_field(
            mesgdef::Lap::TOTAL_TIMER_TIME,
            (moving_seconds * 1000.0) as u32,
        ),
    ];

    // Start position as in session: take the first point with a real fix. Indoor workouts have none.
    if let Some((latitude, longitude)) = points.iter().find_map(|(_, point)| {
        let latitude = point.latitude?;
        let longitude = point.longitude?;
        coordinates_in_domain(latitude, longitude).then_some((latitude, longitude))
    }) {
        if let (Some(lat), Some(lon)) = (semicircles(latitude), semicircles(longitude)) {
            fields.push(i32_field(mesgdef::Lap::START_POSITION_LAT, lat));
            fields.push(i32_field(mesgdef::Lap::START_POSITION_LONG, lon));
        }
    }

    if let Some(distance) = workout
        .get("distance_meters")
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite() && *value >= 0.0)
    {
        fields.push(u32_field(
            mesgdef::Lap::TOTAL_DISTANCE,
            (distance * 100.0) as u32,
        ));
        if moving_seconds > 0.0 {
            if let Some(speed) = encode_speed(distance / moving_seconds) {
                fields.push(u16_field(mesgdef::Lap::AVG_SPEED, speed));
            }
        }
    }
    if let Some(speed) = max_speed_between(points, start_unix, end_unix).and_then(encode_speed) {
        fields.push(u16_field(mesgdef::Lap::MAX_SPEED, speed));
    }
    if let Some(calories) = workout
        .get("calories")
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite() && *value >= 0.0 && *value <= f64::from(u16::MAX))
    {
        fields.push(u16_field(mesgdef::Lap::TOTAL_CALORIES, calories as u16));
    }
    if let Some(avg_hr) = workout
        .get("avg_hr")
        .and_then(Value::as_i64)
        .filter(|value| (1..=255).contains(value))
    {
        fields.push(u8_field(mesgdef::Lap::AVG_HEART_RATE, avg_hr as u8));
    }
    if let Some(max_hr) = workout
        .get("max_hr")
        .and_then(Value::as_i64)
        .filter(|value| (1..=255).contains(value))
    {
        fields.push(u8_field(mesgdef::Lap::MAX_HEART_RATE, max_hr as u8));
    }
    if let Some(gain) = total_elevation(workout, "elevation_gain_m", "elevation_gain_m") {
        if gain <= f64::from(u16::MAX) {
            fields.push(u16_field(mesgdef::Lap::TOTAL_ASCENT, gain.round() as u16));
        }
    }
    if let Some(loss) = total_elevation(workout, "elevation_loss_m", "elevation_loss_m") {
        if loss <= f64::from(u16::MAX) {
            fields.push(u16_field(mesgdef::Lap::TOTAL_DESCENT, loss.round() as u16));
        }
    }

    messages.push(Message {
        num: typedef::MesgNum::LAP,
        fields,
        ..Default::default()
    });
}

#[allow(clippy::too_many_arguments)]
pub(super) fn push_session(
    messages: &mut Vec<Message>,
    workout: &Value,
    sport: typedef::Sport,
    sub_sport: typedef::SubSport,
    start_unix: i64,
    end_unix: i64,
    elapsed_seconds: f64,
    lap_count: u16,
    points: &[(i64, Point)],
) {
    let moving_seconds = timer_seconds(workout, start_unix, end_unix);
    let mut fields = vec![
        u16_field(mesgdef::Session::MESSAGE_INDEX, 0),
        u32_field(mesgdef::Session::TIMESTAMP, fit_timestamp(end_unix)),
        u32_field(mesgdef::Session::START_TIME, fit_timestamp(start_unix)),
        enum_field(mesgdef::Session::EVENT, typedef::Event::SESSION.0),
        enum_field(mesgdef::Session::EVENT_TYPE, typedef::EventType::STOP.0),
        enum_field(mesgdef::Session::SPORT, sport.0),
        enum_field(mesgdef::Session::SUB_SPORT, sub_sport.0),
        u32_field(
            mesgdef::Session::TOTAL_ELAPSED_TIME,
            (elapsed_seconds * 1000.0) as u32,
        ),
        u32_field(
            mesgdef::Session::TOTAL_TIMER_TIME,
            (moving_seconds * 1000.0) as u32,
        ),
        u16_field(mesgdef::Session::FIRST_LAP_INDEX, 0),
        u16_field(mesgdef::Session::NUM_LAPS, lap_count),
    ];

    // Start position comes from the first point with a real fix, not the first record. The first
    // record of an indoor workout has no coordinates at all.
    if let Some((latitude, longitude)) = points.iter().find_map(|(_, point)| {
        let latitude = point.latitude?;
        let longitude = point.longitude?;
        coordinates_in_domain(latitude, longitude).then_some((latitude, longitude))
    }) {
        if let (Some(lat), Some(lon)) = (semicircles(latitude), semicircles(longitude)) {
            fields.push(i32_field(mesgdef::Session::START_POSITION_LAT, lat));
            fields.push(i32_field(mesgdef::Session::START_POSITION_LONG, lon));
        }
    }

    if let Some(distance) = workout.get("distance_meters").and_then(Value::as_f64) {
        if distance.is_finite() && distance >= 0.0 {
            fields.push(u32_field(
                mesgdef::Session::TOTAL_DISTANCE,
                (distance * 100.0) as u32,
            ));
            // Same as lap: distance / time, and both operands are measured values just written to this file.
            // Without this, the importer's "average speed / average pace" is empty.
            if moving_seconds > 0.0 {
                if let Some(speed) = encode_speed(distance / moving_seconds) {
                    fields.push(u16_field(mesgdef::Session::AVG_SPEED, speed));
                }
            }
        }
    }
    if let Some(speed) = points
        .iter()
        .filter_map(|(_, point)| point.speed_mps)
        .filter(|value| value.is_finite() && *value >= 0.0)
        .fold(None, |best: Option<f64>, value| {
            Some(best.map_or(value, |best| best.max(value)))
        })
        .and_then(encode_speed)
    {
        fields.push(u16_field(mesgdef::Session::MAX_SPEED, speed));
    }
    // Total ascent/descent: sum of the measured per-segment values. Before, it was written only on laps, but importer overviews read
    // the session, so "total ascent" always showed 0, even when the splits clearly had 9.35 m.
    if let Some(gain) = total_elevation(workout, "elevation_gain_m", "elevation_gain_m") {
        if gain <= f64::from(u16::MAX) {
            fields.push(u16_field(
                mesgdef::Session::TOTAL_ASCENT,
                gain.round() as u16,
            ));
        }
    }
    if let Some(loss) = total_elevation(workout, "elevation_loss_m", "elevation_loss_m") {
        if loss <= f64::from(u16::MAX) {
            fields.push(u16_field(
                mesgdef::Session::TOTAL_DESCENT,
                loss.round() as u16,
            ));
        }
    }
    // Average/max cadence, unit conversion same as record.
    {
        let cadences: Vec<f64> = points
            .iter()
            .filter_map(|(_, point)| point.cadence_spm)
            .filter(|value| value.is_finite() && *value > 0.0)
            .collect();
        if !cadences.is_empty() {
            let mean = cadences.iter().sum::<f64>() / cadences.len() as f64;
            if let Some(value) = steps_per_minute_to_fit_cadence(mean, sport) {
                fields.push(u8_field(mesgdef::Session::AVG_CADENCE, value));
            }
            let peak = cadences.iter().cloned().fold(f64::MIN, f64::max);
            if let Some(value) = steps_per_minute_to_fit_cadence(peak, sport) {
                fields.push(u8_field(mesgdef::Session::MAX_CADENCE, value));
            }
        }
    }
    // Steps -> TOTAL_CYCLES.
    //
    // Before, this field was not written and importers had to estimate: OPPO Health estimated 1274 steps from 0.83 km,
    // while the cloud summary had the real step count.
    //
    // Dividing by two has the same basis as `steps_per_minute_to_fit_cadence`: for running/walking one cycle
    // is one full stride (two footfalls). This direction was confirmed by test: OPPO showed "max cadence 142
    // steps/min" while the file had 71 rpm.
    //
    // Write it only for walking-type workouts. In cycling one cycle is one crank turn, which is not a step count.
    // Putting steps there only gives a fake total pedal count.
    if is_foot_sport(sport) {
        if let Some(cycles) = workout
            .get("total_steps")
            .and_then(Value::as_i64)
            .filter(|value| *value > 0)
            .map(|value| value / 2)
            .filter(|value| *value > 0 && *value <= i64::from(u32::MAX))
        {
            fields.push(u32_field(mesgdef::Session::TOTAL_CYCLES, cycles as u32));
        }
    }
    if let Some(calories) = workout
        .get("calories")
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite() && *value >= 0.0 && *value <= f64::from(u16::MAX))
    {
        fields.push(u16_field(mesgdef::Session::TOTAL_CALORIES, calories as u16));
    }
    if let Some(avg_hr) = workout
        .get("avg_hr")
        .and_then(Value::as_i64)
        .filter(|value| (1..=255).contains(value))
    {
        fields.push(u8_field(mesgdef::Session::AVG_HEART_RATE, avg_hr as u8));
    }
    if let Some(max_hr) = workout
        .get("max_hr")
        .and_then(Value::as_i64)
        .filter(|value| (1..=255).contains(value))
    {
        fields.push(u8_field(mesgdef::Session::MAX_HEART_RATE, max_hr as u8));
    }

    // The data below was always in `workouts` and `workout_hr_zones`, but it
    // was never written to the FIT file. Someone compared our exported .fit field by field with Zepp's own
    // export (GitHub, 2026-09-04): our values are more exact (Zepp's avg_speed
    // and total_strides contradict each other, ours agree), but "the Zepp file carries
    // more: altitude, power, running power, stride length, time in HR zones, training effect".
    //
    // The scale / offset of each field follows the FIT profile, as noted in its comment.
    // A wrong encoding gives no error. The reader only sees a number of the wrong magnitude that still looks
    // plausible.

    // Stride length. The database uses cm; FIT `avg_step_length` is u16, unit mm, scale 10,
    // so the stored value is "mm times ten". 109 cm -> 1090 mm -> 10900.
    if let Some(stride_cm) = workout
        .get("avg_stride_cm")
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite() && *value > 0.0)
    {
        let encoded = stride_cm * 100.0;
        if encoded.is_finite() && encoded <= f64::from(u16::MAX) {
            fields.push(u16_field(
                mesgdef::Session::AVG_STEP_LENGTH,
                encoded.round() as u16,
            ));
        }
    }

    // Power. Each record always wrote `power`, but the session had no summary, so platforms that read
    // power per session thought this workout had no power data. The unit is watts, no scale.
    let powers: Vec<f64> = points
        .iter()
        .filter_map(|(_, point)| point.power_watts)
        .filter(|value| value.is_finite() && *value >= 0.0)
        .collect();
    if !powers.is_empty() {
        let average = powers.iter().sum::<f64>() / powers.len() as f64;
        let peak = powers.iter().copied().fold(f64::MIN, f64::max);
        if average.is_finite() && average <= f64::from(u16::MAX) {
            fields.push(u16_field(
                mesgdef::Session::AVG_POWER,
                average.round() as u16,
            ));
        }
        if peak.is_finite() && peak <= f64::from(u16::MAX) {
            fields.push(u16_field(mesgdef::Session::MAX_POWER, peak.round() as u16));
        }
    }

    // Altitude. Total ascent/descent was already written, absolute altitude was not. "How high you ran"
    // and "how much you climbed" are different questions. The encoding reuses the per-record
    // `encode_altitude` ((m + 500) x 5), not a second copy here: two copies drift apart sooner or later,
    // and then session and record altitudes disagree, and the reader thinks the data is broken.
    // Max/min come from the cloud summary: it covers the whole workout, including parts without per-second samples.
    // The summary has no average, so it is computed from samples. A field the source does not give is not invented.
    for (key, field) in [
        ("max_altitude_m", mesgdef::Session::MAX_ALTITUDE),
        ("min_altitude_m", mesgdef::Session::MIN_ALTITUDE),
    ] {
        if let Some(encoded) = workout
            .get(key)
            .and_then(Value::as_f64)
            .filter(|value| value.is_finite())
            .and_then(encode_altitude)
        {
            fields.push(u16_field(field, encoded));
        }
    }
    let altitudes: Vec<f64> = points
        .iter()
        .filter_map(|(_, point)| point.altitude_m)
        .filter(|value| value.is_finite())
        .collect();
    if !altitudes.is_empty() {
        let average = altitudes.iter().sum::<f64>() / altitudes.len() as f64;
        if let Some(encoded) = encode_altitude(average) {
            fields.push(u16_field(mesgdef::Session::AVG_ALTITUDE, encoded));
        }
    }

    // Training effect. u8, scale 10: 3.4 is stored as 34. The watch range is 0.0-5.0.
    for (key, field) in [
        ("training_effect", mesgdef::Session::TOTAL_TRAINING_EFFECT),
        (
            "anaerobic_training_effect",
            mesgdef::Session::TOTAL_ANAEROBIC_TRAINING_EFFECT,
        ),
    ] {
        if let Some(effect) = workout
            .get(key)
            .and_then(Value::as_f64)
            .filter(|value| value.is_finite() && *value >= 0.0 && *value <= 5.0)
        {
            fields.push(u8_field(field, (effect * 10.0).round() as u8));
        }
    }

    if let Some(zones) = hr_zone_field(workout) {
        fields.push(u32_array_field(mesgdef::Session::TIME_IN_HR_ZONE, zones));
    }

    messages.push(Message {
        num: typedef::MesgNum::SESSION,
        fields,
        ..Default::default()
    });
}
