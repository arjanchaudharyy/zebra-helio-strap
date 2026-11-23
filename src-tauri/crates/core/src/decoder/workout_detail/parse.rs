//! Small parsers for delta strings and value strings, pause ranges, and time-based fill (split out of decoder/workout_detail.rs, logic unchanged).

use super::*;

pub(super) fn detail_object(raw: &Value) -> Option<&serde_json::Map<String, Value>> {
    if let Some(data) = raw.get("data").and_then(Value::as_object) {
        if data.contains_key("trackid") || data.contains_key("longitude_latitude") {
            return Some(data);
        }
    }
    raw.as_object()
}

pub(super) fn parse_i64(value: Option<&Value>) -> Option<i64> {
    match value? {
        Value::Number(number) => number.as_i64(),
        Value::String(text) => text.trim().parse().ok(),
        _ => None,
    }
}

/// When a semicolon segment fails to parse, keep a `None` placeholder and do not drop the index. In the delta format
/// the segment number is position data, and silent compaction shifts every later delta.
pub(super) fn parse_int_list(value: Option<&Value>) -> Vec<Option<i32>> {
    let Some(text) = value.and_then(Value::as_str) else {
        return Vec::new();
    };
    text.split(';')
        .filter(|part| !part.is_empty())
        .map(|part| part.trim().parse().ok())
        .collect()
}

pub(super) fn parse_coordinate_deltas(
    value: Option<&Value>,
) -> (Vec<Option<i64>>, Vec<Option<i64>>) {
    let Some(text) = value.and_then(Value::as_str) else {
        return (Vec::new(), Vec::new());
    };
    let mut latitudes = Vec::new();
    let mut longitudes = Vec::new();
    for part in text.split(';').filter(|part| !part.is_empty()) {
        let mut bits = part.split(',');
        let lat = bits.next().and_then(|item| item.parse().ok());
        let lon = bits.next().and_then(|item| item.parse().ok());
        latitudes.push(lat);
        longitudes.push(lon);
    }
    (latitudes, longitudes)
}

pub(super) fn parse_altitude_cm(value: Option<&Value>) -> Vec<Option<i64>> {
    let mut values = parse_int_list(value)
        .into_iter()
        .map(|item| item.map(i64::from))
        .collect::<Vec<_>>();
    if let Some(first_valid) = values
        .iter()
        .position(|value| value.is_some_and(is_plausible_altitude_cm))
    {
        let fill = values[first_valid];
        for item in values.iter_mut().take(first_valid) {
            *item = fill;
        }
    }
    values
}

pub(super) fn is_plausible_altitude_cm(cm: i64) -> bool {
    (MIN_PLAUSIBLE_ALTITUDE_CM..=MAX_PLAUSIBLE_ALTITUDE_CM).contains(&cm)
}

pub(super) fn cm_to_meters(cm: i64) -> Option<f64> {
    is_plausible_altitude_cm(cm).then(|| cm as f64 / 100.0)
}

/// Parses one `dt,value;dt,value;...` string.
///
/// **A garbled delta drops the whole row instead of counting as 0.** This used to be
/// `raw_delta.parse().unwrap_or(0)`: a non-empty delta that did not parse put this
/// sample on the same timestamp as the previous one. At best that duplicates samples; at worst it corrupts pace, power,
/// running form and splits, and the UI looks normal because the sample count is right.
///
/// An empty delta is different: in the protocol it means "the next second". `empty_delta_is_one`
/// turns this on explicitly. It is not a guess.
pub(super) fn parse_delta_pairs(
    value: Option<&Value>,
    empty_delta_is_one: bool,
) -> Vec<(i64, i32)> {
    parse_delta_pairs_by(value, empty_delta_is_one, |raw| raw.parse::<i32>().ok())
}

/// Same as `parse_delta_pairs`, but values can have decimals and are read as the nearest integer.
///
/// #159: some watches write `currentDistance` as `100000.00000`. Integer parsing dropped every row,
/// so there was no cumulative distance reading and no kilometer splits. Use this only for distance: decimals in heart rate
/// or altitude are a different case and must not be rounded silently.
pub(super) fn parse_rounded_delta_pairs(
    value: Option<&Value>,
    empty_delta_is_one: bool,
) -> Vec<(i64, i32)> {
    parse_delta_pairs_by(value, empty_delta_is_one, |raw| {
        let parsed = raw.trim().parse::<f64>().ok()?;
        let rounded = parsed.round();
        (parsed.is_finite() && rounded >= i32::MIN as f64 && rounded <= i32::MAX as f64)
            .then_some(rounded as i32)
    })
}

fn parse_delta_pairs_by(
    value: Option<&Value>,
    empty_delta_is_one: bool,
    parse_value: impl Fn(&str) -> Option<i32>,
) -> Vec<(i64, i32)> {
    let Some(text) = value.and_then(Value::as_str) else {
        return Vec::new();
    };
    let mut pairs = Vec::new();
    for part in text.split(';').filter(|part| !part.is_empty()) {
        let mut bits = part.splitn(2, ',');
        let raw_delta = bits.next().unwrap_or("").trim();
        let raw_value = bits.next().unwrap_or("");
        let delta = if raw_delta.is_empty() {
            if empty_delta_is_one {
                1
            } else {
                // This stream has no "empty = 1 second" rule, so an empty delta is
                // unreadable data. Skip it; do not invent a value.
                continue;
            }
        } else {
            match raw_delta.parse::<i64>() {
                Ok(parsed) => parsed,
                Err(_) => continue,
            }
        };
        let Some(sample) = parse_value(raw_value) else {
            continue;
        };
        pairs.push((delta, sample));
    }
    pairs
}

pub(super) fn parse_float_pairs(value: Option<&Value>) -> Vec<(i64, f64)> {
    let Some(text) = value.and_then(Value::as_str) else {
        return Vec::new();
    };
    let mut pairs = Vec::new();
    for part in text.split(';').filter(|part| !part.is_empty()) {
        let mut bits = part.splitn(2, ',');
        let Some(delta) = bits.next().and_then(|item| item.parse().ok()) else {
            continue;
        };
        let Some(sample) = bits.next().and_then(|item| item.parse().ok()) else {
            continue;
        };
        pairs.push((delta, sample));
    }
    pairs
}

/// `(dt, value)` pairs where the value is a plain reading rather than a delta.
///
/// Unlike `parse_float_pairs` an empty leading delta means "one second later",
/// the same convention `parse_delta_pairs` uses, because real `power_meter`
/// and `equivPace` strings contain them.
pub(super) fn parse_valued_pairs(value: Option<&Value>) -> Vec<(i64, f64)> {
    let Some(text) = value.and_then(Value::as_str) else {
        return Vec::new();
    };
    let mut pairs = Vec::new();
    for part in text.split(';').filter(|part| !part.is_empty()) {
        let mut bits = part.splitn(2, ',');
        let raw_delta = bits.next().unwrap_or("").trim();
        // Same as `parse_delta_pairs`: an empty delta is "the next second" in the protocol. A garbled delta
        // is not 0, it is unreadable. Skip that row.
        let delta = if raw_delta.is_empty() {
            1
        } else {
            match raw_delta.parse::<i64>() {
                Ok(parsed) => parsed,
                Err(_) => continue,
            }
        };
        let Some(sample) = bits.next().and_then(|item| item.trim().parse::<f64>().ok()) else {
            continue;
        };
        pairs.push((delta, sample));
    }
    pairs
}

/// `(dt, ground contact, vertical oscillation, vertical stride ratio)`.
pub(super) type PostureRow = (i64, i32, i32, i32);

pub(super) fn parse_run_posture(value: Option<&Value>) -> Vec<PostureRow> {
    let Some(text) = value.and_then(Value::as_str) else {
        return Vec::new();
    };
    let mut rows = Vec::new();
    for part in text.split(';').filter(|part| !part.is_empty()) {
        let bits: Vec<&str> = part.split(',').collect();
        if bits.len() < 4 {
            continue;
        }
        let raw_delta = bits[0].trim();
        let delta = if raw_delta.is_empty() {
            1
        } else {
            match raw_delta.parse::<i64>() {
                Ok(value) => value,
                Err(_) => continue,
            }
        };
        let (Ok(contact), Ok(oscillation), Ok(ratio)) = (
            bits[1].trim().parse::<i32>(),
            bits[2].trim().parse::<i32>(),
            bits[3].trim().parse::<i32>(),
        ) else {
            continue;
        };
        rows.push((delta, contact, oscillation, ratio));
    }
    rows
}

/// Map a device sentinel to NaN so the fill never carries it forward as a
/// real reading; `finite_at` then drops it.
pub(super) fn sentinel_free(value: i32, sentinel: i32) -> f64 {
    if value == sentinel || value < 0 {
        f64::NAN
    } else {
        f64::from(value)
    }
}

pub(super) fn finite_at(
    map: Option<&std::collections::HashMap<i64, f64>>,
    unix_ts: i64,
) -> Option<f64> {
    map.and_then(|map| map.get(&unix_ts).copied())
        .filter(|value| value.is_finite())
}

pub(super) fn parse_gait(value: Option<&Value>) -> Vec<(i64, i32, i32, i32)> {
    let Some(text) = value.and_then(Value::as_str) else {
        return Vec::new();
    };
    let mut rows = Vec::new();
    for part in text.split(';').filter(|part| !part.is_empty()) {
        let bits: Vec<&str> = part.split(',').collect();
        if bits.len() < 4 {
            continue;
        }
        let (Ok(delta), Ok(steps), Ok(stride), Ok(cadence)) = (
            bits[0].parse::<i64>(),
            bits[1].parse::<i32>(),
            bits[2].parse::<i32>(),
            bits[3].parse::<i32>(),
        ) else {
            continue;
        };
        rows.push((delta, steps, stride, cadence));
    }
    rows
}

/// Pause ranges, `start,duration,-1,-1,kind;...`.
///
/// **The kind no longer filters rows** (#139). The old code kept only 2 / 3 / 0 and dropped other kinds.
/// One Balance 2 user had 127 details with `pause` and not one pause was stored, so the FIT file
/// had no timer stop events and Strava counted all 89 minutes as moving. The kind is only a label: 2 is manual,
/// 3 is auto, 0 is unknown, other numbers are other, and the range is always kept.
///
/// The start also has more than one format: Unix seconds (all 423 rows in our data), Unix
/// milliseconds, or seconds since the workout start. The second column can be a duration or an absolute
/// end time. The range must fall within 48 hours after the workout start. If it does not, the columns
/// were misread, so drop the row instead of forcing it in.
pub(super) fn parse_pauses(value: Option<&Value>, from: i64) -> Vec<PauseInterval> {
    let Some(text) = value.and_then(Value::as_str) else {
        return Vec::new();
    };
    let mut pauses = Vec::new();
    for part in text.split(';').filter(|part| !part.trim().is_empty()) {
        let bits: Vec<&str> = part.split(',').map(str::trim).collect();
        if bits.len() < 2 {
            continue;
        }
        let (Ok(raw_start), Ok(raw_second)) = (bits[0].parse::<i64>(), bits[1].parse::<i64>())
        else {
            continue;
        };
        let kind = match bits.get(4).and_then(|kind| kind.parse::<i32>().ok()) {
            Some(2) => "manual",
            Some(3) => "auto",
            Some(0) | None => "unknown",
            Some(_) => "other",
        };
        let Some((start, end)) = pause_bounds(raw_start, raw_second, from) else {
            continue;
        };
        // Bound only by the 48-hour limit, not by `to`. An indoor workout can have no summary end time
        // and no time deltas, so `to` is only start + 1 second, and clipping by it removes every pause. FIT export
        // clamps again to the workout start and end.
        let limit = from.saturating_add(MAX_ACTIVITY_SECONDS);
        if start < from || start >= limit || end <= start || end > limit {
            continue;
        }
        let (Some(start_time), Some(end_time)) = (
            Utc.timestamp_opt(start, 0).single(),
            Utc.timestamp_opt(end, 0).single(),
        ) else {
            continue;
        };
        if end_time <= start_time {
            continue;
        }
        pauses.push(PauseInterval {
            start_time,
            end_time,
            kind: kind.into(),
        });
    }
    pauses.sort_by_key(|pause| pause.start_time);
    pauses
}

/// The `(start, end)` of one pause row, in Unix seconds.
///
/// * start >= 10^11: millisecond timestamp, and the second column is also read as milliseconds.
/// * start < 10^9: seconds since the workout start (`from`).
/// * second column greater than the start and itself like a timestamp: absolute end time. Otherwise it is a duration.
fn pause_bounds(raw_start: i64, raw_second: i64, from: i64) -> Option<(i64, i64)> {
    const MILLIS_THRESHOLD: i64 = 100_000_000_000;
    const UNIX_THRESHOLD: i64 = 1_000_000_000;
    if raw_second < 0 {
        return None;
    }
    let (start, second) = if raw_start >= MILLIS_THRESHOLD {
        // A millisecond end time and a millisecond duration both become seconds after / 1000. The code below tells them apart.
        (raw_start / 1000, raw_second / 1000)
    } else if raw_start < UNIX_THRESHOLD {
        (from.checked_add(raw_start)?, raw_second)
    } else {
        (raw_start, raw_second)
    };
    let end = if second >= UNIX_THRESHOLD && second > start {
        second
    } else {
        start.checked_add(second)?
    };
    Some((start, end))
}

pub(super) fn timed_cumulative_i32(
    from: i64,
    to: i64,
    elements: &[(i64, i32)],
) -> std::collections::HashMap<i64, i32> {
    timed_fill(from, to, elements, 0, |current, delta| {
        current.saturating_add(*delta)
    })
}

pub(super) fn timed_fixed_f64(
    from: i64,
    to: i64,
    elements: &[(i64, f64)],
) -> std::collections::HashMap<i64, f64> {
    timed_fill(from, to, elements, 0.0, |_, value| *value)
}

pub(super) fn timed_fill<T: Copy>(
    from: i64,
    to: i64,
    elements: &[(i64, T)],
    init: T,
    update: impl Fn(T, &T) -> T,
) -> std::collections::HashMap<i64, T> {
    let mut result = std::collections::HashMap::new();
    let mut working = from;
    let mut value = init;
    let limit = to.saturating_add(1);
    for (index, (delta, sample)) in elements.iter().enumerate() {
        if *delta < 0 {
            // A negative span on the first item is the baseline reading "just before the track starts". Real
            // heart rate strings start with `-1,105;`, and every later item is a delta from it. Its value
            // must go into the running total. The old code skipped it too, so the whole curve lost its base, and one
            // "AI-detected activity" showed heart rate as 1-43 bpm while the summary max was 148
            // (= 105 + 43). It takes no time, so the next item starts from `from` as usual.
            //
            // A negative span anywhere else cannot be valid. Whether or not the total uses `update`, it
            // must not merge in silently, or this one bad point shifts the base of every later point
            // with no trace. `delta == 0` (an extra reading in the same second) is normal
            // and still merges in.
            if index == 0 {
                value = update(value, sample);
            }
            continue;
        }
        value = update(value, sample);
        let start = if index == 0 { 0 } else { 1 };
        if *delta >= start {
            for _ in start..=*delta {
                result.insert(working, value);
                match working.checked_add(1) {
                    Some(next) => {
                        working = next;
                        if working > limit {
                            break;
                        }
                    }
                    None => break,
                }
            }
        }
    }
    while working <= to {
        result.insert(working, value);
        match working.checked_add(1) {
            Some(next) => working = next,
            None => break,
        }
    }
    result
}
