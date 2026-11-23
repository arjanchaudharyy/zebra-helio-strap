//! Latest samples, heart rate and stress curves, local coverage range (split out of storage/mod.rs by domain, logic unchanged).

use super::*;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StreamFreshness {
    pub last_cloud_sync_at: Option<String>,
    pub newest_sample_at: Option<String>,
}

/// The span of days that actually has data on this device.
///
/// Why it exists: the UI offers "6 months" everywhere, but those pickers all read the local database.
/// With only 30 days in the database, picking 6 months only stretches the axis and the first five months are empty. Before this,
/// nothing told the user so, and "I picked 6 months but see only 30 days" was reported
/// as a bug. It is not a bug. We just did not say it.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LocalCoverage {
    /// Earliest day (`YYYY-MM-DD`). `None` when the database is empty.
    pub earliest_day: Option<String>,
    /// Latest day (`YYYY-MM-DD`).
    pub latest_day: Option<String>,
    /// Number of days from `earliest_day` to today. Compared directly with "the range you picked".
    pub covered_days: i64,
}

impl Database {
    pub fn heart_rate_series(&self, hours: i64) -> Result<Vec<HeartRatePoint>> {
        let hours = hours.clamp(1, 24 * 14);
        let cutoff = (Utc::now() - chrono::Duration::hours(hours)).to_rfc3339();
        // Two sources (band_data device rows, heartRate API user_fused
        // rows) can hold the same minute; collapse to one row per timestamp
        // preferring user_fused so charts never draw duplicate points.
        let mut stmt = self.conn.prepare(
            "SELECT m.timestamp, m.value
             FROM metric_samples m
             WHERE m.metric = 'heart_rate' AND m.timestamp >= ?1
               AND m.id = (
                   SELECT id FROM metric_samples
                   WHERE metric = 'heart_rate' AND timestamp = m.timestamp
                   ORDER BY CASE source_scope
                       WHEN 'user_fused' THEN 0
                       WHEN 'device' THEN 1
                       ELSE 2 END, id
                   LIMIT 1)
             ORDER BY m.timestamp ASC",
        )?;
        let rows = stmt.query_map([cutoff], |row| {
            Ok(HeartRatePoint {
                timestamp: row.get(0)?,
                value: row.get(1)?,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    /// All-day stress curve: individual readings from `metric_samples`, in time order.
    ///
    /// Unlike the heart rate curve, no dedup by source is needed: stress has only one source,
    /// `all_day_stress`. Duplicate readings at one instant can only come from two watches, and then both must stay.
    /// The unique index already includes `device_id`.
    ///
    /// Periods with no samples get no filler points. A watch not worn all night means no data. A flat 0 line
    /// would make those hours look especially relaxed.
    pub fn stress_series(&self, hours: i64) -> Result<Vec<StressPoint>> {
        let hours = hours.clamp(1, 24 * 14);
        let cutoff = (Utc::now() - chrono::Duration::hours(hours)).to_rfc3339();
        let mut stmt = self.conn.prepare(
            "SELECT timestamp, value FROM metric_samples
             WHERE metric = 'stress' AND timestamp >= ?1
             ORDER BY timestamp ASC",
        )?;
        let rows = stmt.query_map([cutoff], |row| {
            Ok(StressPoint {
                timestamp: row.get(0)?,
                value: row.get(1)?,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub fn stream_freshness(&self) -> Result<BTreeMap<String, StreamFreshness>> {
        let mut freshness = BTreeMap::<String, StreamFreshness>::new();
        let mut stmt = self.conn.prepare(
            "SELECT stream, MAX(fetched_at) FROM raw_records GROUP BY stream ORDER BY stream",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
        })?;
        for row in rows {
            let (stream, timestamp) = row?;
            freshness.entry(stream).or_default().last_cloud_sync_at = timestamp;
        }

        // Heart-rate can legitimately fall back to minute samples decoded from
        // band_data, so the sleep fetch is also a heart-rate cloud source.
        let sleep_fetch = freshness
            .get("sleep")
            .and_then(|value| value.last_cloud_sync_at.clone());
        if let Some(sleep_fetch) = sleep_fetch {
            let heart_rate = freshness.entry("heart_rate".into()).or_default();
            if heart_rate.last_cloud_sync_at.as_deref() < Some(sleep_fetch.as_str()) {
                heart_rate.last_cloud_sync_at = Some(sleep_fetch);
            }
        }

        for (stream, query) in [
            (
                "heart_rate",
                "SELECT MAX(timestamp) FROM metric_samples
                 WHERE metric = 'heart_rate' AND import_batch_id IS NULL",
            ),
            (
                "hrv",
                "SELECT MAX(timestamp) FROM metric_samples
                 WHERE metric = 'hrv' AND import_batch_id IS NULL",
            ),
            // Data imported from other apps does not count as "how far the cloud fetch got": the pre-sync probe compares the latest heart rate time with the cloud.
            (
                "daily_summary",
                "SELECT MAX(date) FROM daily_metrics WHERE import_batch_id IS NULL",
            ),
            (
                "sleep",
                "SELECT MAX(end_time) FROM sleep_sessions WHERE import_batch_id IS NULL",
            ),
            ("workouts", "SELECT MAX(end_time) FROM workouts"),
        ] {
            let timestamp = self
                .conn
                .query_row(query, [], |row| row.get::<_, Option<String>>(0))?;
            freshness.entry(stream.into()).or_default().newest_sample_at = timestamp;
        }
        Ok(freshness)
    }

    /// The span of days that actually has data on this device.
    ///
    /// Asks four tables for their earliest/latest and takes the union: looking at only one would under-report
    /// by months on accounts that have workouts but no daily summary. It returns **days**, not timestamps, because it is compared
    /// directly with the day-based UI choices "last 7 days / 30 days / 6 months".
    pub fn local_coverage(&self, today: NaiveDate) -> Result<LocalCoverage> {
        let (earliest, latest) = self.coverage_day_bounds()?;

        // Coverage days count from the earliest day to **today**, not to `latest_day`: the user asks
        // "how far back can I look", and two unsynced days must not make the answer smaller.
        let covered_days = earliest
            .as_deref()
            .and_then(|day| NaiveDate::parse_from_str(day, "%Y-%m-%d").ok())
            .map(|day| (today - day).num_days() + 1)
            .unwrap_or(0)
            .max(0);

        Ok(LocalCoverage {
            earliest_day: earliest,
            latest_day: latest,
            covered_days,
        })
    }

    /// Earliest/latest **local day** of each of the four tables, as a union. An empty database returns `(None, None)`.
    ///
    /// `date(x, 'localtime')` is monotonic non-decreasing in `x`, so taking MIN/MAX of the raw column first and
    /// converting to a local day in Rust gives the same result as "convert each row, then MIN/MAX". Only the raw
    /// column can use an index: the daily_metrics unique key starts with date, and since v33 workouts
    /// has idx_workouts_start. metric_samples / sleep_sessions still scan the table,
    /// but skip the cost of one date() call per row.
    ///
    /// An extreme row that cannot be parsed returns None. Before, date() turned such rows into NULL and they
    /// dropped out of MIN/MAX. A failed conversion makes this table report no bounds, the same as "no data",
    /// instead of failing the whole query.
    pub(super) fn coverage_day_bounds(&self) -> Result<(Option<String>, Option<String>)> {
        self.coverage_day_bounds_in(false)
    }

    pub(super) fn coverage_day_bounds_in(
        &self,
        visible_only: bool,
    ) -> Result<(Option<String>, Option<String>)> {
        let mut earliest: Option<String> = None;
        let mut latest: Option<String> = None;
        // Both ends of the sample table are taken per metric on the unique index (see `sample_timestamp_bounds`).
        // The other three tables already use an index or are small.
        let samples = self.sample_timestamp_bounds()?;
        for (query, is_timestamp) in [
            ("SELECT MIN(date), MAX(date) FROM daily_metrics", false),
            ("", true),
            (
                "SELECT MIN(start_time), MAX(end_time) FROM sleep_sessions",
                true,
            ),
            (
                if visible_only {
                    "SELECT MIN(start_time), MAX(end_time) FROM workouts_visible"
                } else {
                    "SELECT MIN(start_time), MAX(end_time) FROM workouts"
                },
                true,
            ),
        ] {
            let (low, high) = if query.is_empty() {
                samples.clone()
            } else {
                self.conn.query_row(query, [], |row| {
                    Ok((
                        row.get::<_, Option<String>>(0)?,
                        row.get::<_, Option<String>>(1)?,
                    ))
                })?
            };
            let low = low.and_then(|value| {
                if is_timestamp {
                    local_day_of_stored(&value)
                } else {
                    Some(value)
                }
            });
            let high = high.and_then(|value| {
                if is_timestamp {
                    local_day_of_stored(&value)
                } else {
                    Some(value)
                }
            });
            if let Some(low) = low {
                if earliest
                    .as_deref()
                    .is_none_or(|current| low.as_str() < current)
                {
                    earliest = Some(low);
                }
            }
            if let Some(high) = high {
                if latest
                    .as_deref()
                    .is_none_or(|current| high.as_str() > current)
                {
                    latest = Some(high);
                }
            }
        }
        Ok((earliest, latest))
    }

    /// Metric names that occur in the sample table, ascending.
    ///
    /// A hand-written skip scan: each step finds "the next metric greater than the previous one" on the unique
    /// index `(metric, timestamp, …)`, so it does only "metric count + 1" index lookups. `SELECT DISTINCT`
    /// would read an index of over 300,000 rows from start to end.
    pub(super) fn sample_metric_names(&self) -> Result<Vec<String>> {
        let mut statement = self.conn.prepare_cached(
            "SELECT metric FROM metric_samples WHERE metric > ?1 ORDER BY metric LIMIT 1",
        )?;
        let mut names = Vec::new();
        let mut cursor = String::new();
        while let Some(next) = statement
            .query_row([&cursor], |row| row.get::<_, String>(0))
            .optional()?
        {
            cursor.clone_from(&next);
            names.push(next);
        }
        Ok(names)
    }

    /// Earliest / latest timestamp of the whole sample table, taken once at each end of the index per metric.
    /// The old single `MIN(timestamp), MAX(timestamp)` scanned the whole index (about 50 ms on a real database).
    pub(super) fn sample_timestamp_bounds(&self) -> Result<(Option<String>, Option<String>)> {
        let mut low: Option<String> = None;
        let mut high: Option<String> = None;
        // MIN and MAX must be two statements: SQLite's min/max optimization applies only when "the statement has a single
        // MIN or MAX". In one statement it falls back to scanning every row of this metric.
        let mut first_of = self
            .conn
            .prepare_cached("SELECT MIN(timestamp) FROM metric_samples WHERE metric = ?1")?;
        let mut last_of = self
            .conn
            .prepare_cached("SELECT MAX(timestamp) FROM metric_samples WHERE metric = ?1")?;
        for metric in self.sample_metric_names()? {
            let first: Option<String> = first_of.query_row([&metric], |row| row.get(0))?;
            let last: Option<String> = last_of.query_row([&metric], |row| row.get(0))?;
            if let Some(first) = first {
                if low
                    .as_deref()
                    .is_none_or(|current| first.as_str() < current)
                {
                    low = Some(first);
                }
            }
            if let Some(last) = last {
                if high
                    .as_deref()
                    .is_none_or(|current| last.as_str() > current)
                {
                    high = Some(last);
                }
            }
        }
        Ok((low, high))
    }

    pub fn newest_samples(&self) -> Result<BTreeMap<String, Option<String>>> {
        Ok(self
            .stream_freshness()?
            .into_iter()
            .map(|(stream, value)| (stream, value.newest_sample_at))
            .collect())
    }
}
