//! Daily event payloads are stored per UTC day.
//!
//! Charge, readiness and DailyHealth on `/v2/users/me/events` have one entry per day (readiness
//! can have two per day), and sync pulls the last 30 days every 15 minutes. Before, the whole window was stored as **one**
//! raw payload, with the millisecond window start/end in the key. The key differed on every sync, so each sync added
//! a new row: one Charge row is 4.7 MB of JSON (~860 KB compressed), a full day open grows the database by 80 MB,
//! and in a real two-month database it used 525 MB, of which 96% had no derived row pointing to it.
//!
//! Now it is split by the entry's UTC day, one row per day, with a stable key: `events:<type>:<sub>:day:<date>`.
//! Pulling the same day again overwrites. It does not add.
//!
//! **Why splitting does not change the result**: normalization is per entry, and the final dedup by `(date, metric, source, device)`
//! is "last write wins". After the day split, writing in order still lets the latest entry of the same day win. On a real database,
//! all 2161 historical window payloads were compared one by one: normalizing the whole window and writing the day split in order gave
//! **exactly the same** daily metrics.
//!
//! **Why the request window aligns to whole days**: for two payloads of the same day, the new one must be a superset of the old one, so the overwrite
//! loses nothing. The window start moves forward every 15 minutes. Without alignment, the first 15 minutes of the earliest day would
//! disappear from the new payload, and the overwrite would also delete that day's derived rows. After aligning to UTC midnight, each day is either
//! overwritten as a whole day, or (today) loaded from midnight up to now. The later pull always contains the earlier one.

use chrono::{DateTime, Duration, NaiveDate, Utc};
use serde_json::{Map, Value};

const DAY_MS: i64 = 86_400_000;

/// Expands a half-open range `[start, end)` to whole UTC days: `from` is midnight of the start day, and `to` is
/// the last millisecond of the end day, but not past `now`.
///
/// `to` uses "last millisecond" and not next-day midnight: an entry at exactly midnight belongs to the next day, and including it would
/// add a "next day" with only one entry for no reason.
pub fn utc_day_aligned_millis(
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    now: DateTime<Utc>,
) -> (i64, i64) {
    let from = start.timestamp_millis().div_euclid(DAY_MS) * DAY_MS;
    let end_ms = end.timestamp_millis();
    let next_midnight = (end_ms + DAY_MS - 1).div_euclid(DAY_MS) * DAY_MS;
    let to = (next_midnight - 1).min(now.timestamp_millis()).max(from);
    (from, to)
}

/// The last millisecond of the UTC day that contains `millis`. The whole-window key form uses it as the end: the request itself loads up to
/// now, but the key stays the same within a day, so a re-pull overwrites and does not add.
pub fn end_of_utc_day_millis(millis: i64) -> i64 {
    (millis.div_euclid(DAY_MS) + 1) * DAY_MS - 1
}

/// The stable key for one day. The date is the UTC day, same as the split rule.
pub fn day_source_key(event_type: &str, sub_type: &str, day: NaiveDate) -> String {
    format!(
        "events:{event_type}:{sub_type}:day:{}",
        day.format("%Y-%m-%d")
    )
}

/// Old window-form key `events:<type>:<sub>:<from_ms>:<to_ms>` -> `events:<type>:<sub>`.
/// Returns `None` for other forms (including the new per-day key).
pub fn window_key_family(source_key: &str) -> Option<&str> {
    let mut parts = source_key.rsplitn(3, ':');
    let to = parts.next()?;
    let from = parts.next()?;
    let family = parts.next()?;
    let numeric = |value: &str| !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit());
    if !numeric(from) || !numeric(to) {
        return None;
    }
    let mut segments = family.split(':');
    let valid = segments.next() == Some("events")
        && segments.next().is_some_and(|value| !value.is_empty())
        && segments.next().is_some_and(|value| !value.is_empty())
        && segments.next().is_none();
    valid.then_some(family)
}

/// Result of the day split.
#[derive(Debug, Clone, PartialEq)]
pub struct EventDays {
    /// One payload per UTC day, ascending by date. Each has the same shape as the original response (top-level fields other than `items`
    /// are kept as is). Only `items` holds just this day's entries, in the original order.
    pub days: Vec<(NaiveDate, Value)>,
    /// Entries with no readable timestamp. They cannot go to a day, so they are stored with the whole window as before.
    pub residual: Option<Value>,
}

/// Splits by each entry's `timestamp` (milliseconds, number or numeric string) into one payload per UTC day.
///
/// Returns `None` when the response is not `{ "items": [...] }` or `items` is empty. The caller then stores
/// the whole response as one row as before, because "this window is empty" is also a fact to keep.
pub fn split_by_utc_day(payload: &Value) -> Option<EventDays> {
    let object = payload.as_object()?;
    let items = object.get("items")?.as_array()?;
    if items.is_empty() {
        return None;
    }
    let mut days: std::collections::BTreeMap<NaiveDate, Vec<Value>> = Default::default();
    let mut residual = Vec::new();
    for item in items {
        match item_day(item) {
            Some(day) => days.entry(day).or_default().push(item.clone()),
            None => residual.push(item.clone()),
        }
    }
    let wrap = |items: Vec<Value>| {
        let mut out = Map::with_capacity(object.len());
        for (key, value) in object {
            if key != "items" {
                out.insert(key.clone(), value.clone());
            }
        }
        out.insert("items".into(), Value::Array(items));
        Value::Object(out)
    };
    Some(EventDays {
        days: days
            .into_iter()
            .map(|(day, items)| (day, wrap(items)))
            .collect(),
        residual: (!residual.is_empty()).then(|| wrap(residual)),
    })
}

/// One day as `[midnight, next midnight)`.
pub fn day_bounds(day: NaiveDate) -> (DateTime<Utc>, DateTime<Utc>) {
    let start = day.and_hms_opt(0, 0, 0).unwrap_or_default().and_utc();
    (start, start + Duration::days(1))
}

fn item_day(item: &Value) -> Option<NaiveDate> {
    let raw = item.get("timestamp")?;
    let millis = raw
        .as_i64()
        .or_else(|| raw.as_f64().filter(|v| v.is_finite()).map(|v| v as i64))
        .or_else(|| {
            raw.as_str()
                .and_then(|text| text.trim().parse::<i64>().ok())
        })?;
    DateTime::<Utc>::from_timestamp_millis(millis).map(|at| at.date_naive())
}

#[cfg(test)]
mod tests;
