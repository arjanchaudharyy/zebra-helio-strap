//! Fetching from the official data API: chunks of at most 90 days, newest first, silent-truncation checks, split into raw payloads per day.
//!
//! Three tested facts set the shape of this code (tested on the user's own account):
//!
//! - **Long ranges are silently truncated**: the server cuts endDate to about 500 days after startDate, still returns 200,
//!   gives no paging hint, and drops the **newest** part. So each chunk is at most [`MAX_CHUNK_DAYS`], and after a chunk
//!   returns we check the last day: if it is short of the requested end, we ask again from the gap ([`needs_tail`]).
//! - Both date parameters are inclusive (`startDate=09-22&endDate=09-29&interval=daily` returns 8 days).
//! - Payloads are stored per day / per record: the key is `official:<kind>:<that day or record>`. With keys per request window,
//!   the window moves one day each day, so the key changes daily and the same day is stored again and again.
//!
//! The raw payload also carries the time zone of the request: hourly steps only give the hour, and converting that to an absolute time
//! needs the time zone. When the payload states it, a replay does not have to guess.

use super::OfficialClient;
use crate::models::{error::Result, CapabilityStatus, RawRecord, SourceScope, ZeppBridgeError};
use chrono::{DateTime, Datelike, Duration, NaiveDate, Utc};
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// The maximum days in one chunk. The official truncation starts at about 500 days; 90 days is far from it and keeps one response small.
pub const MAX_CHUNK_DAYS: i64 = 90;
/// Heart rate is per minute, 1440 records a day: one week per chunk.
pub const HEART_RATE_CHUNK_DAYS: i64 = 7;
/// If the last day of a chunk is this many days or more before the requested end, suspect truncation and ask again from the gap.
/// With truly no data (watch not worn, not synced), the extra request returns empty and the loop stops.
pub const TRUNCATION_GAP_DAYS: i64 = 7;
/// The maximum follow-up requests per chunk, so a change in server behavior cannot cause an endless loop.
const MAX_TAIL_REQUESTS: usize = 3;

/// The prefix of `raw_records.source_key`: it marks an official payload (`storage/official.rs`).
pub const SOURCE_PREFIX: &str = "official:";
pub const PROVIDER: &str = "official";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OfficialKind {
    Sleep,
    HeartRate,
    ActivityDaily,
    ActivityHourly,
    Sports,
    SportDetail,
    Pai,
    Body,
}

impl OfficialKind {
    /// The sync stream it goes into. It keeps the legacy stream names: progress, data health and replay use stream names.
    /// Official payloads differ by the `source_key` prefix and the `provider` column, not by a separate set of streams.
    pub fn stream(self) -> &'static str {
        match self {
            Self::Sleep => "sleep",
            Self::HeartRate => "heart_rate",
            Self::ActivityDaily | Self::ActivityHourly => "daily_summary",
            Self::Sports => "workouts",
            Self::SportDetail => "workout_detail",
            Self::Pai => "wellness",
            Self::Body => "weight",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Sleep => "sleep",
            Self::HeartRate => "heart_rate",
            Self::ActivityDaily => "activity_daily",
            Self::ActivityHourly => "activity_hourly",
            Self::Sports => "sports",
            Self::SportDetail => "sport_detail",
            Self::Pai => "pai",
            Self::Body => "body",
        }
    }

    pub fn from_label(label: &str) -> Option<Self> {
        [
            Self::Sleep,
            Self::HeartRate,
            Self::ActivityDaily,
            Self::ActivityHourly,
            Self::Sports,
            Self::SportDetail,
            Self::Pai,
            Self::Body,
        ]
        .into_iter()
        .find(|kind| kind.label() == label)
    }

    /// Reads the kind from `official:<label>:...`.
    pub fn from_source_key(source_key: &str) -> Option<Self> {
        let rest = source_key.strip_prefix(SOURCE_PREFIX)?;
        Self::from_label(rest.split(':').next()?)
    }

    pub fn chunk_days(self) -> i64 {
        match self {
            Self::HeartRate => HEART_RATE_CHUNK_DAYS,
            _ => MAX_CHUNK_DAYS,
        }
    }

    fn path(self) -> &'static str {
        match self {
            Self::Sleep => "/users/-/sleep",
            Self::HeartRate => "/users/-/heartrates",
            Self::ActivityDaily | Self::ActivityHourly => "/users/-/activities",
            Self::Sports => "/users/-/sports",
            Self::SportDetail => "/users/-/sportDetail",
            Self::Pai => "/users/-/openData/PaiSummary",
            Self::Body => "/users/-/body",
        }
    }

    fn extra_query(self, time_zone: &str) -> Vec<(&'static str, String)> {
        match self {
            Self::Sleep | Self::ActivityDaily => vec![("interval", "daily".into())],
            Self::ActivityHourly => vec![("interval", "hourly".into())],
            Self::HeartRate => vec![("timezone", time_zone.into()), ("type", "ALL".into())],
            Self::Sports | Self::Body => vec![("timezone", time_zone.into())],
            Self::SportDetail | Self::Pai => Vec::new(),
        }
    }
}

/// Splits `[start, end]` (both inclusive) into chunks of at most `days` days, newest chunk first.
pub fn date_chunks_newest_first(
    start: NaiveDate,
    end: NaiveDate,
    days: i64,
) -> Vec<(NaiveDate, NaiveDate)> {
    let days = days.max(1);
    let mut chunks = Vec::new();
    let mut chunk_end = end;
    while chunk_end >= start {
        let chunk_start = (chunk_end - Duration::days(days - 1)).max(start);
        chunks.push((chunk_start, chunk_end));
        let Some(next) = chunk_start.pred_opt() else {
            break;
        };
        chunk_end = next;
    }
    chunks
}

/// The last day of a chunk is too far from the end: ask again from the day after the last day.
///
/// An empty result (`last = None`) does not continue: that is truly no data, not truncation.
pub fn needs_tail(requested_end: NaiveDate, last: Option<NaiveDate>) -> Option<NaiveDate> {
    let last = last?;
    ((requested_end - last).num_days() > TRUNCATION_GAP_DAYS)
        .then(|| last.succ_opt())
        .flatten()
}

fn day_prefix(text: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(text.get(..10)?, "%Y-%m-%d").ok()
}

/// The day of a moment in a time zone.
fn local_date(timestamp: i64, time_zone: &str) -> Option<NaiveDate> {
    let zone = jiff::tz::TimeZone::get(time_zone).ok()?;
    let instant = jiff::Timestamp::from_second(timestamp).ok()?;
    NaiveDate::parse_from_str(&instant.to_zoned(zone).date().to_string(), "%Y-%m-%d").ok()
}

/// The IANA time zone name of this computer; UTC when unknown. Used when only the official channel is connected and no watch time zone is known.
pub fn system_time_zone() -> String {
    jiff::tz::TimeZone::system()
        .iana_name()
        .map(str::to_string)
        .unwrap_or_else(|| "UTC".to_string())
}

/// Today in a time zone. Today in UTC when the time zone is unknown.
pub fn local_today(time_zone: &str) -> NaiveDate {
    local_date(Utc::now().timestamp(), time_zone).unwrap_or_else(|| Utc::now().date_naive())
}
/// The UTC time of midnight of a day in a time zone. Falls back to UTC midnight when unknown (only used to label payload start and end).
pub fn local_midnight_utc(date: NaiveDate, time_zone: &str) -> DateTime<Utc> {
    let fallback =
        || DateTime::from_naive_utc_and_offset(date.and_hms_opt(0, 0, 0).unwrap_or_default(), Utc);
    let Ok(zone) = jiff::tz::TimeZone::get(time_zone) else {
        return fallback();
    };
    let Ok(civil) =
        jiff::civil::Date::new(date.year() as i16, date.month() as i8, date.day() as i8)
    else {
        return fallback();
    };
    civil
        .to_zoned(zone)
        .ok()
        .and_then(|zoned| DateTime::from_timestamp(zoned.timestamp().as_second(), 0))
        .unwrap_or_else(fallback)
}

fn number(item: &Value, key: &str) -> Option<i64> {
    match item.get(key)? {
        Value::Number(number) => number
            .as_i64()
            .or_else(|| number.as_f64().map(|value| value as i64)),
        Value::String(text) => text.trim().parse().ok(),
        _ => None,
    }
}

fn text(item: &Value, key: &str) -> Option<String> {
    match item.get(key)? {
        Value::String(text) if !text.is_empty() => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

/// The time of the newest real reading in official per-minute heart rate. Minutes without the watch are 0; like the normalizer, only 25-250 counts.
pub fn newest_heart_rate_at(items: &[Value]) -> Option<DateTime<Utc>> {
    items
        .iter()
        .filter(|item| {
            number(item, "heartRateData").is_some_and(|value| (25..=250).contains(&value))
        })
        .filter_map(|item| number(item, "timestamp"))
        .max()
        .and_then(|seconds| DateTime::from_timestamp(seconds, 0))
}

/// The day a record belongs to.
pub fn item_day(kind: OfficialKind, item: &Value, time_zone: &str) -> Option<NaiveDate> {
    match kind {
        OfficialKind::Pai => item
            .get("calendarDay")
            .and_then(Value::as_str)
            .and_then(day_prefix),
        OfficialKind::Sports => local_date(number(item, "startTime")?, time_zone),
        OfficialKind::Body => local_date(number(item, "timestamp")?, time_zone),
        OfficialKind::SportDetail => local_date(number(item, "startTime")?, time_zone),
        _ => item
            .get("date")
            .and_then(Value::as_str)
            .and_then(day_prefix),
    }
}

/// Splits a set of records into raw payloads per day (workouts per record, weight per measurement).
pub fn split_records(kind: OfficialKind, items: Vec<Value>, time_zone: &str) -> Vec<RawRecord> {
    split_records_counted(kind, items, time_zone).0
}

/// Same as [`split_records`], and also counts the records dropped for a missing date or ID. They must not
/// disappear silently; they go into the completeness result of this chunk.
pub fn split_records_counted(
    kind: OfficialKind,
    items: Vec<Value>,
    time_zone: &str,
) -> (Vec<RawRecord>, usize) {
    let mut groups: BTreeMap<String, (NaiveDate, Vec<Value>)> = BTreeMap::new();
    let mut skipped = 0usize;
    for item in items {
        let Some(day) = item_day(kind, &item, time_zone) else {
            skipped += 1;
            continue;
        };
        let key = match kind {
            OfficialKind::Sports => match text(&item, "trackId") {
                Some(track) => track,
                None => {
                    skipped += 1;
                    continue;
                }
            },
            OfficialKind::Body => match number(&item, "timestamp") {
                Some(timestamp) => timestamp.to_string(),
                None => {
                    skipped += 1;
                    continue;
                }
            },
            _ => format!("day:{day}"),
        };
        groups
            .entry(key)
            .or_insert_with(|| (day, Vec::new()))
            .1
            .push(item);
    }
    let records = groups
        .into_iter()
        .map(|(key, (day, items))| {
            raw_record(kind, &key, day, json!({ "items": items }), time_zone)
        })
        .collect();
    (records, skipped)
}

/// One official raw payload. `payload` gets an added `timeZone` (see the file header).
pub fn raw_record(
    kind: OfficialKind,
    key: &str,
    day: NaiveDate,
    mut payload: Value,
    time_zone: &str,
) -> RawRecord {
    if let Value::Object(map) = &mut payload {
        map.insert("timeZone".into(), Value::String(time_zone.into()));
    }
    let start_utc = local_midnight_utc(day, time_zone);
    RawRecord {
        stream: kind.stream().into(),
        source_key: format!("{SOURCE_PREFIX}{}:{key}", kind.label()),
        source_scope: SourceScope::UserFused,
        device_id: None,
        start_utc,
        end_utc: Some(start_utc + Duration::days(1)),
        payload,
        capability: CapabilityStatus::Verified,
    }
}

/// The official list API has only one success shape: `{"items": [...]}` (tested; empty is
/// `items: []`). Any other shape is never "no data":
/// - A nonzero `code` is a business error (HTTP 200 can also return it);
/// - No `items`, `items` null or not an array, or a top level that is not an object: an unknown response.
fn items_of(body: Value) -> Result<Vec<Value>> {
    let Value::Object(mut map) = body else {
        return Err(ZeppBridgeError::ParseError(
            "The official response is not an object, unknown shape".into(),
        ));
    };
    if let Some(code) = map
        .get("code")
        .and_then(Value::as_i64)
        .filter(|code| *code != 0)
    {
        let message = map
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("(the cloud gave no explanation)")
            .to_string();
        return Err(ZeppBridgeError::CloudRejected { code, message });
    }
    match map.remove("items") {
        Some(Value::Array(items)) => Ok(items),
        Some(_) => Err(ZeppBridgeError::ParseError(
            "The official response items is not an array".into(),
        )),
        None => Err(ZeppBridgeError::ParseError(
            "The official response has no items, unknown shape".into(),
        )),
    }
}

/// The result of one chunk. `gap` says what part of the chunk is missing (truncation follow-ups hit the limit, a follow-up made no
/// progress, records were dropped for a missing date or ID). The records already fetched are stored as usual.
#[derive(Debug)]
pub struct ChunkOutcome {
    pub records: Vec<RawRecord>,
    pub gap: Option<String>,
}

pub struct OfficialFetcher<'a> {
    pub client: &'a OfficialClient,
    pub access_token: &'a str,
    /// The IANA time zone name: the heart rate, workout and weight APIs need it, and hourly steps use it to get absolute times.
    pub time_zone: String,
}

impl OfficialFetcher<'_> {
    /// Fetches one chunk (both ends inclusive) and splits it into daily raw payloads. On signs of truncation, it continues from the gap.
    /// If the limit is reached and it is still truncated, or a follow-up makes no progress, it returns what it has and writes the gap into `gap`.
    pub async fn fetch_chunk(
        &self,
        kind: OfficialKind,
        start: NaiveDate,
        end: NaiveDate,
    ) -> Result<ChunkOutcome> {
        let mut items = Vec::new();
        let mut cursor = start;
        let mut gap = None;
        for attempt in 0..=MAX_TAIL_REQUESTS {
            let mut query = vec![
                ("startDate", cursor.format("%Y-%m-%d").to_string()),
                ("endDate", end.format("%Y-%m-%d").to_string()),
            ];
            query.extend(kind.extra_query(&self.time_zone));
            let batch = items_of(
                self.client
                    .get_json(self.access_token, kind.path(), &query)
                    .await?,
            )?;
            let last = batch
                .iter()
                .filter_map(|item| item_day(kind, item, &self.time_zone))
                .max();
            items.extend(batch);
            match needs_tail(end, last) {
                Some(next) if next > cursor && next <= end => {
                    if attempt == MAX_TAIL_REQUESTS {
                        gap = Some(format!(
                            "official {} still truncated before {next} after {MAX_TAIL_REQUESTS} follow-ups; {next} to {end} not fetched",
                            kind.label()
                        ));
                        break;
                    }
                    tracing::info!(
                        "official {} possibly truncated after {:?}, continuing from {next}",
                        kind.label(),
                        last
                    );
                    cursor = next;
                }
                Some(next) if next <= cursor => {
                    gap = Some(format!(
                        "official {} follow-up from {cursor} returned no new records; {cursor} to {end} not fetched",
                        kind.label()
                    ));
                    break;
                }
                _ => break,
            }
        }
        let (records, skipped) = split_records_counted(kind, items, &self.time_zone);
        if skipped > 0 {
            let note = format!(
                "{skipped} official {} records have no date or ID and were not stored",
                kind.label()
            );
            gap = Some(match gap {
                Some(previous) => format!("{previous}; {note}"),
                None => note,
            });
        }
        Ok(ChunkOutcome { records, gap })
    }

    /// Probes the cloud before a sync (`sync/probe.rs`): the newest moment in official heart rate for this day. Stores nothing.
    pub async fn newest_heart_rate(&self, day: NaiveDate) -> Result<Option<DateTime<Utc>>> {
        let kind = OfficialKind::HeartRate;
        let date = day.format("%Y-%m-%d").to_string();
        let mut query = vec![("startDate", date.clone()), ("endDate", date)];
        query.extend(kind.extra_query(&self.time_zone));
        let items = items_of(
            self.client
                .get_json(self.access_token, kind.path(), &query)
                .await?,
        )?;
        Ok(newest_heart_rate_at(&items))
    }

    /// The details of one workout (per-second series, track, pauses).
    pub async fn fetch_sport_detail(&self, track_id: &str, day: NaiveDate) -> Result<RawRecord> {
        let body = self
            .client
            .get_json(
                self.access_token,
                OfficialKind::SportDetail.path(),
                &[("trackId", track_id.to_string())],
            )
            .await?;
        if !body.is_object() {
            return Err(ZeppBridgeError::ParseError("The official workout details are not an object".into()));
        }
        Ok(raw_record(
            OfficialKind::SportDetail,
            track_id,
            day,
            body,
            &self.time_zone,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(text: &str) -> NaiveDate {
        NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn chunks_cover_the_range_newest_first_without_gaps_or_overlap() {
        let chunks = date_chunks_newest_first(day("2026-01-01"), day("2026-09-29"), 90);
        assert_eq!(chunks.first().unwrap().1, day("2026-09-29"));
        assert_eq!(chunks.last().unwrap().0, day("2026-01-01"));
        for pair in chunks.windows(2) {
            assert_eq!(pair[1].1.succ_opt().unwrap(), pair[0].0);
        }
        assert!(chunks
            .iter()
            .all(|(start, end)| (*end - *start).num_days() < 90));
    }

    #[test]
    fn a_short_tail_is_not_truncation_but_a_long_one_is() {
        let end = day("2026-09-29");
        assert_eq!(needs_tail(end, Some(day("2026-09-28"))), None);
        assert_eq!(needs_tail(end, None), None);
        assert_eq!(
            needs_tail(end, Some(day("2026-07-01"))),
            Some(day("2026-07-02"))
        );
    }

    #[test]
    fn records_split_per_day_with_stable_keys_and_the_request_zone() {
        let items = vec![
            json!({"date": "2026-09-28", "hour": 11, "steps": 208}),
            json!({"date": "2026-09-28", "hour": 12, "steps": 20}),
            json!({"date": "2026-09-29", "hour": 8, "steps": 5}),
        ];
        let records = split_records(OfficialKind::ActivityHourly, items, "Asia/Shanghai");
        assert_eq!(records.len(), 2);
        assert_eq!(
            records[0].source_key,
            "official:activity_hourly:day:2026-09-28"
        );
        assert_eq!(records[0].stream, "daily_summary");
        assert_eq!(records[0].payload["items"].as_array().unwrap().len(), 2);
        assert_eq!(records[0].payload["timeZone"], "Asia/Shanghai");
        // Shanghai midnight = 16:00Z on the day before.
        assert_eq!(
            records[0].start_utc.to_rfc3339(),
            "2026-09-27T16:00:00+00:00"
        );
        assert_eq!(
            OfficialKind::from_source_key(&records[0].source_key),
            Some(OfficialKind::ActivityHourly)
        );
    }

    #[test]
    fn workouts_split_per_track_and_skip_items_without_an_id() {
        let items = vec![
            json!({"trackId": "1789908976", "startTime": 1789908976}),
            json!({"startTime": 1789908976}),
        ];
        let records = split_records(OfficialKind::Sports, items, "Asia/Shanghai");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].source_key, "official:sports:1789908976");
    }
}
