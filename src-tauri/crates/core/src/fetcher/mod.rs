use crate::connectors::ZeppConnector;

use crate::models::{error::*, *};

use chrono::{DateTime, Duration, NaiveDate, Utc};

use serde_json::Value;

use std::collections::BTreeSet;

mod pages;
mod probe;
mod wellness;

use pages::*;
pub use probe::*;
use wellness::*;

#[derive(Debug, Clone, Copy)]
pub struct FetchWindow {
    pub start_utc: DateTime<Utc>,
    pub end_utc: DateTime<Utc>,
}

impl FetchWindow {
    /// Regular sync window. The limit is still 365 days: one request that covers too long a span makes the server time out.
    /// For older history, use history backfill. It splits by month and can resume.
    pub fn days(days: i64) -> Result<Self> {
        if !(1..=365).contains(&days) {
            return Err(ZeppBridgeError::ConfigError(
                "sync window days must be in 1..=365".into(),
            ));
        }
        let end_utc = Utc::now();
        Ok(Self {
            start_utc: end_utc - Duration::days(days),
            end_utc,
        })
    }

    /// Any range. History backfill splits by month itself, so the 365-day limit does not apply. This only checks
    /// that the range direction is correct.
    pub fn between(start_utc: DateTime<Utc>, end_utc: DateTime<Utc>) -> Result<Self> {
        if end_utc <= start_utc {
            return Err(ZeppBridgeError::ConfigError(
                "fetch window end time must be later than start time".into(),
            ));
        }
        Ok(Self { start_utc, end_utc })
    }

    pub fn start_day(&self) -> String {
        self.start_utc.format("%Y-%m-%d").to_string()
    }

    pub fn end_day(&self) -> String {
        // Timestamp windows are half-open; day endpoints include both dates.
        self.end_utc
            .checked_sub_signed(Duration::nanoseconds(1))
            .unwrap_or(self.end_utc)
            .format("%Y-%m-%d")
            .to_string()
    }

    /// End day for endpoints that file data by "device local day": the UTC end day plus one day.
    ///
    /// Sleep (band_data detail) and WatchSportStatistics are filed by device local date. Sleep is filed on
    /// **the wake-up day**. Waking at 07:00 in UTC+8 is October 1 local time but still September 30 UTC: a request by UTC
    /// date misses last night's sleep until after 08:00 (and the sync report still says success). In any time zone the
    /// local date is at most one day ahead of UTC, so one extra day at the end is enough. In the western hemisphere the extra day is empty.
    pub fn end_day_local_padded(&self) -> String {
        let end = (self.end_utc - Duration::nanoseconds(1)).date_naive();
        end.succ_opt().unwrap_or(end).format("%Y-%m-%d").to_string()
    }

    /// Convert the half-open timestamp window to inclusive device-local dates.
    /// Use the zone's rules at each endpoint, including historical DST changes.
    fn local_days(&self, time_zone: &str) -> Result<(NaiveDate, NaiveDate)> {
        let zone = jiff::tz::TimeZone::get(time_zone)
            .map_err(|error| ZeppBridgeError::ConfigError(error.to_string()))?;
        let date = |instant: DateTime<Utc>| -> Result<NaiveDate> {
            let timestamp = jiff::Timestamp::from_second(instant.timestamp())
                .map_err(|error| ZeppBridgeError::ConfigError(error.to_string()))?;
            NaiveDate::parse_from_str(
                &timestamp.to_zoned(zone.clone()).date().to_string(),
                "%Y-%m-%d",
            )
            .map_err(|error| ZeppBridgeError::ConfigError(error.to_string()))
        };
        Ok((
            date(self.start_utc)?,
            date(self.end_utc - Duration::nanoseconds(1))?,
        ))
    }

    pub fn chunks(self, chunk_days: i64) -> Vec<Self> {
        let chunk_days = chunk_days.max(1);
        let mut chunks = Vec::new();
        let mut cursor = self.start_utc;
        while cursor < self.end_utc {
            let next = (cursor + Duration::days(chunk_days)).min(self.end_utc);
            if next > cursor {
                chunks.push(Self {
                    start_utc: cursor,
                    end_utc: next,
                });
            }
            cursor = next;
        }
        if chunks.is_empty() {
            chunks.push(self);
        }
        chunks
    }

    /// Split by UTC day. Each day is a **full day**: from that midnight to the next midnight, and today up to `now`.
    ///
    /// Heart rate payloads split this way and use day keys (`heart_rate:day:<date>:<page>`). Before, the key held the seconds of
    /// both window ends, and the window moves with "now": a sync every 15 minutes added a few new payloads, hundreds per day,
    /// never cleaned up, and full replay got slower and slower. With full days, each request for a past day is identical,
    /// so "skip if content is unchanged" works for it. Today's row grows with time and is overwritten in place.
    ///
    /// The first day also starts at midnight, not at the window start: a key for one day must always map to the same time span. A half-day payload
    /// must not overwrite a full-day one.
    pub fn utc_days(self, now: DateTime<Utc>) -> Vec<(NaiveDate, Self)> {
        let first = self.start_utc.date_naive();
        let last = (self.end_utc - Duration::nanoseconds(1)).date_naive();
        let mut out = Vec::new();
        let mut day = first;
        while day <= last {
            let (start_utc, next) = crate::event_days::day_bounds(day);
            let end_utc = next.min(now.max(start_utc + Duration::seconds(1)));
            out.push((day, Self { start_utc, end_utc }));
            let Some(following) = day.succ_opt() else {
                break;
            };
            day = following;
        }
        out
    }

    /// The same slices as [`Self::chunks`], but ordered from the newest.
    ///
    /// Slice bounds do not change (the payload source_key follows the bounds). Only the slice nearest to now is fetched first:
    /// the home page needs "latest heart rate / today's steps / last night's sleep" and must not wait for the full history.
    pub fn chunks_newest_first(self, chunk_days: i64) -> Vec<Self> {
        let mut chunks = self.chunks(chunk_days);
        chunks.reverse();
        chunks
    }
}

/// A fetch result keeps endpoint/source identity beside its raw payload. This
/// is what allows sync to retain provenance before normalization.
#[derive(Debug, Clone)]
pub struct FetchedRecord {
    pub raw: RawRecord,
    /// A sub-slice in the request window failed (404 / unavailable), but other slices already returned payloads.
    ///
    /// The caller must treat the whole window as incomplete: data already fetched can be written, but
    /// this month must not be recorded as persisted / empty_from_cloud.
    pub incomplete: bool,
    /// Diagnostic context for partial fetches; never replaces successfully fetched data.
    pub incomplete_reason: Option<String>,
}

impl FetchedRecord {
    fn from_raw(raw: RawRecord) -> Self {
        Self {
            raw,
            incomplete: false,
            incomplete_reason: None,
        }
    }
}

pub struct DataFetcher {
    connector: ZeppConnector,
}

impl DataFetcher {
    pub fn new(connector: ZeppConnector) -> Self {
        Self { connector }
    }

    pub async fn fetch_heart_rate_records(
        &self,
        window: FetchWindow,
    ) -> Result<Vec<FetchedRecord>> {
        let mut records = Vec::new();
        let mut last_error = None;
        for (day, chunk) in window.utc_days(Utc::now()) {
            match fetch_heart_rate_pages_with(day, chunk, |cursor, end| {
                self.connector
                    .fetch_heart_rate_with_options(cursor, end, HEART_RATE_PAGE_LIMIT, 2)
            })
            .await
            {
                Ok(pages) => records.extend(pages),
                Err(error) if is_abort_error(&error) => return Err(error),
                Err(error) if error.is_unavailable() => last_error = Some(error),
                Err(error) => return Err(error),
            }
        }
        conclude_slices(records, last_error, "heart rate window has no recognizable records")
    }

    /// Cloud check before sync (`sync/probe.rs`): the time of the latest heart rate in cloud band data from the day of `since` to today.
    /// `None` if there is no heart rate at all (the probe then treats it as "unknown" and syncs as usual).
    ///
    /// It reads `band_data` details, **the same source as local heart rate** (`band_data:detail` of the `sleep` stream).
    /// Before, it read `/users/{id}/heartRate`: for some accounts that endpoint always returns an empty page (the `heart_rate` stream is unavailable),
    /// and the empty page counted as "cloud is not newer". So after the phone synced, the desktop still said "not uploaded to the cloud yet" (user test on 10-08).
    /// Nothing is stored and no payload is kept.
    pub async fn newest_band_heart_rate(
        &self,
        since: DateTime<Utc>,
    ) -> Result<Option<DateTime<Utc>>> {
        let window = FetchWindow::between(since - Duration::minutes(1), Utc::now())?;
        let record = self.fetch_sleep_record(window).await?;
        let band = crate::normalizer::Normalizer::normalize_band_data(&record.raw.payload)?;
        Ok(band
            .heart_rate_samples
            .iter()
            .map(|sample| sample.timestamp)
            .max())
    }

    pub async fn fetch_sport_detail_record(
        &self,
        workout_id: &str,
        source: &str,
        start_utc: DateTime<Utc>,
        end_utc: Option<DateTime<Utc>>,
    ) -> Result<FetchedRecord> {
        let payload = self
            .connector
            .fetch_sport_detail(workout_id, source)
            .await?;
        Ok(FetchedRecord::from_raw(RawRecord {
            stream: "workout_detail".into(),
            source_key: format!("workout_detail:{workout_id}:{source}"),
            source_scope: SourceScope::Device,
            device_id: None,
            start_utc,
            end_utc,
            payload,
            capability: CapabilityStatus::Verified,
        }))
    }

    pub async fn fetch_sleep_records(&self, window: FetchWindow) -> Result<Vec<FetchedRecord>> {
        fetch_sleep_slices_with(window, |chunk| self.fetch_sleep_record(chunk)).await
    }

    pub async fn fetch_sleep_record(&self, window: FetchWindow) -> Result<FetchedRecord> {
        let payload = self
            .connector
            .fetch_band_data(
                &window.start_day(),
                &window.end_day_local_padded(),
                "detail",
                8,
                0,
            )
            .await?;
        let capability = crate::normalizer::Normalizer::band_capability(&payload);
        Ok(FetchedRecord::from_raw(RawRecord {
            stream: "sleep".into(),
            source_key: format!(
                "band_data:detail:{}:{}",
                window.start_day(),
                window.end_day()
            ),
            source_scope: SourceScope::Device,
            device_id: None,
            start_utc: window.start_utc,
            end_utc: Some(window.end_utc),
            payload,
            capability,
        }))
    }

    /// Sport history uses track IDs, not timestamps. The range helper uses the
    /// UTC epoch as a conservative cursor window because no local track index is
    /// known yet; a server response with no structured records is reported as
    /// unavailable rather than as a successful empty workout stream.
    pub async fn fetch_workout_records(&self, window: FetchWindow) -> Result<Vec<FetchedRecord>> {
        let start = window.start_utc.timestamp();
        let end = window.end_utc.timestamp();
        let mut records = Vec::new();
        // Despite its path, Zepp's run history endpoint is the account-wide
        // workout feed. The activity kind lives in each record's numeric
        // `type`; sibling paths such as `/strength/history.json` normally 404
        // and must not be treated as separate feeds.
        let sports = ["run"];
        let mut last_optional_error = None;
        for sport in sports {
            // Zepp limits records per page. Response data.next is the stopTrackId cursor for the next page
            // (-1/0/missing = no more). Without paging, older records in the window are lost.
            let mut stop_track_id = end;
            loop {
                match self
                    .connector
                    .fetch_sport_history(sport, start, stop_track_id, 1)
                    .await
                {
                    Ok(payload) => {
                        let next = payload
                            .pointer("/data/next")
                            .and_then(Value::as_i64)
                            .unwrap_or(-1);
                        records.push(FetchedRecord::from_raw(RawRecord {
                            stream: "workouts".into(),
                            source_key: sport_history_key(sport, &payload),
                            source_scope: SourceScope::Device,
                            device_id: None,
                            start_utc: window.start_utc,
                            end_utc: Some(window.end_utc),
                            payload,
                            capability: CapabilityStatus::Verified,
                        }));
                        // Stop when the cursor no longer moves toward the window start, so a server fault cannot cause an endless loop.
                        if next <= 0 || next >= stop_track_id || next <= start {
                            break;
                        }
                        stop_track_id = next;
                    }
                    Err(error) if is_abort_error(&error) => return Err(error),
                    Err(error) if error.is_unavailable() => {
                        last_optional_error = Some(error);
                        break;
                    }
                    Err(error) => return Err(error),
                }
            }
        }
        if records.is_empty() {
            return Err(last_optional_error.unwrap_or_else(|| {
                ZeppBridgeError::Unavailable("sport history has no available types".into())
            }));
        }
        Ok(records)
    }

    pub async fn fetch_hrv_records(&self, window: FetchWindow) -> Result<Vec<FetchedRecord>> {
        let mut records = Vec::new();
        let mut last_error = None;
        for chunk in window.chunks(7) {
            match self
                .connector
                .fetch_hrv(&chunk.start_day(), &chunk.end_day())
                .await
            {
                Ok(payload) => records.push(FetchedRecord::from_raw(RawRecord {
                    stream: "hrv".into(),
                    source_key: format!(
                        "events:hrv_sdnn:{}:{}",
                        chunk.start_day(),
                        chunk.end_day()
                    ),
                    source_scope: SourceScope::UserFused,
                    device_id: None,
                    start_utc: chunk.start_utc,
                    end_utc: Some(chunk.end_utc),
                    payload,
                    capability: CapabilityStatus::Verified,
                })),
                Err(error) if is_abort_error(&error) => return Err(error),
                Err(error) if error.is_unavailable() => last_error = Some(error),
                Err(error) => return Err(error),
            }
        }
        conclude_slices(records, last_error, "HRV window has no recognizable records")
    }

    pub async fn fetch_daily_statistics_records(
        &self,
        window: FetchWindow,
    ) -> Result<Vec<FetchedRecord>> {
        fetch_daily_summary_slices_with(window, |slice| self.fetch_daily_statistics_slice(slice))
            .await
    }

    async fn fetch_daily_statistics_slice(
        &self,
        window: FetchWindow,
    ) -> Result<Vec<FetchedRecord>> {
        let (from, to) =
            crate::event_days::utc_day_aligned_millis(window.start_utc, window.end_utc, Utc::now());
        fetch_daily_statistics_slice_with(window, |event_type, sub_type| async move {
            if event_type == "WatchSportStatistics" {
                self.connector
                    .fetch_watch_statistics(
                        sub_type,
                        &window.start_day(),
                        &window.end_day_local_padded(),
                        900,
                        true,
                    )
                    .await
            } else {
                self.connector
                    .fetch_events(event_type, Some(sub_type), from, to, 2000, true)
                    .await
            }
        })
        .await
    }
}

/// Key of one sport history page payload: built from **content**, not from the request window.
///
/// This endpoint pages by a track ID cursor, and the request start moves with "now". Before, the key held the start seconds,
/// so each sync made a new payload (in the dev database, 67 of 69 had no workout pointing at them). With content keys:
/// the same cloud response is the same row and skips normalization. A new workout gives a new row; the workouts in the old row move to
/// the new payload, nothing points at the old one, and `prune_superseded_page_raws` removes it. The stream name prefix does not change:
/// normalization reads the type from the key (`sport_history:<type>:`).
pub(crate) fn sport_history_key(sport: &str, payload: &Value) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(payload.to_string().as_bytes());
    let digest = hex::encode(hasher.finalize());
    format!("sport_history:{sport}:h{}", &digest[..16])
}

/// Response of one daily event window -> one raw payload per UTC day.
///
/// An empty response, a response of unknown shape, and items without a timestamp still keep one row for the window:
/// "this window is empty" is a fact we keep too. Its key ends at the last millisecond of that day, so a refetch on the same day
/// overwrites it.
fn event_day_records(
    event_type: &str,
    sub_type: &str,
    from: i64,
    to: i64,
    payload: Value,
) -> Vec<FetchedRecord> {
    let record = |source_key: String, start_utc, end_utc, payload| {
        FetchedRecord::from_raw(RawRecord {
            stream: "daily_summary".into(),
            source_key,
            source_scope: SourceScope::UserFused,
            device_id: None,
            start_utc,
            end_utc: Some(end_utc),
            payload,
            capability: CapabilityStatus::Verified,
        })
    };
    let whole = |payload| {
        let start = DateTime::<Utc>::from_timestamp_millis(from).unwrap_or_default();
        let end = DateTime::<Utc>::from_timestamp_millis(to.saturating_add(1)).unwrap_or(start);
        let key_to = crate::event_days::end_of_utc_day_millis(to);
        record(
            format!("events:{event_type}:{sub_type}:{from}:{key_to}"),
            start,
            end,
            payload,
        )
    };
    let Some(split) = crate::event_days::split_by_utc_day(&payload) else {
        return vec![whole(payload)];
    };
    let mut records: Vec<FetchedRecord> = split
        .days
        .into_iter()
        .map(|(day, payload)| {
            let (start, end) = crate::event_days::day_bounds(day);
            record(
                crate::event_days::day_source_key(event_type, sub_type, day),
                start,
                end,
                payload,
            )
        })
        .collect();
    if let Some(residual) = split.residual {
        records.push(whole(residual));
    }
    records
}

#[cfg(test)]
mod tests;
