//! Splits sleep, stages, heart rate and daily metrics out of band data items (split out of normalizer/mod.rs).

use super::*;

pub(super) fn sleep_from_band_item(
    item: &Map<String, Value>,
    summary: &Map<String, Value>,
    sleep: &Map<String, Value>,
    scope: SourceScope,
    zone: Option<&str>,
) -> std::result::Result<SleepSession, String> {
    let start_time = first_value(sleep, &["st", "startTime", "start_time"])
        .and_then(parse_timestamp)
        .ok_or_else(|| "sleep summary missing start time".to_string())?;
    let end_time = first_value(sleep, &["ed", "endTime", "end_time"])
        .and_then(parse_timestamp)
        .ok_or_else(|| "sleep summary missing end time".to_string())?;
    if end_time <= start_time {
        return Err("sleep end time is not after start time".to_string());
    }

    let deep_minutes = optional_band_stage_minutes(sleep, &["dp", "deepMinutes"], &[5]);
    let light_minutes = optional_band_stage_minutes(sleep, &["lt", "lightMinutes"], &[4]);
    // Awake time counts as known only with a `wk` field or a real mode=7 segment. Before, any stage array
    // gave 0, so sleep edited in the app (without awake segments) showed "awake 0 minutes, asleep = in bed",
    // which did not match the app.
    let awake_minutes = first_number(sleep, &["wk", "awakeMinutes"])
        .map(|value| value.round() as i32)
        .or_else(|| {
            let minutes = band_stage_minutes(sleep, 7);
            (minutes > 0).then_some(minutes)
        });
    let rem_from_field = first_number(sleep, &["rm", "remMinutes", "rem"])
        .map(|value| value.round() as i32)
        .filter(|value| *value >= 0);
    let rem_from_stages = band_stage_minutes(sleep, 8) + band_stage_minutes(sleep, 11);
    let span_minutes = (end_time - start_time).num_minutes().max(0) as i32;
    let rem_minutes = rem_from_field.or_else(|| (rem_from_stages > 0).then_some(rem_from_stages));
    // Time asleep: field > in bed - awake (when awake is really known) > deep + light + REM.
    // Only when all three are missing is time in bed left. Also record it in time_in_bed so the UI knows what it is
    // (this column cannot be empty in the database; same fallback as the official channel).
    let staged_minutes = [deep_minutes, light_minutes, rem_minutes]
        .into_iter()
        .flatten()
        .sum::<i32>();
    let (duration_minutes, time_in_bed_minutes) =
        match first_number(sleep, &["duration", "durationMinutes"]) {
            Some(value) => (value.round() as i32, None),
            None => match awake_minutes {
                Some(awake) => ((span_minutes - awake).max(0), None),
                None if staged_minutes > 0 => (staged_minutes, None),
                None => (span_minutes, Some(span_minutes)),
            },
        };
    let source_device = device_id(item)
        .or_else(|| first_string(summary, &["sn"]))
        .filter(|value| !value.is_empty());
    let sleep_id = first_string(item, &["sleep_id", "sleepId", "id"]).unwrap_or_else(|| {
        format!(
            "band:{}:{}:{}",
            source_device.as_deref().unwrap_or("unknown"),
            start_time.timestamp(),
            end_time.timestamp()
        )
    });
    let stages = sleep_stages_from_band(item, summary, sleep, zone);

    Ok(SleepSession {
        sleep_id,
        start_time,
        end_time,
        score: first_number(sleep, &["ss", "score", "sleepScore"])
            .map(|value| value.round() as i32)
            .filter(|value| (0..=100).contains(value)),
        duration_minutes,
        deep_minutes,
        light_minutes,
        rem_minutes,
        awake_minutes,
        source_scope: scope,
        device_id: source_device,
        synced_at: None,
        time_in_bed_minutes,
        stages,
        wake_count: first_number(sleep, &["wc", "wakeCount"])
            .map(|value| value.round() as i32)
            .filter(|value| (0..=200).contains(value)),
    })
}

pub(super) fn sleep_from_flat_object(object: &Map<String, Value>) -> Option<SleepSession> {
    let sleep_id = first_string(object, &["sleep_id", "sleepId", "id", "sessionId"])?;
    let start_time =
        first_value(object, &["start_time", "startTime", "beginTime"]).and_then(parse_timestamp)?;
    let end_time =
        first_value(object, &["end_time", "endTime", "finishTime"]).and_then(parse_timestamp)?;
    if end_time <= start_time {
        return None;
    }
    let awake_minutes =
        first_number(object, &["awake_minutes", "awakeMinutes", "awake"]).map(duration_to_minutes);
    let source_device = device_id(object);
    let span_minutes = (end_time - start_time).num_minutes() as i32;
    Some(SleepSession {
        sleep_id,
        start_time,
        end_time,
        score: first_number(object, &["score", "sleepScore"]).map(|value| value as i32),
        duration_minutes: first_number(
            object,
            &["duration_minutes", "durationMinutes", "duration"],
        )
        .map(duration_to_minutes)
        .unwrap_or_else(|| match awake_minutes {
            Some(awake) => (span_minutes - awake).max(0),
            None => span_minutes.max(0),
        }),
        deep_minutes: first_number(object, &["deep_minutes", "deepMinutes", "deep"])
            .map(duration_to_minutes),
        light_minutes: first_number(object, &["light_minutes", "lightMinutes", "light"])
            .map(duration_to_minutes),
        rem_minutes: first_number(object, &["rem_minutes", "remMinutes", "rem"])
            .map(duration_to_minutes),
        awake_minutes,
        source_scope: source_scope(Some(object), source_device.as_deref()),
        device_id: source_device,
        synced_at: None,
        time_in_bed_minutes: None,
        stages: Vec::new(),
        wake_count: None,
    })
}

/// Cloud stage mode -> stage name.
///
/// Unknown modes return `unknown`, **no longer `awake`**. The old code (`_ =>
/// Some("awake")`, "to avoid gaps in the stage bar") meant: if Zepp adds a new
/// mode, or a new watch produces a mode we do not know, ZeppBridge clearly tells the user
/// "you were awake then". That invents a fact for the user and conflicts with this project's rule:
/// "missing is missing; never generate fake zeros".
pub(super) fn sleep_stage_name(mode: i64) -> &'static str {
    match mode {
        5 => "deep",
        4 => "light",
        // New firmware also encodes REM as 11
        8 | 11 => "rem",
        7 => "awake",
        _ => "unknown",
    }
}

pub(super) fn sleep_stages_from_band(
    item: &Map<String, Value>,
    summary: &Map<String, Value>,
    sleep: &Map<String, Value>,
    zone: Option<&str>,
) -> Vec<SleepStageSlice> {
    let Some(date) = first_string(item, &["date_time", "date", "dayId"])
        .and_then(|value| NaiveDate::parse_from_str(&value, "%Y-%m-%d").ok())
    else {
        return Vec::new();
    };
    // A missing tz must not be read as UTC 0: old firmware often omits this field, and a silent 0 shifts all stages by several time zones.
    let Some(timezone_offset) = first_value(summary, &["tz"])
        .and_then(timezone_offset_seconds)
        .map(|value| value.clamp(-18 * 3600, 18 * 3600))
        .map(|value| effective_band_offset(value, date, zone))
    else {
        return Vec::new();
    };
    let Some(local_midnight) = date.and_hms_opt(0, 0, 0) else {
        return Vec::new();
    };
    let utc_midnight = DateTime::<Utc>::from_naive_utc_and_offset(
        local_midnight - Duration::seconds(timezone_offset),
        Utc,
    );
    let Some(stages) = sleep.get("stage").and_then(Value::as_array) else {
        return Vec::new();
    };

    let build = |anchor: DateTime<Utc>| -> Vec<SleepStageSlice> {
        stages
            .iter()
            .filter_map(Value::as_object)
            .filter_map(|stage| {
                let mode = first_number(stage, &["mode"])?.round() as i64;
                let name = sleep_stage_name(mode);
                let start = first_number(stage, &["start"])? as i64;
                let stop = first_number(stage, &["stop"])? as i64;
                if stop < start {
                    return None;
                }
                let start_time = add_minutes(anchor, start)?;
                let end_minutes = stop.checked_add(1)?;
                let end_time = add_minutes(anchor, end_minutes)?;
                if end_time <= start_time {
                    return None;
                }
                Some(SleepStageSlice {
                    stage: name.to_string(),
                    start_time,
                    end_time,
                    // Keep the raw code only for unknown modes. For the four known ones it adds no information.
                    raw_mode: (name == "unknown").then_some(mode),
                })
            })
            .collect()
    };

    // stage.start/stop are minutes from the local midnight of the "night of falling asleep"; past midnight they are >= 1440.
    // But date_time is the wake-up day. Night sleep must anchor to midnight of the day before date_time,
    // or all stages shift by +24h (seen in real data: stage 1460 min = 00:20 on the wake-up day).
    // Segments such as naps, encoded from midnight of the same day, pick the same-day anchor by overlap with [st, ed].
    let session_start =
        first_value(sleep, &["st", "startTime", "start_time"]).and_then(parse_timestamp);
    let session_end = first_value(sleep, &["ed", "endTime", "end_time"]).and_then(parse_timestamp);
    let prev_day = Duration::try_days(1)
        .and_then(|delta| utc_midnight.checked_sub_signed(delta))
        .map(build)
        .unwrap_or_default();
    match (session_start, session_end) {
        (Some(start), Some(end)) => {
            let same_day = build(utc_midnight);
            let overlap = |slices: &[SleepStageSlice]| -> i64 {
                slices
                    .iter()
                    .map(|slice| {
                        let from = slice.start_time.max(start);
                        let to = slice.end_time.min(end);
                        (to - from).num_seconds().max(0)
                    })
                    .sum()
            };
            if overlap(&same_day) > overlap(&prev_day) {
                same_day
            } else {
                prev_day
            }
        }
        _ => prev_day,
    }
}

pub(super) fn workout_has_track_geometry(object: &Map<String, Value>) -> bool {
    for key in [
        "latitude",
        "longitude",
        "lat",
        "lon",
        "lng",
        "track_points",
        "trackPoints",
        "gps_points",
        "gpsPoints",
        "route",
        "polyline",
    ] {
        match object.get(key) {
            Some(Value::Array(items)) if !items.is_empty() => return true,
            Some(Value::Number(number)) if number.as_f64().is_some_and(|value| value != 0.0) => {
                return true;
            }
            Some(Value::String(text))
                if !text.trim().is_empty() && key != "location" && !text.starts_with("ws") =>
            {
                return true;
            }
            _ => {}
        }
    }
    false
}

pub(super) fn workout_sample_count(object: &Map<String, Value>) -> i64 {
    for key in [
        "sample_count",
        "sampleCount",
        "hr_samples",
        "heartRateSamples",
        "samples",
    ] {
        match object.get(key) {
            Some(Value::Array(items)) => return items.len() as i64,
            Some(Value::Number(number)) => {
                if let Some(value) = number.as_i64().filter(|value| *value > 0) {
                    return value;
                }
            }
            _ => {}
        }
    }
    0
}

/// Stage minutes: use the field if present. With no field but a stage array, sum by mode (it can be 0).
/// With neither field nor stages, the value is "not provided", not 0.
pub(super) fn optional_band_stage_minutes(
    sleep: &Map<String, Value>,
    names: &[&str],
    modes: &[i64],
) -> Option<i32> {
    if let Some(value) = first_number(sleep, names) {
        return Some(value.round() as i32);
    }
    sleep.get("stage").and_then(Value::as_array)?;
    Some(
        modes
            .iter()
            .map(|mode| band_stage_minutes(sleep, *mode))
            .sum(),
    )
}

pub(super) fn band_stage_minutes(sleep: &Map<String, Value>, expected_mode: i64) -> i32 {
    sleep
        .get("stage")
        .and_then(Value::as_array)
        .map(|stages| {
            stages
                .iter()
                .filter_map(Value::as_object)
                .filter(|stage| {
                    first_number(stage, &["mode"])
                        .map(|value| value.round() as i64 == expected_mode)
                        .unwrap_or(false)
                })
                .filter_map(|stage| {
                    let start = first_number(stage, &["start"])? as i64;
                    let stop = first_number(stage, &["stop"])? as i64;
                    (stop >= start).then_some((stop - start + 1) as i32)
                })
                .sum()
        })
        .unwrap_or(0)
}

pub(super) fn heart_rate_from_band_item(
    item: &Map<String, Value>,
    decoded_summary: Option<&Value>,
    zone: Option<&str>,
) -> std::result::Result<Vec<MetricSample>, String> {
    let Some(encoded) = item.get("data_hr").and_then(Value::as_str) else {
        return Ok(Vec::new());
    };
    let day = first_string(item, &["date_time", "date", "dayId"])
        .and_then(|value| NaiveDate::parse_from_str(&value, "%Y-%m-%d").ok())
        .ok_or_else(|| "data_hr missing a valid date".to_string())?;
    let bytes = STANDARD
        .decode(encoded.trim())
        .map_err(|error| format!("data_hr Base64 invalid: {error}"))?;
    // A missing tz must not be read as UTC 0: old firmware often omits this field, and a silent 0 shifts the day's
    // heart rate samples by several time zones (same trap as in sleep_stages_from_band).
    let Some(timezone_offset) = decoded_summary
        .and_then(Value::as_object)
        .and_then(|summary| first_value(summary, &["tz"]))
        .and_then(timezone_offset_seconds)
        .map(|value| value.clamp(-18 * 3600, 18 * 3600))
        .map(|value| effective_band_offset(value, day, zone))
    else {
        return Ok(Vec::new());
    };
    let local_midnight = day
        .and_hms_opt(0, 0, 0)
        .ok_or_else(|| "data_hr date cannot be built".to_string())?;
    let utc_midnight = DateTime::<Utc>::from_naive_utc_and_offset(
        local_midnight - Duration::seconds(timezone_offset),
        Utc,
    );
    let source_device = device_id(item);
    Ok(bytes
        .into_iter()
        .take(1440)
        .enumerate()
        .filter(|(_, value)| (20..=240).contains(value))
        .map(|(minute, value)| MetricSample {
            metric: "heart_rate".into(),
            timestamp: utc_midnight + Duration::minutes(minute as i64),
            value: f64::from(value),
            unit: "bpm".into(),
            source_scope: SourceScope::Device,
            device_id: source_device.clone(),
        })
        .collect())
}

pub(super) fn daily_metrics_from_band_summary(
    item: &Map<String, Value>,
    summary: &Map<String, Value>,
    scope: SourceScope,
) -> Vec<DailyMetric> {
    let Some(date) = first_string(item, &["date_time", "date", "dayId"])
        .and_then(|value| NaiveDate::parse_from_str(&value, "%Y-%m-%d").ok())
        .map(|value| value.format("%Y-%m-%d").to_string())
    else {
        return Vec::new();
    };
    let source_device = device_id(item);
    let mut metrics = Vec::new();
    if let Some(sleep) = summary.get("slp").and_then(Value::as_object) {
        if let Some(value) =
            first_number(sleep, &["rhr"]).filter(|value| (20.0..=250.0).contains(value))
        {
            metrics.push(DailyMetric {
                date: date.clone(),
                metric: "resting_hr".into(),
                value,
                unit: "bpm".into(),
                source_scope: scope.clone(),
                device_id: source_device.clone(),
            });
        }
    }
    if let Some(activity) = summary.get("stp").and_then(Value::as_object) {
        for (metric, names, unit) in [
            ("steps", &["ttl"][..], "steps"),
            ("active_calories", &["cal"][..], "kcal"),
            ("distance", &["dis"][..], "m"),
        ] {
            if let Some(value) = first_number(activity, names).filter(|value| *value >= 0.0) {
                metrics.push(DailyMetric {
                    date: date.clone(),
                    metric: metric.into(),
                    value,
                    unit: unit.into(),
                    source_scope: scope.clone(),
                    device_id: source_device.clone(),
                });
            }
        }
    }
    metrics
}

pub(super) fn collect_charge_metrics(
    event: &Map<String, Value>,
    value: &Map<String, Value>,
    records: &mut Vec<DailyMetric>,
) -> usize {
    let Some(date) = summary_date(event, Some(value)) else {
        return 0;
    };
    let Some(samples) = value.get("samples").and_then(Value::as_array) else {
        return 0;
    };
    let latest = samples
        .iter()
        .filter_map(Value::as_object)
        .filter(|sample| {
            first_number(sample, &["total"])
                .map(|score| (0.0..=100.0).contains(&score))
                .unwrap_or(false)
        })
        .max_by_key(|sample| {
            first_number(sample, &["s", "offset"])
                .map(|offset| offset.round() as i64)
                .unwrap_or(0)
        });
    let Some(sample) = latest else {
        return 0;
    };
    let source_device = device_id(value).or_else(|| device_id(event));
    // The event object carries `eventType`; the inner value object does not,
    // and Charge is an account-level aggregate.
    let scope = source_scope(Some(event), source_device.as_deref());
    let mut count = 0;
    for (metric, field) in [
        ("hybrid_charge", "total"),
        ("physical_charge", "physical"),
        ("mental_charge", "mental"),
    ] {
        if let Some(score) = first_number(sample, &[field])
            .filter(|score| score.is_finite() && (0.0..=100.0).contains(score))
        {
            records.push(DailyMetric {
                date: date.clone(),
                metric: metric.into(),
                value: score,
                unit: "score".into(),
                source_scope: scope.clone(),
                device_id: source_device.clone(),
            });
            count += 1;
        }
    }
    count
}

/// A Charge entry that is recognized but has no score that day: the date is readable and `samples` exists, but every cell's
/// `total` is a sentinel (the watch gives 255 when it computed no energy value that day).
///
/// It must stay separate from "unknown payloads". After per-day storage such a day is its own payload. Treating it as
/// a parse failure would make every sync report "a response has no recognizable records" and quarantine a good payload.
pub(super) fn is_scoreless_charge_day(event: &Map<String, Value>) -> bool {
    if first_string(event, &["eventType"]).as_deref() != Some("Charge") {
        return false;
    }
    let Some(value) = event.get("value").and_then(Value::as_object) else {
        return false;
    };
    let Some(samples) = value.get("samples").and_then(Value::as_array) else {
        return false;
    };
    summary_date(event, Some(value)).is_some()
        && !samples.is_empty()
        && samples.iter().all(|sample| {
            sample.as_object().is_some_and(|sample| {
                !first_number(sample, &["total"])
                    .is_some_and(|score| (0.0..=100.0).contains(&score))
            })
        })
}

/// `(metric name, candidate keys in the payload, unit, trusted range)`.
///
/// It has its own name because clippy `type_complexity` rejects a 4-tuple nested in a 2-tuple,
/// and all four are required. The range matters most: it is the rule for "what this field's sentinel looks like".
/// Each field differs; one shared rule would write sentinels into the database as readings.
pub(super) type SentinelMetricSpec = (
    &'static str,
    &'static [&'static str],
    &'static str,
    (f64, f64),
);

pub(super) fn collect_daily_metrics(
    object: &Map<String, Value>,
    parent: Option<&Map<String, Value>>,
    source_device: Option<String>,
    records: &mut Vec<DailyMetric>,
) -> usize {
    let Some(date) = summary_date(object, parent) else {
        return 0;
    };
    let scope_source = parent.unwrap_or(object);
    let scope = source_scope(Some(scope_source), source_device.as_deref());
    let metric_fields: [(&str, &[&str], &str); 21] = [
        (
            "steps",
            &["steps", "step", "stepCount", "totalSteps"],
            "steps",
        ),
        (
            "calories",
            &["calories", "calorie", "totalCalories"],
            "kcal",
        ),
        (
            "active_minutes",
            &["activeMinutes", "totalBurningDuration"],
            "min",
        ),
        ("distance", &["distance", "totalDistance"], "m"),
        (
            "resting_hr",
            &["resting_hr", "restingHr", "restingHeartRate", "rhr"],
            "bpm",
        ),
        (
            "readiness",
            &["readiness", "readinessScore", "watchScore", "rdnsScore"],
            "score",
        ),
        ("physical_readiness", &["phyScore"], "score"),
        ("mental_readiness", &["mentScore"], "score"),
        ("hrv_readiness", &["hrvScore"], "score"),
        ("rhr_readiness", &["rhrScore"], "score"),
        ("skin_temp_readiness", &["skinTempScore"], "score"),
        ("afib_readiness", &["afibScore"], "score"),
        ("ahi_readiness", &["ahiScore"], "score"),
        (
            "bio_charge",
            &["bio_charge", "bioCharge", "bodyBattery", "chargeScore"],
            "score",
        ),
        (
            "hybrid_charge",
            &["hybrid_charge", "hybridCharge", "hybridChargeScore"],
            "score",
        ),
        (
            "training_load",
            &[
                "training_load",
                "trainingLoad",
                "wtlSum",
                "currnetDayTrainLoad",
            ],
            "load",
        ),
        (
            "vo2max",
            &[
                "vo2max",
                "vo2Max",
                "VO2_MAX",
                "VO2_max",
                "vo2_max_run",
                "vo2_max_walking",
            ],
            "ml/kg/min",
        ),
        ("stress", &["stress", "stressScore"], "score"),
        ("spo2", &["spo2", "bloodOxygen", "blood_oxygen"], "%"),
        ("running_distance", &["totalRunningDistance"], "m"),
        ("cycling_distance", &["totalCyclingDistance"], "m"),
    ];
    let mut count = 0;
    for (metric, names, unit) in metric_fields {
        if let Some(value) = first_number_from(object, parent, names).filter(|value| {
            value.is_finite()
                && !((metric == "readiness" || metric.ends_with("_readiness")) && *value == 255.0)
        }) {
            records.push(DailyMetric {
                date: date.clone(),
                metric: metric.into(),
                value,
                unit: unit.into(),
                source_scope: scope.clone(),
                device_id: source_device.clone(),
            });
            count += 1;
        }
    }

    // The fields that were always in the cloud summary but never read before.
    //
    // These cannot share the `is_finite()` check of the table above: this stream uses **sentinel values** for
    // "not measured", and each field has its own sentinel. All were counted from 25,348 local readiness
    // records:
    //
    // * `sleepHRV` measured 44-133, `sleepRHR` 43-75; neither ever showed a sentinel;
    // * `hrvBaseline` / `rhrBaseline` each have 7 entries of 255. In this stream 255 means
    //   "not measured" (`afibScore` is 255 in all 25,348 entries);
    // * `ahiBaseline` has 7 entries of -1; the rest fall in 0-0.49;
    // * the three goal values are set by the user; 0 means not set, not "a goal of 0 steps".
    //
    // **`phyBaseline` / `mentBaseLine` are not read**: in tests they equal `phyScore` /
    // `mentScore` **exactly in every one** of 25,348 records. They are not baselines; they are the same score
    // under another name. Reading them would only add two identical columns to the database.
    let sentinel_fields: [SentinelMetricSpec; 8] = [
        ("sleep_hrv", &["sleepHRV"], "ms", (1.0, 254.0)),
        ("sleep_rhr", &["sleepRHR"], "bpm", (25.0, 120.0)),
        ("hrv_baseline", &["hrvBaseline"], "ms", (1.0, 254.0)),
        ("rhr_baseline", &["rhrBaseline"], "bpm", (25.0, 120.0)),
        ("ahi_baseline", &["ahiBaseline"], "events/h", (0.0, 100.0)),
        ("step_goal", &["stepGoal"], "steps", (1.0, 100_000.0)),
        ("calorie_goal", &["calorieGoal"], "kcal", (1.0, 10_000.0)),
        (
            "active_minutes_goal",
            &["burningDurationGoal"],
            "min",
            (1.0, 1440.0),
        ),
    ];
    for (metric, names, unit, range) in sentinel_fields {
        if let Some(value) = first_number_from(object, parent, names)
            .filter(|value| value.is_finite() && (range.0..=range.1).contains(value))
        {
            records.push(DailyMetric {
                date: date.clone(),
                metric: metric.into(),
                value,
                unit: unit.into(),
                source_scope: scope.clone(),
                device_id: source_device.clone(),
            });
            count += 1;
        }
    }

    let event_type = parent
        .and_then(|value| first_string(value, &["eventType"]))
        .or_else(|| first_string(object, &["eventType"]));
    if count == 0 {
        let mapped_metric = match event_type.as_deref() {
            Some("Charge") => Some(("bio_charge", "score")),
            Some("readiness") => Some(("readiness", "score")),
            _ => None,
        };
        if let Some((metric, unit)) = mapped_metric {
            if let Some(value) = first_value(object, &["value", "score", "charge"])
                .and_then(parse_number)
                .filter(|value| value.is_finite() && !(metric == "readiness" && *value == 255.0))
            {
                records.push(DailyMetric {
                    date,
                    metric: metric.into(),
                    value,
                    unit: unit.into(),
                    source_scope: scope,
                    device_id: source_device,
                });
                count += 1;
            }
        }
    }
    count
}

/// The offset that the band summary `tz` (seconds) should really use on this day.
///
/// In 2026-10 two European users reported "everything is 1 hour early" and "4 o'clock data shows as 5 o'clock", while the Zepp app was right:
/// `data_hr` / sleep stages are minute indexes from local midnight, which the app draws as clock times. We convert with `tz` to UTC.
/// If `tz` gives the **standard time** offset during DST, the result is one hour late. China has no DST,
/// so the dev database does not show it. The fix applies only when all three are true; otherwise nothing changes:
/// - the device IANA time zone is known;
/// - the zone is in DST on this day (actual offset != standard offset; standard offset is the smaller of January / July);
/// - `tz` equals the standard offset exactly.
///
/// Payloads whose `tz` already includes DST (equal to the actual offset) are not affected; a switch day still uses one offset per day.
pub(crate) fn effective_band_offset(tz: i64, day: NaiveDate, zone: Option<&str>) -> i64 {
    let Some(zone) = zone.and_then(|name| jiff::tz::TimeZone::get(name).ok()) else {
        return tz;
    };
    let offset_at = |month: i8, day_of_month: i8| -> Option<i64> {
        let date = jiff::civil::Date::new(day.year() as i16, month, day_of_month).ok()?;
        let zoned = date.at(12, 0, 0, 0).to_zoned(zone.clone()).ok()?;
        Some(i64::from(zoned.offset().seconds()))
    };
    let (Some(actual), Some(january), Some(july)) = (
        offset_at(day.month() as i8, day.day() as i8),
        offset_at(1, 15),
        offset_at(7, 15),
    ) else {
        return tz;
    };
    let standard = january.min(july);
    if actual != standard && tz == standard {
        actual
    } else {
        tz
    }
}
