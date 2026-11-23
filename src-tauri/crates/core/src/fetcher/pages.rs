//! Paged and sliced fetch: heart rate cursor paging, daily summary and sleep by slice, failure merging (split out of fetcher/mod.rs, logic unchanged).

use super::*;

/// Keep each response intact: pagination metadata and unknown fields belong to raw.
///
/// `window` is the full UTC day `day` (see `FetchWindow::utc_days`). The key is
/// `heart_rate:day:<date>:<page>`, so the same page of the same day is always the same payload.
pub(super) async fn fetch_heart_rate_pages_with<F, Fut>(
    day: NaiveDate,
    window: FetchWindow,
    mut fetch: F,
) -> Result<Vec<FetchedRecord>>
where
    F: FnMut(i64, i64) -> Fut,
    Fut: std::future::Future<Output = Result<Value>>,
{
    let end = window.end_utc.timestamp();
    let mut cursor = window.start_utc.timestamp();
    let mut records = Vec::new();
    let mut page = 0usize;
    loop {
        let payload = fetch(cursor, end).await?;
        let items = heart_rate_items(&payload);
        let next = if items.len() >= HEART_RATE_PAGE_LIMIT as usize {
            heart_rate_cursor(&items).filter(|next| *next > cursor && *next < end)
        } else {
            None
        };
        records.push(FetchedRecord::from_raw(RawRecord {
            stream: "heart_rate".into(),
            source_key: format!("heart_rate:day:{day}:{page}"),
            source_scope: SourceScope::UserFused,
            device_id: None,
            start_utc: DateTime::from_timestamp(cursor, 0).unwrap_or(window.start_utc),
            end_utc: Some(window.end_utc),
            payload,
            capability: CapabilityStatus::Verified,
        }));
        let Some(next) = next else { break };
        cursor = next;
        page += 1;
    }
    Ok(records)
}

/// Merge the results of a sliced fetch into one window result.
///
/// `Cancelled` / `NeedsReauth` must return at once even when some slices succeeded. They must not be
/// hidden by "we have some records". When a sub-slice returns 404: with data, mark it `incomplete` so backfill retries;
/// with no records at all, return the original unavailable error.
pub(super) fn conclude_slices(
    mut records: Vec<FetchedRecord>,
    last_error: Option<ZeppBridgeError>,
    empty_message: &str,
) -> Result<Vec<FetchedRecord>> {
    if let Some(error) = last_error {
        if error.is_cancelled() || error.needs_reauth() {
            return Err(error);
        }
        if records.is_empty() {
            return Err(error);
        }
        for record in &mut records {
            record.incomplete = true;
            record.incomplete_reason = Some(error.to_string());
        }
        return Ok(records);
    }
    if records.is_empty() {
        return Err(ZeppBridgeError::Unavailable(empty_message.into()));
    }
    Ok(records)
}

pub(super) fn is_abort_error(error: &ZeppBridgeError) -> bool {
    error.is_cancelled() || error.needs_reauth()
}

/// A year of daily events can exceed the response limit or contain one bad
/// historical slice. Keep every successful response and leave failed slices
/// marked incomplete so backfill can retry them.
pub(super) async fn fetch_daily_statistics_slice_with<F, Fut>(
    window: FetchWindow,
    mut fetch: F,
) -> Result<Vec<FetchedRecord>>
where
    F: FnMut(&'static str, &'static str) -> Fut,
    Fut: std::future::Future<Output = Result<Value>>,
{
    let mut records = Vec::new();
    let mut last_error = None;
    let mut failures = Vec::new();
    let (from, to) =
        crate::event_days::utc_day_aligned_millis(window.start_utc, window.end_utc, Utc::now());
    for (event_type, sub_type) in [
        ("DailyHealth", "summary"),
        ("Charge", "real_data"),
        ("readiness", "watch_score"),
        ("WatchSportStatistics", "SPORT_LOAD"),
        ("WatchSportStatistics", "VO2_MAX"),
    ] {
        match fetch(event_type, sub_type).await {
            Ok(payload) if event_type == "WatchSportStatistics" => {
                records.push(FetchedRecord::from_raw(RawRecord {
                    stream: "daily_summary".into(),
                    source_key: format!(
                        "WatchSportStatistics:{sub_type}:{}:{}",
                        window.start_day(),
                        window.end_day()
                    ),
                    source_scope: SourceScope::UserFused,
                    device_id: None,
                    start_utc: window.start_utc,
                    end_utc: Some(window.end_utc),
                    payload,
                    capability: CapabilityStatus::Verified,
                }))
            }
            Ok(payload) => {
                records.extend(event_day_records(event_type, sub_type, from, to, payload))
            }
            Err(error) if is_abort_error(&error) => return Err(error),
            Err(error) if event_type != "DailyHealth" && error.is_unavailable() => {}
            Err(error) => {
                failures.push(format!(
                    "{event_type}/{sub_type} ({}..{}): {}",
                    window.start_day(),
                    window.end_day(),
                    error.user_message()
                ));
                last_error = Some(error);
            }
        }
    }
    let mut records = conclude_slices(records, last_error, "daily summary window has no recognizable records")?;
    if !failures.is_empty() {
        let reason = failures.join("; ");
        for record in &mut records {
            record.incomplete_reason = Some(reason.clone());
        }
    }
    Ok(records)
}

pub(super) async fn fetch_daily_summary_slices_with<F, Fut>(
    window: FetchWindow,
    mut fetch: F,
) -> Result<Vec<FetchedRecord>>
where
    F: FnMut(FetchWindow) -> Fut,
    Fut: std::future::Future<Output = Result<Vec<FetchedRecord>>>,
{
    let mut records = Vec::new();
    let mut last_error = None;
    let mut failures = Vec::new();
    for chunk in window.chunks(30) {
        match fetch(chunk).await {
            Ok(slice) => records.extend(slice),
            Err(error) if is_abort_error(&error) => return Err(error),
            Err(error) => {
                failures.push(format!(
                    "daily_summary ({}..{}): {}",
                    chunk.start_day(),
                    chunk.end_day(),
                    error.user_message()
                ));
                if !error.is_unavailable() || last_error.is_none() {
                    last_error = Some(error);
                }
            }
        }
    }
    let mut records = conclude_slices(records, last_error, "daily summary window has no recognizable records")?;
    if !failures.is_empty() {
        let reason = failures.join("; ");
        for record in &mut records {
            let reason = match record.incomplete_reason.take() {
                Some(existing) => format!("{existing}; {reason}"),
                None => reason.clone(),
            };
            record.incomplete_reason = Some(reason);
        }
    }
    Ok(records)
}

/// A failed historical slice must not discard other nights or prevent the
/// latest slice from being requested. The connector already bounds retries.
pub(super) async fn fetch_sleep_slices_with<F, Fut>(
    window: FetchWindow,
    mut fetch: F,
) -> Result<Vec<FetchedRecord>>
where
    F: FnMut(FetchWindow) -> Fut,
    Fut: std::future::Future<Output = Result<FetchedRecord>>,
{
    let mut records = Vec::new();
    let mut last_error = None;
    for chunk in window.chunks(7) {
        match fetch(chunk).await {
            Ok(record) => records.push(record),
            Err(error) if is_abort_error(&error) => return Err(error),
            Err(error) => {
                // Do not downgrade a request failure to unavailable when a
                // later slice happens to return 404.
                if !error.is_unavailable() || last_error.is_none() {
                    last_error = Some(error);
                }
            }
        }
    }
    conclude_slices(records, last_error, "sleep window has no recognizable records")
}

/// The heartRate endpoint's per-request sample cap.
pub(super) const HEART_RATE_PAGE_LIMIT: i64 = 1000;

/// Collect the sample items out of a heartRate payload, mirroring the
/// normalizer's accepted shapes. Pure so pagination logic is unit-testable.
pub(super) fn heart_rate_items(payload: &Value) -> Vec<Value> {
    if let Some(array) = payload.as_array() {
        return array.to_vec();
    }
    let Some(object) = payload.as_object() else {
        return Vec::new();
    };
    for key in ["items", "records", "results", "list"] {
        if let Some(array) = object.get(key).and_then(Value::as_array) {
            return array.to_vec();
        }
    }
    if let Some(data) = object.get("data") {
        if let Some(array) = data.as_array() {
            return array.to_vec();
        }
        if let Some(data_object) = data.as_object() {
            for key in ["items", "records", "results", "list"] {
                if let Some(array) = data_object.get(key).and_then(Value::as_array) {
                    return array.to_vec();
                }
            }
        }
    }
    Vec::new()
}

/// Compute the next pagination cursor from merged heart-rate items: the max
/// sample timestamp plus one second. Timestamps may be epoch seconds or
/// milliseconds; malformed items are skipped.
pub(super) fn heart_rate_cursor(items: &[Value]) -> Option<i64> {
    let mut max_ts: Option<i64> = None;
    for item in items {
        let Some(object) = item.as_object() else {
            continue;
        };
        let mut item_ts: Option<i64> = None;
        for key in ["timestamp", "time", "timeStamp", "startTime"] {
            let Some(value) = object.get(key) else {
                continue;
            };
            let parsed = match value {
                Value::Number(number) => number.as_i64(),
                Value::String(text) => text.trim().parse::<i64>().ok(),
                _ => None,
            };
            if let Some(parsed) = parsed {
                item_ts = Some(parsed);
                break;
            }
        }
        if let Some(ts) = item_ts {
            max_ts = Some(max_ts.map_or(ts, |current: i64| current.max(ts)));
        }
    }
    // Always convert to **seconds**, then step by one.
    //
    // The caller's `cursor` and `end` are both seconds (`window.start_utc.timestamp()`),
    // and both go unchanged into the request `startTime` / `endTime`. Before, this returned
    // `ts + 1000` for ms timestamps, which is a ms value: when one page held a ms timestamp,
    // the next request's `startTime` became much larger than `endTime`, the server judged
    // the range invalid and returned an empty page, so **for large heart rate data with 1000 full rows, everything after page 2
    // was lost**, and the sync still reported "success".
    max_ts.map(|ts| {
        let seconds = if ts >= 10_000_000_000 { ts / 1000 } else { ts };
        seconds.saturating_add(1)
    })
}
