//! Encode one workout as FIT: merge per-point series, record messages, lap triggers (split out of export_fit.rs, logic unchanged).

use super::*;

/// Encode one workout. Return `Ok(None)` when there is no sample to write, so the caller skips it.
///
/// The empty check is "no route and no samples", not "no route": indoor workouts
/// have no GPS, and FIT never requires GPS. A treadmill record with heart rate only is a fully valid
/// FIT file.
pub(super) fn encode_workout(workout: &Value) -> Result<Option<(Vec<u8>, usize)>, String> {
    let points = merge_series(workout);
    let sets = array(&workout["strength"], "sets");
    if points.is_empty() && sets.is_empty() {
        return Ok(None);
    }

    let (sport, sub_sport) = map_sport(text(
        workout
            .get("effective_type")
            .or_else(|| workout.get("workout_type")),
    ));

    let points_span = points
        .first()
        .zip(points.last())
        .map(|((first, _), (last, _))| (*first, *last));
    let declared_span = parse_unix(text(workout.get("start_time")))
        .zip(parse_unix(text(workout.get("end_time"))))
        .filter(|(start, end)| end > start);
    let Some((start_unix, end_unix)) = declared_span
        .filter(|(start, end)| {
            points_span.is_none_or(|(first, last)| *start <= first && *end >= last)
        })
        .or(points_span)
    else {
        return Ok(None);
    };

    let mut messages = Vec::new();

    // file_id must be the first message, or the file is not seen as an activity.
    messages.push(Message {
        num: typedef::MesgNum::FILE_ID,
        fields: vec![
            enum_field(mesgdef::FileId::TYPE, typedef::File::ACTIVITY.0),
            u16_field(mesgdef::FileId::MANUFACTURER, typedef::Manufacturer::ZEPP.0),
            u32_field(mesgdef::FileId::TIME_CREATED, fit_timestamp(start_unix)),
        ],
        ..Default::default()
    });

    // Write the device name only when the local database really recorded it. This is the watch Zepp reports, not
    // ZeppBridge itself.
    let device_label = text(workout.get("device_label"));
    if !device_label.is_empty() {
        messages.push(Message {
            num: typedef::MesgNum::DEVICE_INFO,
            fields: vec![
                u32_field(mesgdef::DeviceInfo::TIMESTAMP, fit_timestamp(start_unix)),
                u16_field(
                    mesgdef::DeviceInfo::MANUFACTURER,
                    typedef::Manufacturer::ZEPP.0,
                ),
                string_field(mesgdef::DeviceInfo::PRODUCT_NAME, device_label),
            ],
            ..Default::default()
        });
    }

    messages.push(timer_event(start_unix, typedef::EventType::START));

    // Pause intervals: write timer stop when a pause starts and timer start when it resumes. This uses the same basis as
    // the GPX export that splits trkseg at both sides of a pause, only in FIT terms.
    let pauses = pause_intervals(workout, start_unix, end_unix);
    let pause_events: Vec<_> = pauses
        .iter()
        .flat_map(|&(start, end)| {
            [
                (start, typedef::EventType::STOP),
                (end, typedef::EventType::START),
            ]
        })
        .collect();

    let mut record_count = 0usize;
    let mut pause_cursor = 0usize;
    for (unix, point) in &points {
        while pause_cursor < pause_events.len() && pause_events[pause_cursor].0 <= *unix {
            let (at, event_type) = pause_events[pause_cursor];
            messages.push(timer_event(at, event_type));
            pause_cursor += 1;
        }
        messages.push(record_message(*unix, point, sport));
        record_count += 1;
    }

    for &(at, event_type) in &pause_events[pause_cursor..] {
        messages.push(timer_event(at, event_type));
    }

    messages.push(timer_event(end_unix, typedef::EventType::STOP));
    messages.extend(sets.iter().filter_map(strength_set_message));

    let elapsed_seconds = (end_unix - start_unix).max(0) as f64;
    let moving_seconds = timer_seconds(workout, start_unix, end_unix);
    // Laps the watch recorded win over our per-kilometer splits.
    //
    // "I often press the lap button. If your .fit can carry lap data, that is my first choice."
    // These are the exact words in the field-by-field comparison report on GitHub. Watch laps are the real
    // segments of the workout. Per-kilometer splits are cut by us later. Both are in the database, but the file can hold only one group.
    let lap_source = if array(workout, "laps").is_empty() {
        LapSource {
            field: "splits",
            trigger: typedef::LapTrigger::DISTANCE,
        }
    } else {
        LapSource {
            field: "laps",
            trigger: watch_lap_trigger(workout),
        }
    };
    let mut lap_count = push_laps(
        &mut messages,
        workout,
        lap_source,
        sport,
        sub_sport,
        start_unix,
        &points,
    );
    // When no lap was written, we must add one. See `push_whole_activity_lap` for the reason.
    if lap_count == 0 {
        push_whole_activity_lap(
            &mut messages,
            workout,
            sport,
            sub_sport,
            start_unix,
            end_unix,
            elapsed_seconds,
            &points,
        );
        lap_count = 1;
    }
    push_session(
        &mut messages,
        workout,
        sport,
        sub_sport,
        start_unix,
        end_unix,
        elapsed_seconds,
        lap_count,
        &points,
    );

    let mut activity_fields = vec![
        u32_field(mesgdef::Activity::TIMESTAMP, fit_timestamp(end_unix)),
        u32_field(
            mesgdef::Activity::TOTAL_TIMER_TIME,
            (moving_seconds * 1000.0) as u32,
        ),
        u16_field(mesgdef::Activity::NUM_SESSIONS, 1),
        enum_field(mesgdef::Activity::TYPE, typedef::Activity::MANUAL.0),
        enum_field(mesgdef::Activity::EVENT, typedef::Event::ACTIVITY.0),
        enum_field(mesgdef::Activity::EVENT_TYPE, typedef::EventType::STOP.0),
    ];
    // Local timestamp.
    //
    // `activity.local_timestamp` is "the same moment in local time". Importers
    // subtract the UTC `timestamp` from it to find the time zone of the activity. With UTC only,
    // Garmin Connect falls back to the account's default time zone, so a 06:00 run in Beijing time
    // shows as 22:00 on the day before.
    //
    // start_time in the database is already UTC, so a zero offset does not prove the watch's time zone.
    // Keep only inputs that still carry an explicit non-zero offset. When it cannot be confirmed, omit the field.
    if let Some(offset) = local_offset_seconds(workout) {
        activity_fields.push(u32_field(
            mesgdef::Activity::LOCAL_TIMESTAMP,
            fit_timestamp(end_unix + i64::from(offset)),
        ));
    }
    messages.push(Message {
        num: typedef::MesgNum::ACTIVITY,
        fields: activity_fields,
        ..Default::default()
    });

    let mut fit = FIT {
        messages,
        ..Default::default()
    };

    let mut buffer = std::io::Cursor::new(Vec::new());
    Encoder::new()
        .encode(FromStd::new(&mut buffer), &mut fit)
        .map_err(|error| format!("FIT encoding failed: {error:?}"))?;

    Ok(Some((buffer.into_inner(), record_count)))
}

fn strength_set_message(set: &Value) -> Option<Message> {
    let reps = u16::try_from(set["reps"].as_u64()?)
        .ok()
        .filter(|value| *value != u16::MAX)?;
    let mut fields = vec![
        enum_field(mesgdef::Set::SET_TYPE, typedef::SetType::ACTIVE.0),
        u16_field(mesgdef::Set::REPETITIONS, reps),
    ];
    if let Some(duration) = set["duration_ms"]
        .as_u64()
        .and_then(|value| u32::try_from(value).ok())
        .filter(|value| *value != u32::MAX)
    {
        fields.push(u32_field(mesgdef::Set::DURATION, duration));
    }
    if let Some(weight) = set["weight_kg"]
        .as_f64()
        .map(|kg| (kg * 16.0).round())
        .filter(|value| *value > 0.0 && *value < f64::from(u16::MAX))
    {
        fields.push(u16_field(mesgdef::Set::WEIGHT, weight as u16));
    }
    Some(Message {
        num: typedef::MesgNum::SET,
        fields,
        ..Default::default()
    })
}

/// All writable values at one point in time. All fields are `Option`: not measured means not measured.
#[derive(Default, Clone)]
pub(super) struct Point {
    pub(super) latitude: Option<f64>,
    pub(super) longitude: Option<f64>,
    pub(super) altitude_m: Option<f64>,
    pub(super) heart_rate: Option<i64>,
    pub(super) speed_mps: Option<f64>,
    /// Cadence, unit **steps/minute** (see `steps_per_minute_to_fit_cadence`).
    pub(super) cadence_spm: Option<f64>,
    pub(super) power_watts: Option<f64>,
    pub(super) ground_contact_ms: Option<f64>,
    pub(super) vertical_oscillation_mm: Option<f64>,
    /// Stride length in cm. `workout_samples.stride` has a value every second, but this column in the FIT file
    /// was always empty. That is what the comparison report meant by "stride length only in the Zepp file".
    pub(super) stride_cm: Option<f64>,
    /// Cumulative distance from the watch, in meters (#139). Present only in seconds that have a reading.
    pub(super) distance_m: Option<f64>,
}

/// Merge the `route` and `samples` series into one by timestamp.
///
/// Both are the same timeline of the same workout (GPS gives one point per second, sensors also give
/// one row per second), but each can miss rows, so we align by second, not pair by index. Pairing by index
/// shifts everything after the first missing row in either series.
pub(super) fn merge_series(workout: &Value) -> Vec<(i64, Point)> {
    let mut merged: BTreeMap<i64, Point> = BTreeMap::new();

    for entry in array(workout, "route") {
        let Some(unix) = parse_unix(text(entry.get("timestamp"))) else {
            continue;
        };
        let point = merged.entry(unix).or_default();
        // Coordinates must come as a pair and lie on the Earth's surface. Out-of-range coordinates count as no coordinates,
        // and other values on this record (heart rate, power) are not affected.
        if let (Some(latitude), Some(longitude)) = (
            entry.get("latitude").and_then(Value::as_f64),
            entry.get("longitude").and_then(Value::as_f64),
        ) {
            if coordinates_in_domain(latitude, longitude) {
                point.latitude = Some(latitude);
                point.longitude = Some(longitude);
            }
        }
        if let Some(altitude) = entry.get("altitude_m").and_then(Value::as_f64) {
            point.altitude_m = Some(altitude);
        }
    }

    for entry in array(workout, "samples") {
        let Some(unix) = parse_unix(text(entry.get("timestamp"))) else {
            continue;
        };
        let point = merged.entry(unix).or_default();
        point.heart_rate = entry.get("heart_rate").and_then(Value::as_i64);
        point.speed_mps = entry.get("speed").and_then(Value::as_f64);
        point.cadence_spm = entry.get("cadence").and_then(Value::as_f64);
        point.power_watts = entry.get("power_watts").and_then(Value::as_f64);
        point.ground_contact_ms = entry.get("ground_contact_ms").and_then(Value::as_f64);
        point.stride_cm = entry.get("stride_cm").and_then(Value::as_f64);
        point.distance_m = entry.get("distance_m").and_then(Value::as_f64);
        point.vertical_oscillation_mm =
            entry.get("vertical_oscillation_mm").and_then(Value::as_f64);
        // Do not overwrite altitude that route already gave: both have the same source, but the route value matches the coordinates.
        if point.altitude_m.is_none() {
            point.altitude_m = entry.get("altitude_m").and_then(Value::as_f64);
        }
    }

    // A point in time with nothing measured is not worth a record.
    merged.retain(|_, point| {
        point.latitude.is_some()
            || point.heart_rate.is_some()
            || point.speed_mps.is_some()
            || point.power_watts.is_some()
            || point.altitude_m.is_some()
            || point.cadence_spm.is_some()
            || point.distance_m.is_some()
    });

    merged.into_iter().collect()
}

/// Write one point in time as one `record`.
pub(super) fn record_message(unix: i64, point: &Point, sport: typedef::Sport) -> Message {
    let mut fields = vec![u32_field(mesgdef::Record::TIMESTAMP, fit_timestamp(unix))];

    if let (Some(latitude), Some(longitude)) = (point.latitude, point.longitude) {
        if coordinates_in_domain(latitude, longitude) {
            if let (Some(lat), Some(lon)) = (semicircles(latitude), semicircles(longitude)) {
                fields.push(i32_field(mesgdef::Record::POSITION_LAT, lat));
                fields.push(i32_field(mesgdef::Record::POSITION_LONG, lon));
            }
        }
    }
    if let Some(altitude) = point.altitude_m.and_then(encode_altitude) {
        fields.push(u16_field(mesgdef::Record::ALTITUDE, altitude));
    }
    // Cumulative distance: u32, unit cm (scale 100). Tools such as Strava compute moving distance from it;
    // without it they must integrate GPS, and a disturbed track gains several kilometers.
    if let Some(distance) = point
        .distance_m
        .filter(|value| value.is_finite() && *value >= 0.0)
    {
        let centimetres = (distance * 100.0).round();
        if centimetres < f64::from(u32::MAX) {
            fields.push(u32_field(mesgdef::Record::DISTANCE, centimetres as u32));
        }
    }
    if let Some(heart_rate) = point.heart_rate.filter(|value| (1..=255).contains(value)) {
        fields.push(u8_field(mesgdef::Record::HEART_RATE, heart_rate as u8));
    }
    if let Some(speed) = point
        .speed_mps
        .filter(|value| value.is_finite() && *value >= 0.0)
    {
        let scaled = (speed * 1000.0).round();
        if scaled <= f64::from(u16::MAX) {
            fields.push(u16_field(mesgdef::Record::SPEED, scaled as u16));
        }
    }
    if let Some(cadence) = point
        .cadence_spm
        .and_then(|value| steps_per_minute_to_fit_cadence(value, sport))
    {
        fields.push(u8_field(mesgdef::Record::CADENCE, cadence));
    }
    if let Some(power) = point
        .power_watts
        .filter(|value| value.is_finite() && *value >= 0.0)
    {
        let watts = power.round();
        if watts <= f64::from(u16::MAX) {
            fields.push(u16_field(mesgdef::Record::POWER, watts as u16));
        }
    }
    if let Some(contact) = point
        .ground_contact_ms
        .filter(|value| value.is_finite() && *value > 0.0)
    {
        let scaled = (contact * 10.0).round();
        if scaled <= f64::from(u16::MAX) {
            fields.push(u16_field(mesgdef::Record::STANCE_TIME, scaled as u16));
        }
    }
    // Stride length. The database uses cm; FIT `step_length` is u16, unit mm, scale 10,
    // so the stored value is "mm times ten". 1.09 m -> 1090 mm -> 10900.
    if let Some(stride) = point
        .stride_cm
        .filter(|value| value.is_finite() && *value > 0.0)
    {
        let encoded = (stride * 100.0).round();
        if encoded <= f64::from(u16::MAX) {
            fields.push(u16_field(mesgdef::Record::STEP_LENGTH, encoded as u16));
        }
    }
    if let Some(oscillation) = point
        .vertical_oscillation_mm
        .filter(|value| value.is_finite() && *value > 0.0)
    {
        let scaled = (oscillation * 10.0).round();
        if scaled <= f64::from(u16::MAX) {
            fields.push(u16_field(
                mesgdef::Record::VERTICAL_OSCILLATION,
                scaled as u16,
            ));
        }
    }

    Message {
        num: typedef::MesgNum::RECORD,
        fields,
        ..Default::default()
    }
}

/// Are the watch laps auto laps by distance, or pressed by a person?
///
/// The watch does not say, but lap lengths do: auto laps all have the same length (real data once had
/// a 5820 m run cut into 14 laps of 415 m), and pressed laps vary (another workout in the same data
/// had intervals of 570 / 2310 / 320 / 150 / 530 m).
///
/// When we cannot tell, we report `MANUAL`: it means "a person marked this lap", and this
/// group of laps does come from the watch lap list, not from our cuts. Marking pressed laps as `DISTANCE`
/// would claim something for the watch that it never said.
pub(super) fn watch_lap_trigger(workout: &Value) -> typedef::LapTrigger {
    let distances: Vec<f64> = array(workout, "laps")
        .iter()
        .filter_map(|lap| lap.get("distance_m").and_then(Value::as_f64))
        .filter(|value| value.is_finite() && *value > 0.0)
        .collect();
    // The last lap is a remainder with a naturally different length, so it is left out of the check.
    let considered = distances.len().saturating_sub(1);
    if considered < 2 {
        return typedef::LapTrigger::MANUAL;
    }
    let first = distances[0];
    let uniform = distances[..considered]
        .iter()
        .all(|value| (value - first).abs() / first < 0.01);
    if uniform {
        typedef::LapTrigger::DISTANCE
    } else {
        typedef::LapTrigger::MANUAL
    }
}
