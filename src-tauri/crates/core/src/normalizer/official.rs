//! Normalization of Zepp official open platform payloads.
//!
//! Each payload is a `{ items, timeZone }` that `official/fetch.rs` split by day (workouts per record, weight per weigh-in).
//! Where the old channel parser fits, the fields are renamed to old channel names and passed to it (workout summary, workout detail, PAI, weight).
//! Both sides use the same backend with the same data format; only the field names are camelCase. Sleep, per-minute heart rate, and daily / hourly
//! steps have different shapes and are parsed here.
//!
//! Three rules apply (`docs/reference/official-api-facts.zh-CN.md` section 3): the official `sleepHrv` goes into no HRV column;
//! missing values are not filled with 0; unknown values are not guessed.

use super::band::normalize_type_text;
use super::*;
use serde_json::json;

/// One official night of sleep, plus the two `sleep_sessions` columns that only the official channel can fill.
#[derive(Debug, Clone)]
pub struct OfficialSleep {
    pub session: SleepSession,
    /// The real REM value from the official channel (seconds).
    pub rem_seconds: Option<i64>,
    /// Total nap time of the day (seconds). A `napStage` array that is present but empty means "no nap this day", so record 0;
    /// only a missing field is unknown.
    pub nap_total_seconds: Option<i64>,
    /// The nap stage segments (each `napStage` item is also `{mode, start, stop}`, with the same codes and the same local
    /// midnight as the night). When the night stages do not match a midnight, no segment is kept; naps are not shifted by a guessed midnight.
    pub nap_stages: Vec<SleepStageSlice>,
}

fn items(raw: &Value) -> &[Value] {
    raw.get("items")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

fn time_zone(raw: &Value) -> Option<&str> {
    raw.get("timeZone")
        .and_then(Value::as_str)
        .filter(|zone| !zone.is_empty())
}

fn int(object: &Map<String, Value>, key: &str) -> Option<i64> {
    first_number(object, &[key])
        .filter(|value| value.is_finite())
        .map(|value| value.round() as i64)
}

/// A span of minutes (`stop` is inclusive, same as the old channel).
fn stage_span_minutes(stage: &Map<String, Value>) -> Option<i64> {
    let start = int(stage, "start")?;
    let stop = int(stage, "stop")?;
    (stop >= start).then_some(stop - start + 1)
}

/// Official sleep `stage.start/stop` are minutes from some local midnight, but the payload has no time zone.
///
/// Derive that midnight from the session start: `start - earliest segment minutes x 60`, then align to 15 minutes
/// (all real time zone offsets are whole multiples of 15 minutes). A mismatch means the stages and the session of that day do not match.
/// Then no stage bar is drawn; the whole night is not shifted by a guessed midnight.
pub fn official_stage_anchor(session_start: i64, earliest_stage_minute: i64) -> Option<i64> {
    let raw = session_start.checked_sub(earliest_stage_minute.checked_mul(60)?)?;
    let snapped = (raw as f64 / 900.0).round() as i64 * 900;
    // In tests both match to the second; allow two minutes for rounding of the session start. Any whole-minute midnight is at most 7.5 minutes from the 15-minute grid,
    // so the margin must not grow to minutes or more, or this check does nothing.
    ((raw - snapped).abs() <= 120).then_some(snapped)
}

fn official_stages(stages: &[Value], anchor: i64) -> Vec<SleepStageSlice> {
    stages
        .iter()
        .filter_map(Value::as_object)
        .filter_map(|stage| {
            let mode = int(stage, "mode")?;
            let start = int(stage, "start")?;
            let stop = int(stage, "stop")?;
            if stop < start || !(0..=4 * 1440).contains(&start) {
                return None;
            }
            let name = sleep_stage_name(mode);
            Some(SleepStageSlice {
                stage: name.to_string(),
                start_time: DateTime::from_timestamp(anchor + start * 60, 0)?,
                end_time: DateTime::from_timestamp(anchor + (stop + 1) * 60, 0)?,
                raw_mode: (name == "unknown").then_some(mode),
            })
        })
        .collect()
}

/// Official sport type name -> key in this app's sport catalog. Unknown names stay as is in lowercase (`circuit_mode`); no guessing.
pub fn official_workout_type(value: &str) -> String {
    let upper = value.trim().to_uppercase();
    let mapped = match upper.as_str() {
        "OUTDOOR_RUN" | "OUTDOOR_RUNNING" | "RUNNING" => "run",
        "TREADMILL" | "INDOOR_RUN" | "INDOOR_RUNNING" => "treadmill",
        "WALKING" | "OUTDOOR_WALKING" => "walking",
        "INDOOR_WALKING" => "indoor_walking",
        "OUTDOOR_CYCLING" | "CYCLING" => "ride",
        "INDOOR_CYCLING" => "indoor_cycling",
        "MOUNTAINEER" | "MOUNTAINEERING" | "HIKING" => "hiking",
        "TRAIL_RUN" | "TRAIL_RUNNING" => "trail_running",
        "POOL_SWIMMING" => "pool_swimming",
        "OPEN_WATER_SWIMMING" => "open_water_swimming",
        "STRENGTH_TRAINING" | "STRENGTH" => "strength",
        "FREE_TRAINING" => "free_training",
        "YOGA" => "yoga",
        "ROCK_CLIMBING" => "rock_climbing",
        "ACTIVITY" => "activity",
        _ => return normalize_type_text(value),
    };
    mapped.to_string()
}

impl Normalizer {
    /// `/users/-/sleep?interval=daily`: one entry per day; a day counts as a night only when it has a session.
    pub fn normalize_official_sleep(raw: &Value) -> Vec<OfficialSleep> {
        let mut out = Vec::new();
        for item in items(raw).iter().filter_map(Value::as_object) {
            let (Some(start), Some(stop)) = (int(item, "start"), int(item, "stop")) else {
                continue;
            };
            // The official channel gives 0 for a day with no sleep.
            if start <= 0 || stop <= start {
                continue;
            }
            let (Some(start_time), Some(end_time)) = (
                DateTime::from_timestamp(start, 0),
                DateTime::from_timestamp(stop, 0),
            ) else {
                continue;
            };
            let minutes = |key: &str| {
                int(item, key)
                    .filter(|value| (0..=24 * 60).contains(value))
                    .map(|value| value as i32)
            };
            let deep_minutes = minutes("deepSleepTime");
            let light_minutes = minutes("shallowSleepTime");
            let awake_minutes = minutes("wakeTime");
            let rem_minutes = minutes("rem");
            let span = ((stop - start) / 60) as i32;
            let anchor = item
                .get("stage")
                .and_then(Value::as_array)
                .and_then(|stages| {
                    let earliest = stages
                        .iter()
                        .filter_map(Value::as_object)
                        .filter_map(|stage| int(stage, "start"))
                        .min()?;
                    official_stage_anchor(start, earliest)
                });
            let stages = item
                .get("stage")
                .and_then(Value::as_array)
                .zip(anchor)
                .map(|(stages, anchor)| official_stages(stages, anchor))
                .unwrap_or_default();
            let nap_stages = item
                .get("napStage")
                .and_then(Value::as_array)
                .zip(anchor)
                .map(|(naps, anchor)| official_stages(naps, anchor))
                .unwrap_or_default();
            // Empty array = really no nap (0). Entries with any unknown item = unknown (None).
            // Do not build a too-small total from the known items, and never sum an all-bad array to 0 (R10).
            let nap_total_seconds =
                item.get("napStage")
                    .and_then(Value::as_array)
                    .and_then(|naps| {
                        naps.iter()
                            .map(|nap| nap.as_object().and_then(stage_span_minutes))
                            .sum::<Option<i64>>()
                            .map(|minutes| minutes * 60)
                    });
            // Time asleep: with an awake time, use "in bed - awake". Without it, use the measured deep + light sleep
            // (+ REM). Do not treat an unknown awake time as 0, and do not pass time in bed off as time asleep (R10).
            // When neither exists, only time in bed is left. Also record it in time_in_bed so the UI knows.
            // Use whichever of deep, light and REM exist (before, both deep and light were required, else it fell back to time in bed).
            let staged = [deep_minutes, light_minutes, rem_minutes]
                .into_iter()
                .flatten()
                .sum::<i32>();
            let (duration_minutes, time_in_bed_minutes) = match awake_minutes {
                Some(awake) => ((span - awake).max(0), None),
                None if staged > 0 => (staged, Some(span)),
                None => (span, Some(span)),
            };
            out.push(OfficialSleep {
                session: SleepSession {
                    sleep_id: format!("official:{start}"),
                    start_time,
                    end_time,
                    score: int(item, "sleepScore")
                        .filter(|score| (1..=100).contains(score))
                        .map(|score| score as i32),
                    duration_minutes,
                    deep_minutes,
                    light_minutes,
                    rem_minutes,
                    awake_minutes,
                    source_scope: SourceScope::UserFused,
                    device_id: None,
                    synced_at: None,
                    time_in_bed_minutes,
                    stages,
                    wake_count: None,
                },
                rem_seconds: rem_minutes.map(|value| i64::from(value) * 60),
                nap_total_seconds,
                nap_stages,
            });
        }
        out
    }

    /// `/users/-/heartrates?type=ALL`: per minute. The official channel gives 0 for minutes without the watch; that is not a reading, so skip it.
    pub fn normalize_official_heart_rate(raw: &Value) -> Vec<MetricSample> {
        items(raw)
            .iter()
            .filter_map(Value::as_object)
            .filter_map(|item| {
                let value = first_number(item, &["heartRateData"])?;
                if !(25.0..=250.0).contains(&value) {
                    return None;
                }
                Some(MetricSample {
                    metric: "heart_rate".into(),
                    timestamp: DateTime::from_timestamp(int(item, "timestamp")?, 0)?,
                    value,
                    unit: "bpm".into(),
                    source_scope: SourceScope::UserFused,
                    device_id: None,
                })
            })
            .collect()
    }

    /// `/users/-/activities?interval=daily`: the same data as the old channel `stp`; metric names follow the old channel.
    pub fn normalize_official_activity_daily(raw: &Value) -> Vec<DailyMetric> {
        let mut out = Vec::new();
        for item in items(raw).iter().filter_map(Value::as_object) {
            let Some(date) = first_string(item, &["date"])
                .filter(|date| NaiveDate::parse_from_str(date, "%Y-%m-%d").is_ok())
            else {
                continue;
            };
            for (metric, key, unit) in [
                ("steps", "steps", "steps"),
                ("distance", "distance", "m"),
                ("active_calories", "calories", "kcal"),
            ] {
                if let Some(value) =
                    first_number(item, &[key]).filter(|value| value.is_finite() && *value >= 0.0)
                {
                    out.push(DailyMetric {
                        date: date.clone(),
                        metric: metric.into(),
                        value,
                        unit: unit.into(),
                        source_scope: SourceScope::UserFused,
                        device_id: None,
                    });
                }
            }
        }
        out
    }

    /// `/users/-/activities?interval=hourly`: the official channel returns only hours with steps. A missing hour means none;
    /// do not fill 0 (the chart shows a gap, not a 0 bar).
    ///
    /// The "hour" is in the time zone of the request, which the payload records in `timeZone`. Without it, nothing is stored.
    pub fn normalize_official_activity_hourly(raw: &Value) -> Vec<MetricSample> {
        let Some(zone) = time_zone(raw) else {
            return Vec::new();
        };
        items(raw)
            .iter()
            .filter_map(Value::as_object)
            .filter_map(|item| {
                let date =
                    NaiveDate::parse_from_str(&first_string(item, &["date"])?, "%Y-%m-%d").ok()?;
                let hour = int(item, "hour").filter(|hour| (0..24).contains(hour))?;
                let steps = first_number(item, &["steps"])
                    .filter(|value| value.is_finite() && *value >= 0.0)?;
                let midnight = crate::official::fetch::local_midnight_utc(date, zone);
                Some(MetricSample {
                    metric: "steps_hourly".into(),
                    timestamp: midnight + Duration::hours(hour),
                    value: steps,
                    unit: "steps".into(),
                    source_scope: SourceScope::UserFused,
                    device_id: None,
                })
            })
            .collect()
    }

    /// Official workout summary -> old channel workout summary field names, passed to [`Normalizer::normalize_workouts_with_sport`].
    pub fn official_sports_as_legacy(raw: &Value) -> Value {
        let items: Vec<Value> = items(raw)
            .iter()
            .filter_map(Value::as_object)
            .map(|item| {
                let mut object = item.clone();
                // The official `type` is a string (`OUTDOOR_RUN`); the old channel `type` is a numeric ID.
                if let Some(kind) = object
                    .remove("type")
                    .and_then(|kind| kind.as_str().map(str::to_string))
                {
                    object.insert(
                        "sportType".into(),
                        Value::String(official_workout_type(&kind)),
                    );
                }
                for (from, to) in [
                    ("averageStepFrequency", "avg_frequency"),
                    ("averageStrideLength", "avg_stride_length"),
                ] {
                    if let Some(value) = object.remove(from) {
                        object.insert(to.into(), value);
                    }
                }
                Value::Object(object)
            })
            .collect();
        json!({ "items": items })
    }

    /// Official workout detail -> old channel detail field names, passed to `decode_workout_detail`.
    ///
    /// `distanceInCentimeter` maps to `currentDistance` (both shaped as "seconds delta, cumulative cm"), but the unit was not checked against a real payload.
    /// On storage, per-point distance is kept only when it matches the summary distance; see `storage::official::drop_unreconciled_distance` (#143).
    pub fn official_detail_as_legacy(raw: &Value) -> Value {
        let mut data = Map::new();
        for (from, to) in [
            ("trackId", "trackid"),
            ("source", "source"),
            ("samplingTime", "time"),
            ("latitudeLongitude", "longitude_latitude"),
            ("altitude", "altitude"),
            ("heartRate", "heart_rate"),
            ("speed", "speed"),
            ("gait", "gait"),
            ("pause", "pause"),
            ("kilometerPace", "kilo_pace"),
            ("distanceInCentimeter", "currentDistance"),
        ] {
            if let Some(value) = raw
                .get(from)
                .filter(|value| !value.as_str().is_some_and(str::is_empty))
            {
                data.insert(to.into(), value.clone());
            }
        }
        json!({ "data": data })
    }

    /// Official weight `{weight, height, bmi, timestamp}` -> the old channel weightRecords shape.
    pub fn official_body_as_legacy(raw: &Value) -> Value {
        let items: Vec<Value> = items(raw)
            .iter()
            .filter_map(Value::as_object)
            .filter_map(|item| {
                let timestamp = item.get("timestamp")?.clone();
                let mut summary = Map::new();
                for key in ["weight", "height", "bmi"] {
                    if let Some(value) = item.get(key) {
                        summary.insert(key.into(), value.clone());
                    }
                }
                Some(json!({ "generatedTime": timestamp, "summary": summary }))
            })
            .collect();
        json!({ "items": items })
    }
}
