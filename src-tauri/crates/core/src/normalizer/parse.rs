//! Small value and parse helpers: field aliases, numbers, timestamps, dates and time zones (split out of normalizer/mod.rs).

use super::*;

pub(super) fn decode_base64_json(encoded: &str) -> Result<Value> {
    let bytes = STANDARD
        .decode(encoded.trim())
        .map_err(|error| ZeppBridgeError::ParseError(format!("Base64 invalid: {error}")))?;
    serde_json::from_slice(&bytes)
        .map_err(|error| ZeppBridgeError::ParseError(format!("Base64 content is not JSON: {error}")))
}

pub(super) fn extract_items(raw: &Value) -> Result<Vec<&Value>> {
    if let Some(items) = raw.as_array() {
        if items.is_empty() {
            return Err(ZeppBridgeError::DataUnavailable("response items is empty".into()));
        }
        return Ok(items.iter().collect());
    }
    let Some(object) = raw.as_object() else {
        return Err(ZeppBridgeError::ParseError(
            "response must be an object or an array".into(),
        ));
    };
    for key in ["items", "records", "results", "list"] {
        if let Some(array) = object.get(key).and_then(Value::as_array) {
            if array.is_empty() {
                return Err(ZeppBridgeError::DataUnavailable(format!("response {key} is empty")));
            }
            return Ok(array.iter().collect());
        }
    }
    if let Some(data) = object.get("data") {
        if let Some(array) = data.as_array() {
            if array.is_empty() {
                return Err(ZeppBridgeError::DataUnavailable("response data is empty".into()));
            }
            return Ok(array.iter().collect());
        }
        if let Some(data_object) = data.as_object() {
            for key in ["items", "records", "results", "list", "summary"] {
                if let Some(array) = data_object.get(key).and_then(Value::as_array) {
                    if array.is_empty() {
                        return Err(ZeppBridgeError::DataUnavailable(format!(
                            "response data.{key} is empty"
                        )));
                    }
                    return Ok(array.iter().collect());
                }
            }
        }
        if data.is_string() {
            return Err(ZeppBridgeError::DataUnavailable(
                "response data is an encoded string and cannot be safely decoded in full".into(),
            ));
        }
    }
    Err(ZeppBridgeError::ParseError(format!(
        "response has no items/data array, {}{}",
        crate::models::error::PARSE_KEYS_MARKER,
        object.keys().cloned().collect::<Vec<_>>().join(", ")
    )))
}

/// The cloud clearly returned an empty collection (`[]`, `data: []`, `data.items: []` and similar).
///
/// It accepts the same containers as `extract_items`: what that function reports as "empty" is `true` here.
/// This means "the cloud has nothing for this period", not "the payload is not understood". In the years before the account opened, the sleep endpoint
/// returned `{"code":1,"data":[]}` week by week. That was once recorded as a parse failure, and the backfill page showed a row of red blocks that retries did not clear.
pub(crate) fn payload_is_explicitly_empty(raw: &Value) -> bool {
    if let Some(items) = raw.as_array() {
        return items.is_empty();
    }
    let Some(object) = raw.as_object() else {
        return false;
    };
    for key in ["items", "records", "results", "list"] {
        if let Some(array) = object.get(key).and_then(Value::as_array) {
            return array.is_empty();
        }
    }
    match object.get("data") {
        Some(Value::Array(array)) => array.is_empty(),
        Some(Value::Object(data)) => ["items", "records", "results", "list", "summary"]
            .iter()
            .find_map(|key| data.get(*key).and_then(Value::as_array))
            .is_some_and(Vec::is_empty),
        _ => false,
    }
}

pub(super) fn item_object(value: &Value) -> Option<&Map<String, Value>> {
    let object = value.as_object()?;
    object
        .get("value")
        .and_then(Value::as_object)
        .filter(|nested| {
            nested.keys().any(|key| {
                matches!(
                    key.as_str(),
                    "timestamp" | "time" | "value" | "heartRate" | "steps" | "date"
                )
            })
        })
        .unwrap_or(object)
        .into()
}

pub(super) fn first_value<'a>(object: &'a Map<String, Value>, names: &[&str]) -> Option<&'a Value> {
    names
        .iter()
        .filter_map(|name| object.get(*name))
        .find(|value| {
            !value.is_null() && !value.as_str().is_some_and(|text| text.trim().is_empty())
        })
}

pub(super) fn first_value_from<'a>(
    object: &'a Map<String, Value>,
    nested: Option<&'a Map<String, Value>>,
    names: &[&str],
) -> Option<&'a Value> {
    first_value(object, names).or_else(|| nested.and_then(|value| first_value(value, names)))
}

pub(super) fn first_string(object: &Map<String, Value>, names: &[&str]) -> Option<String> {
    names
        .iter()
        .filter_map(|name| object.get(*name))
        .find_map(|value| match value {
            Value::String(value) if !value.trim().is_empty() => Some(value.clone()),
            Value::Number(value) => Some(value.to_string()),
            _ => None,
        })
}

/// Parses the cloud `heart_range`: the heart rate zone distribution.
///
/// The format is semicolon-separated `seconds,zone upper bound`, for example
/// `1882,113;3486,141;10,154;0,162;0,173;0,190`: 1882 seconds below 113,
/// 3486 seconds from 113 to 141, and so on. The zone bounds come from the user's watch settings. We do not have
/// those settings, so this distribution must come from the cloud. Our own split would give different numbers.
///
/// The sum of the segment seconds matches `run_time` in tests (one walk: 5378 vs 5403).
///
/// An all-zero distribution (0 seconds in every segment) returns empty: it means "this workout has no heart rate data", not
/// "0 seconds in every zone".
pub(super) fn parse_heart_range(raw: Option<&str>) -> Vec<HeartRateZoneBucket> {
    let Some(text) = raw else {
        return Vec::new();
    };
    let mut buckets = Vec::new();
    for (index, part) in text.split(';').enumerate() {
        let mut bits = part.split(',');
        let (Some(seconds), Some(upper)) = (bits.next(), bits.next()) else {
            continue;
        };
        let (Ok(seconds), Ok(upper)) = (seconds.trim().parse::<i64>(), upper.trim().parse::<i32>())
        else {
            continue;
        };
        if seconds < 0 || upper <= 0 {
            continue;
        }
        buckets.push(HeartRateZoneBucket {
            index: index as i32,
            upper_bound_bpm: upper,
            seconds,
        });
    }
    if buckets.iter().all(|bucket| bucket.seconds == 0) {
        return Vec::new();
    }
    buckets
}

pub(super) fn add_milliseconds(base: DateTime<Utc>, offset_ms: i64) -> Option<DateTime<Utc>> {
    Duration::try_milliseconds(offset_ms).and_then(|delta| base.checked_add_signed(delta))
}

pub(super) fn add_minutes(base: DateTime<Utc>, minutes: i64) -> Option<DateTime<Utc>> {
    Duration::try_minutes(minutes).and_then(|delta| base.checked_add_signed(delta))
}

pub(super) fn first_number(object: &Map<String, Value>, names: &[&str]) -> Option<f64> {
    names
        .iter()
        .filter_map(|name| object.get(*name))
        .find_map(parse_number)
}

pub(super) fn first_number_from(
    object: &Map<String, Value>,
    nested: Option<&Map<String, Value>>,
    names: &[&str],
) -> Option<f64> {
    first_number(object, names).or_else(|| nested.and_then(|value| first_number(value, names)))
}

pub(super) fn parse_number(value: &Value) -> Option<f64> {
    let number = match value {
        Value::Number(number) => number.as_f64()?,
        Value::String(text) => text.trim().parse::<f64>().ok()?,
        _ => return None,
    };
    number.is_finite().then_some(number)
}

pub(super) fn parse_timestamp(value: &Value) -> Option<DateTime<Utc>> {
    let number = parse_number(value)?;
    if !number.is_finite() {
        return None;
    }
    // Zepp event payloads sometimes carry the calendar day as a compact
    // integer (`dayId: 20260812`).  Guard against interpreting such values
    // as epoch seconds, which would silently produce dates in 1970.
    // The number itself is the local calendar day. Without a time zone keep UTC midnight; do not invent an offset.
    // Day splits with a `timeZone` go through `summary_date` / `parse_date_with_zone`.
    let compact = number as i64;
    if (19000101..=21001231).contains(&compact) {
        return NaiveDate::parse_from_str(&format!("{compact}"), "%Y%m%d")
            .ok()
            .and_then(|date| date.and_hms_opt(0, 0, 0))
            .map(|naive| DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc));
    }
    if number.abs() >= 10_000_000_000.0 {
        DateTime::from_timestamp_millis(number as i64)
    } else {
        DateTime::from_timestamp(number as i64, 0)
    }
}

pub(super) fn parse_timestamp_or_date(value: &Value) -> Option<DateTime<Utc>> {
    parse_timestamp(value).or_else(|| {
        let date = value.as_str()?;
        let date = NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()?;
        Some(DateTime::<Utc>::from_naive_utc_and_offset(
            date.and_hms_opt(0, 0, 0)?,
            Utc,
        ))
    })
}

pub(super) fn parse_date(value: &Value) -> Option<String> {
    if let Some(text) = value.as_str() {
        if NaiveDate::parse_from_str(text, "%Y-%m-%d").is_ok() {
            return Some(text.to_owned());
        }
        if let Ok(dt) = DateTime::parse_from_rfc3339(text) {
            return Some(dt.date_naive().format("%Y-%m-%d").to_string());
        }
    }
    if let Some(number) = parse_number(value) {
        let compact = number as i64;
        if (19000101..=21001231).contains(&compact) {
            return NaiveDate::parse_from_str(&format!("{compact}"), "%Y%m%d")
                .ok()
                .map(|date| date.format("%Y-%m-%d").to_string());
        }
    }
    parse_timestamp(value).map(|dt| dt.format("%Y-%m-%d").to_string())
}

pub(super) fn parse_date_with_zone(value: &Value, offset_secs: Option<i64>) -> Option<String> {
    // Numeric strings are epochs too; retain the event's timezone when
    // assigning meals (and other daily metrics) to a calendar day.
    if value.as_str().is_some() && parse_number(value).is_none() {
        return parse_date(value);
    }
    if let Some(number) = parse_number(value) {
        let compact = number as i64;
        if (19000101..=21001231).contains(&compact) {
            return parse_date(value);
        }
        let utc = parse_timestamp(value)?;
        let local = match offset_secs {
            Some(secs) => utc.checked_add_signed(Duration::seconds(secs))?,
            None => utc,
        };
        return Some(local.format("%Y-%m-%d").to_string());
    }
    parse_date(value)
}

pub(super) fn summary_date(
    object: &Map<String, Value>,
    nested: Option<&Map<String, Value>>,
) -> Option<String> {
    if let Some(value) = first_value_from(
        object,
        nested,
        &["date", "day", "dayId", "dateString", "localDate"],
    ) {
        return parse_date(value);
    }
    let value = first_value_from(object, nested, &["timestamp", "time", "startTime"])?;
    let zone = first_value_from(object, nested, &["timeZone", "time_zone", "tz"]);
    // When the time zone is an IANA name with DST (Europe/Berlin, America/New_York, ...), the fixed offset table
    // does not have it. Before, it fell back to UTC, and Charge / readiness / daily overview for European and American users landed one day early.
    // Compute the local date with the rules at the instant of the event (the same method as meal day splits in food.rs).
    if let Some(zone) = zone.filter(|zone| timezone_offset_seconds(zone).is_none()) {
        if is_epoch_value(value) {
            if let Some(date) = parse_timestamp(value).and_then(|at| iana_local_date(zone, at)) {
                return Some(date);
            }
        }
    }
    parse_date_with_zone(value, zone.and_then(timezone_offset_seconds))
}

/// Canonical rows are overwritten in insert order: entries with an explicit calendar day must come after the epoch fallback,
/// or the date-only value is always overwritten by the UTC-split timestamp.
pub(super) fn daily_summary_sort_key(item: &Value) -> (u8, i64) {
    let Some(object) = item.as_object() else {
        return (0, 0);
    };
    let nested = object.get("value").and_then(Value::as_object);
    let calendar = ["date", "day", "dayId", "dateString", "localDate"];
    let explicit = first_value_from(object, nested, &calendar).is_some()
        || nested
            .and_then(|value| value.get("samples"))
            .and_then(Value::as_array)
            .is_some_and(|samples| {
                samples.iter().any(|sample| {
                    sample
                        .as_object()
                        .is_some_and(|object| first_value(object, &calendar).is_some())
                })
            });
    let timestamp = first_number_from(object, nested, &["timestamp", "time", "startTime"])
        .map(|value| value.round() as i64)
        .unwrap_or(0);
    (u8::from(explicit), timestamp)
}

/// Whether this is a timestamp (not a date string, and not a compact date such as 20260930).
fn is_epoch_value(value: &Value) -> bool {
    match parse_number(value) {
        Some(number) => !(19000101..=21001231).contains(&(number as i64)),
        None => false,
    }
}

/// The local date at `instant` for an IANA name in a time zone field (it can be written as `"1,Europe/Berlin"`).
pub(super) fn iana_local_date(zone: &Value, instant: DateTime<Utc>) -> Option<String> {
    let name = zone.as_str()?.rsplit(',').next()?.trim();
    if name.is_empty() {
        return None;
    }
    let zone = jiff::tz::TimeZone::get(name).ok()?;
    let timestamp = jiff::Timestamp::from_second(instant.timestamp()).ok()?;
    Some(timestamp.to_zoned(zone).date().to_string())
}

pub(super) fn timezone_offset_seconds(value: &Value) -> Option<i64> {
    match value {
        Value::Number(_) => parse_number(value).and_then(offset_from_number),
        Value::String(text) => parse_timezone_text(text),
        _ => None,
    }
}

pub(super) fn offset_from_number(value: f64) -> Option<i64> {
    if !value.is_finite() {
        return None;
    }
    let raw = value.round() as i64;
    let seconds = if raw.abs() > 18 * 3600 {
        if raw % 1000 != 0 {
            return None;
        }
        raw / 1000
    } else {
        raw
    };
    // Real time zone offsets are multiples of 15 minutes. A device time zone index such as "32" must not be read as 32 seconds.
    if seconds % 900 != 0 {
        return None;
    }
    (-18 * 3600..=18 * 3600)
        .contains(&seconds)
        .then_some(seconds)
}

pub(super) fn parse_timezone_text(text: &str) -> Option<i64> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    if let Some((left, right)) = text.split_once(',') {
        return parse_timezone_text(left).or_else(|| parse_timezone_text(right));
    }
    if let Ok(number) = text.parse::<f64>() {
        return offset_from_number(number);
    }
    let clock = text
        .strip_prefix("GMT")
        .or_else(|| text.strip_prefix("Utc"))
        .or_else(|| text.strip_prefix("UTC"))
        .or_else(|| text.strip_prefix("gmt"))
        .unwrap_or(text);
    parse_offset_clock(clock).or_else(|| iana_fixed_offset(text))
}

pub(super) fn parse_offset_clock(text: &str) -> Option<i64> {
    let text = text.trim();
    let (sign, rest) = if let Some(rest) = text.strip_prefix('+') {
        (1_i64, rest)
    } else {
        let rest = text.strip_prefix('-')?;
        (-1, rest)
    };
    let (hours, minutes) = if let Some((hours, minutes)) = rest.split_once(':') {
        (hours.parse::<i64>().ok()?, minutes.parse::<i64>().ok()?)
    } else if rest.len() == 4 && rest.chars().all(|ch| ch.is_ascii_digit()) {
        (
            rest[..2].parse::<i64>().ok()?,
            rest[2..].parse::<i64>().ok()?,
        )
    } else {
        (rest.parse::<i64>().ok()?, 0)
    };
    if !(0..=18).contains(&hours) || !(0..60).contains(&minutes) {
        return None;
    }
    let seconds = sign * (hours * 3600 + minutes * 60);
    (seconds % 900 == 0).then_some(seconds)
}

pub(super) fn iana_fixed_offset(name: &str) -> Option<i64> {
    // Accept only zones with no DST and a fixed offset all year. For a name with DST, do not apply it and do not invent an offset.
    match name {
        "Asia/Shanghai" | "Asia/Hong_Kong" | "Asia/Taipei" | "Asia/Chongqing" | "Asia/Harbin"
        | "PRC" | "Hongkong" => Some(8 * 3600),
        "Asia/Tokyo" | "Asia/Seoul" | "Japan" => Some(9 * 3600),
        "UTC" | "Etc/UTC" | "Etc/GMT" | "GMT" | "Zulu" => Some(0),
        _ => None,
    }
}

pub(super) fn timestamp_not_unreasonably_future(timestamp: DateTime<Utc>) -> bool {
    if timestamp.year() > 2100 {
        return false;
    }
    match Utc::now().checked_add_signed(Duration::days(1)) {
        Some(limit) => timestamp <= limit,
        None => true,
    }
}
