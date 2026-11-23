//! Daily metric series, training load and latest readings (split out of storage/mod.rs by domain, logic unchanged).

use super::*;

impl Database {
    /// Daily series for the body and training screens.
    ///
    /// Only names in `SERIES_METRICS` / `SAMPLE_ONLY_SERIES_METRICS` are
    /// answered; anything else is skipped rather than guessed at, so a typo in
    /// a caller cannot produce a chart with an invented unit.
    /// Aggregates raw heart rate samples per day: the day's max, min, mean and the sample count.
    ///
    /// Why not `daily_metrics`: Zepp's **daily** max heart rate is not collected at all.
    /// The only thing in the database called `device_max_hr` comes from `maxHr` in the PAI stream. That is the watch's
    /// max heart rate **setting** (the 100–240 number used for zones), not the peak measured
    /// that day. Showing it as the daily max would be one more case of "the UI shows a number, but it does not mean
    /// what you think".
    ///
    /// So this does one thing only: take the per-day max of the raw samples stored locally. The number the user sees
    /// can differ from the Zepp App. That is normal: Zepp filters, we do not.
    ///
    /// **`samples` must be returned too.** When a day has only 12 samples, that "max" is
    /// the max of 12 points, not of the day. Without the sample count, the UI could only draw it
    /// as a complete max, and that would invent a fact.
    ///
    /// Days are split in the local time zone: the user asks about "my day", not "that UTC day".
    ///
    /// Day splitting no longer uses SQLite's `date(timestamp, 'localtime')`: it does a system time zone
    /// conversion per row, and 180 days of over 200,000 rows took about 170-180 ms. Now we first compute where each
    /// local day in the window starts in UTC (chrono handles DST with the local zone rules, the same
    /// rules as `localtime`), then assign (time, value) to days in index order. Sample times use one
    /// format, `YYYY-MM-DDTHH:MM:SS+00:00`, so string order is time order. Rows in other formats are converted one by one,
    /// and rows that fail to convert are skipped (the same meaning as `date()` returning NULL and BETWEEN filtering it out).
    /// The max, min, mean and count algorithm is unchanged: the mean is still the sum of all the day's samples divided by the count.
    pub fn daily_heart_rate_extremes(&self, days: i64) -> Result<Vec<DailyHeartRateExtreme>> {
        let window_days = days.clamp(1, 1825);
        let end = Local::now().date_naive();
        let start = end - Duration::days(window_days - 1);
        let start_text = start.format("%Y-%m-%d").to_string();
        let end_text = end.format("%Y-%m-%d").to_string();
        // UTC margin bounds let uq_metric_sample_key cut a range by (metric, timestamp) in time order.
        // The exact local day split happens below.
        let (utc_lower, utc_upper) = utc_bounds_or_unbounded(&start_text, &end_text);
        let buckets = LocalDayBuckets::new(start, end);
        let mut days: Vec<DayAccumulator> = Vec::new();
        days.resize_with(buckets.len(), DayAccumulator::default);
        let mut stmt = self.conn.prepare_cached(
            "SELECT timestamp, value FROM metric_samples
             WHERE metric = 'heart_rate' AND timestamp >= ?1 AND timestamp < ?2
             ORDER BY timestamp",
        )?;
        let mut rows = stmt.query([utc_lower, utc_upper])?;
        let mut cursor = 0;
        while let Some(row) = rows.next()? {
            // date() turns a non-text time into NULL and it is filtered out: skip it here too.
            let Some(day) = row
                .get_ref(0)?
                .as_str()
                .ok()
                .and_then(|timestamp| buckets.day_of(timestamp, &mut cursor))
            else {
                continue;
            };
            days[day].add(row.get::<_, f64>(1)?);
        }
        // Format the date string only once per day: formatting per row costs more than the whole query.
        Ok(days
            .into_iter()
            .enumerate()
            .filter(|(_, day)| day.count > 0)
            .map(|(index, day)| DailyHeartRateExtreme {
                date: buckets.date(index),
                max: day.max.round() as i32,
                min: day.min.round() as i32,
                average: (day.sum / day.count as f64).round() as i32,
                samples: day.count,
            })
            .collect())
    }

    /// Read meal details from retained food payloads without a migration or replay.
    /// Newest snapshots win when overlapping sync windows contain the same log ID.
    pub fn food_entries(&self, days: i64) -> Result<Vec<FoodEntry>> {
        let end = Local::now().date_naive();
        let start = (end - Duration::days(days.clamp(1, 1825) - 1)).to_string();
        let end = end.to_string();
        let mut stmt = self.conn.prepare(
            "SELECT source_key, payload, payload_zip FROM raw_records
             WHERE stream = 'wellness' AND source_key LIKE 'wellness:food:%'
             ORDER BY fetched_at DESC, id DESC",
        )?;
        let mut rows = stmt.query([])?;
        let mut seen = BTreeSet::new();
        let mut entries = Vec::new();
        while let Some(row) = rows.next()? {
            let key: String = row.get(0)?;
            let payload = decode_raw_payload(row.get(1)?, row.get(2)?)?;
            let payload = serde_json::from_str(&payload)
                .map_err(|error| ZeppBridgeError::ParseError(error.to_string()))?;
            for entry in Normalizer::normalize_wellness(&key, &payload).food_entries {
                let identity = match &entry.food_log_id {
                    Some(id) => format!("id:{id}"),
                    None => serde_json::to_string(&entry)
                        .map_err(|error| ZeppBridgeError::ParseError(error.to_string()))?,
                };
                if seen.insert(identity) && entry.date >= start && entry.date <= end {
                    entries.push(entry);
                }
            }
        }
        entries.sort_by(|a, b| {
            b.date
                .cmp(&a.date)
                .then_with(|| b.mealtime.cmp(&a.mealtime))
        });
        Ok(entries)
    }

    pub fn metric_series(&self, metrics: &[String], days: i64) -> Result<Vec<MetricSeries>> {
        // The leaderboard "All" view (3.0.4) fetches ten calendar years at once.
        let window_days = days.clamp(1, 3700);
        let end = Local::now().date_naive();
        let start = end - Duration::days(window_days - 1);
        let start_text = start.format("%Y-%m-%d").to_string();
        let end_text = end.format("%Y-%m-%d").to_string();

        let mut result = Vec::new();
        for metric in metrics {
            let daily = SERIES_METRICS
                .iter()
                .find(|(name, _, _)| name == metric)
                .map(|(name, source, unit)| (*name, *source, *unit));
            let sample_only = SAMPLE_ONLY_SERIES_METRICS
                .iter()
                .find(|(name, _)| name == metric)
                .map(|(name, unit)| (*name, MetricSource::Samples, *unit));
            let Some((name, source, unit)) = daily.or(sample_only) else {
                continue;
            };

            let points = match source {
                MetricSource::Daily(spread) => {
                    self.daily_metric_points(name, spread, &start_text, &end_text)?
                }
                MetricSource::Samples => self.sample_metric_points(name, &start_text, &end_text)?,
                MetricSource::SleepScores => self.sleep_score_points(&start_text, &end_text)?,
            };

            let values: Vec<f64> = points.iter().map(|point| point.value).collect();
            let (minimum, maximum) = metric_series_extrema(name, &points);
            result.push(MetricSeries {
                metric: name.to_string(),
                unit: unit.to_string(),
                source: match source {
                    MetricSource::Daily(_) => "daily_metrics".to_string(),
                    MetricSource::Samples => "metric_samples".to_string(),
                    MetricSource::SleepScores => "sleep_sessions".to_string(),
                },
                latest: points.last().cloned(),
                average: average_finite(values.iter().copied()).map(round1),
                minimum,
                maximum,
                days_with_data: points.len() as i64,
                window_days,
                points,
            });
        }
        Ok(result)
    }

    /// A night's score belongs to its local wake date. If more than one
    /// session ends on a date, prefer the fused session then the latest end.
    pub(crate) fn sleep_score_points(
        &self,
        start: &str,
        end: &str,
    ) -> Result<Vec<MetricSeriesPoint>> {
        let mut stmt = self.conn.prepare(
            "SELECT date(s.end_time, 'localtime'), s.score
             FROM sleep_sessions_shown s
             WHERE s.score IS NOT NULL
               AND date(s.end_time, 'localtime') BETWEEN ?1 AND ?2
               AND s.id = (
                   SELECT s2.id FROM sleep_sessions_shown s2
                   WHERE s2.score IS NOT NULL
                     AND date(s2.end_time, 'localtime') = date(s.end_time, 'localtime')
                   ORDER BY CASE s2.source_scope
                       WHEN 'user_fused' THEN 0 WHEN 'device' THEN 1 ELSE 2 END,
                       s2.end_time DESC, s2.id DESC LIMIT 1)
             ORDER BY date(s.end_time, 'localtime')",
        )?;
        let rows = stmt.query_map(params![start, end], |row| {
            Ok(MetricSeriesPoint {
                date: row.get(0)?,
                value: row.get::<_, i32>(1)? as f64,
                min: None,
                max: None,
                samples: None,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    /// Inventory normalized metrics, including sleep scores stored with sessions
    /// and names not exposed by the chart contract. Never inspect raw payloads.
    pub fn stored_metrics(&self) -> Result<Vec<StoredMetric>> {
        let mut stmt = self.conn.prepare(
            "SELECT metric, 'daily_metrics', unit, COUNT(*), MIN(date), MAX(date)
             FROM daily_metrics GROUP BY metric, unit
             UNION ALL
             SELECT metric, 'metric_samples', unit, COUNT(*),
                    MIN(date(timestamp, 'localtime')),
                    MAX(date(timestamp, 'localtime'))
             FROM metric_samples GROUP BY metric, unit
             UNION ALL
             SELECT 'sleep_score', 'sleep_sessions', 'score', COUNT(*),
                    MIN(date(end_time, 'localtime')),
                    MAX(date(end_time, 'localtime'))
             FROM sleep_sessions_shown WHERE score IS NOT NULL HAVING COUNT(*) > 0
             ORDER BY metric, 2, unit",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(StoredMetric {
                metric: row.get(0)?,
                source: row.get(1)?,
                unit: row.get(2)?,
                records: row.get(3)?,
                first_date: row.get(4)?,
                last_date: row.get(5)?,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    /// Page exact stored values for any discovered metric. The source is
    /// explicit because some names occur in multiple tables with different meaning.
    pub fn stored_metric_records(
        &self,
        metric: &str,
        source: &str,
        start: Option<&str>,
        end: Option<&str>,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<StoredMetricRecord>> {
        let valid_date = |value: &str| {
            NaiveDate::parse_from_str(value, "%Y-%m-%d")
                .is_ok_and(|date| date.format("%Y-%m-%d").to_string() == value)
        };
        if metric.is_empty()
            || start.is_some_and(|value| !valid_date(value))
            || end.is_some_and(|value| !valid_date(value))
            || matches!((start, end), (Some(a), Some(b)) if a > b)
            || !["daily_metrics", "metric_samples", "sleep_sessions"].contains(&source)
            || (source == "sleep_sessions" && metric != "sleep_score")
        {
            return Err(ZeppBridgeError::ConfigError("Invalid metric query".into()));
        }
        let limit = limit.clamp(1, 201) as i64;
        let offset = offset.min(1_000_000) as i64;
        let query = if source == "daily_metrics" {
            "SELECT date, NULL, value, unit, source_scope FROM daily_metrics
             WHERE metric = ?1 AND (?2 IS NULL OR date >= ?2)
               AND (?3 IS NULL OR date <= ?3)
             ORDER BY date DESC, id DESC LIMIT ?4 OFFSET ?5"
        } else if source == "metric_samples" {
            "SELECT date(timestamp, 'localtime'), timestamp, value, unit, source_scope
             FROM metric_samples WHERE metric = ?1
               AND (?2 IS NULL OR date(timestamp, 'localtime') >= ?2)
               AND (?3 IS NULL OR date(timestamp, 'localtime') <= ?3)
             ORDER BY timestamp DESC, id DESC LIMIT ?4 OFFSET ?5"
        } else {
            "SELECT date(end_time, 'localtime'), end_time, CAST(score AS REAL),
                    'score', source_scope FROM sleep_sessions_shown
             WHERE ?1 = 'sleep_score' AND score IS NOT NULL
               AND (?2 IS NULL OR date(end_time, 'localtime') >= ?2)
               AND (?3 IS NULL OR date(end_time, 'localtime') <= ?3)
             ORDER BY end_time DESC, id DESC LIMIT ?4 OFFSET ?5"
        };
        let mut stmt = self.conn.prepare(query)?;
        let rows = stmt.query_map(params![metric, start, end, limit, offset], |row| {
            Ok(StoredMetricRecord {
                date: row.get(0)?,
                timestamp: row.get(1)?,
                value: row.get(2)?,
                unit: row.get(3)?,
                source_scope: row.get(4)?,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    /// One point per calendar day from `daily_metrics`.
    ///
    /// Where the same day is reported twice — once by the account's own fused
    /// roll-up, once by the watch — the fused reading wins, the same
    /// precedence the export uses, so a chart and an export never disagree.
    ///
    /// `pub(crate)`: the ai_tasks per-day export reuses it to fetch the P4 window, so there is no second copy
    /// of the day-folding rule.
    pub(crate) fn daily_metric_points(
        &self,
        metric: &str,
        spread: Option<(&str, &str)>,
        start: &str,
        end: &str,
    ) -> Result<Vec<MetricSeriesPoint>> {
        let pick = |metric: &str| -> Result<BTreeMap<String, f64>> {
            let mut stmt = self.conn.prepare(
                "SELECT date,
                        COALESCE(
                            MAX(CASE WHEN source_scope = 'user_fused' THEN value END),
                            MAX(value)
                        )
                 FROM daily_metrics
                 WHERE metric = ?1 AND date BETWEEN ?2 AND ?3
                 GROUP BY date ORDER BY date",
            )?;
            let rows = stmt.query_map(params![metric, start, end], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
            })?;
            let mut map = BTreeMap::new();
            for row in rows {
                let (date, value) = row?;
                map.insert(date, value);
            }
            Ok(map)
        };

        let values = pick(metric)?;
        let (minima, maxima) = match spread {
            Some((low, high)) => (pick(low)?, pick(high)?),
            None => (BTreeMap::new(), BTreeMap::new()),
        };

        Ok(values
            .into_iter()
            .map(|(date, value)| MetricSeriesPoint {
                min: minima.get(&date).copied().map(round1),
                max: maxima.get(&date).copied().map(round1),
                samples: None,
                value: round1(value),
                date,
            })
            .collect())
    }

    /// One point per local day from `metric_samples`.
    ///
    /// The day's value is the mean of its readings and the spread is the
    /// readings' own minimum and maximum — measured, not modelled. A day with
    /// one reading reports no spread rather than a zero-width one.
    ///
    /// `pub(crate)`: like `daily_metric_points`, reused by ai_tasks to fetch by window.
    pub(crate) fn sample_metric_points(
        &self,
        metric: &str,
        start: &str,
        end: &str,
    ) -> Result<Vec<MetricSeriesPoint>> {
        let bounds = local_day_range_utc_bounds(start, end);
        let (lower, upper) = match &bounds {
            Some((lower, upper)) => (Some(lower.as_str()), Some(upper.as_str())),
            None => (None, None),
        };
        let mut stmt = self.conn.prepare(
            "SELECT date(timestamp, 'localtime') AS day,
                    AVG(value), MIN(value), MAX(value), COUNT(*)
             FROM metric_samples
             WHERE metric = ?1
               AND (?4 IS NULL OR timestamp >= ?4)
               AND (?5 IS NULL OR timestamp < ?5)
               AND date(timestamp, 'localtime') BETWEEN ?2 AND ?3
             GROUP BY day ORDER BY day",
        )?;
        let rows = stmt.query_map(params![metric, start, end, lower, upper], |row| {
            Ok(MetricSeriesPoint {
                date: row.get(0)?,
                value: round1(row.get::<_, f64>(1)?),
                min: Some(round1(row.get::<_, f64>(2)?)),
                max: Some(round1(row.get::<_, f64>(3)?)),
                samples: Some(row.get::<_, i64>(4)?),
            })
        })?;
        Ok(rows
            .collect::<std::result::Result<Vec<_>, _>>()?
            .into_iter()
            .map(|mut point| {
                if point.samples == Some(1) {
                    point.min = None;
                    point.max = None;
                }
                point
            })
            .collect())
    }

    /// Acute (7 day) against chronic (28 day) training load.
    ///
    /// The chronic window reaches 27 days before the range so the first day
    /// asked for is already backed by a full window instead of ramping up from
    /// zero. Shared with the export so the screen and the file agree.
    /// Zepp's daily training_load is already a rolling seven-day total; only
    /// individual workout loads may be summed here. Only fully fetched and
    /// persisted workout lists establish coverage; missing loads invalidate a day.
    /// Incomplete windows remain null, including windows with no evidence.
    pub fn training_load_balance(
        &self,
        start: NaiveDate,
        end: NaiveDate,
    ) -> Result<Vec<TrainingBalancePoint>> {
        let history_start = (start - Duration::days(27)).format("%Y-%m-%d").to_string();
        let end_text = end.format("%Y-%m-%d").to_string();
        let mut stmt = self.conn.prepare(
            "SELECT date(start_time, 'localtime') AS day, SUM(training_load),
                    SUM(CASE WHEN training_load IS NULL OR training_load < 0 THEN 1 ELSE 0 END)
             FROM workouts_visible WHERE date(start_time, 'localtime') BETWEEN ?1 AND ?2
             GROUP BY day ORDER BY day",
        )?;
        let rows = stmt.query_map(params![history_start, end_text], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<f64>>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })?;
        let mut coverage = self.complete_workout_days(&history_start, &end_text)?;
        let mut by_date: BTreeMap<String, f64> = BTreeMap::new();
        for row in rows {
            let (date, value, missing) = row?;
            if missing > 0 || !value.is_some_and(|v| v.is_finite()) {
                coverage.remove(&date);
            } else if let Some(value) = value {
                by_date.insert(date, value);
            }
        }

        let mut balance = Vec::new();
        let mut day = start;
        while day <= end {
            let window_sum = |days: i64| -> (f64, i64) {
                let mut total = 0.0;
                let mut present = 0i64;
                for back in 0..days {
                    let key = (day - Duration::days(back)).format("%Y-%m-%d").to_string();
                    if let Some(value) = by_date.get(&key) {
                        total += *value;
                    }
                    if coverage.contains(&key) {
                        present += 1;
                    }
                }
                (total, present)
            };
            let (acute, acute_days) = window_sum(7);
            let (chronic, chronic_days) = window_sum(28);
            let chronic_weekly = chronic / 4.0;
            let ratio = (acute_days == 7 && chronic_days == 28 && chronic_weekly > 0.0)
                .then(|| (acute / chronic_weekly * 100.0).round() / 100.0);
            balance.push(TrainingBalancePoint {
                date: day.format("%Y-%m-%d").to_string(),
                acute_7d: (acute_days == 7).then(|| round1(acute)),
                acute_days_with_data: acute_days,
                chronic_28d: (chronic_days == 28).then(|| round1(chronic)),
                chronic_days_with_data: chronic_days,
                acute_chronic_ratio: ratio,
            });
            day += Duration::days(1);
        }
        Ok(balance)
    }

    pub(super) fn latest_metric_f64(&self, metric: &str) -> Result<Option<f64>> {
        self.conn
            .query_row(
                "SELECT value FROM metric_samples WHERE metric = ?1
                 ORDER BY timestamp DESC,
                    CASE source_scope WHEN 'user_fused' THEN 0 WHEN 'device' THEN 1 ELSE 2 END,
                    id DESC LIMIT 1",
                [metric],
                |row| row.get(0),
            )
            .optional()
            .map_err(Into::into)
    }

    /// When a day has several rows, the pick is exactly the same as `daily_metric_points` (charts, the per-day table sent to AI):
    /// the account-merged value first, otherwise the largest row. Before, this took "the latest inserted device row", so people wearing
    /// two watches saw different numbers on the home card, the chart and the activity rings (feedback: "steps do not match").
    pub(super) fn latest_daily_f64(&self, metric: &str) -> Result<Option<f64>> {
        self.conn
            .query_row(
                "SELECT value FROM daily_metrics WHERE metric = ?1
                 ORDER BY date DESC, (source_scope = 'user_fused') DESC, value DESC, id DESC
                 LIMIT 1",
                [metric],
                |row| row.get(0),
            )
            .optional()
            .map_err(Into::into)
    }

    pub(super) fn latest_daily_i32(&self, metric: &str) -> Result<Option<i32>> {
        Ok(self
            .latest_daily_f64(metric)?
            .map(|value| value.round() as i32))
    }

    pub(super) fn latest_daily_i32_for_date(
        &self,
        metric: &str,
        date: NaiveDate,
    ) -> Result<Option<i32>> {
        self.conn
            .query_row(
                "SELECT value FROM daily_metrics WHERE metric = ?1 AND date = ?2
                 ORDER BY (source_scope = 'user_fused') DESC, value DESC, id DESC LIMIT 1",
                params![metric, date.format("%Y-%m-%d").to_string()],
                |row| row.get::<_, f64>(0),
            )
            .optional()
            .map(|value| value.map(|value| value.round() as i32))
            .map_err(Into::into)
    }
}

/// The max, min, sum and count of one day's heart rate samples.
struct DayAccumulator {
    max: f64,
    min: f64,
    sum: f64,
    count: i64,
}

impl Default for DayAccumulator {
    fn default() -> Self {
        Self {
            max: f64::NEG_INFINITY,
            min: f64::INFINITY,
            sum: 0.0,
            count: 0,
        }
    }
}

impl DayAccumulator {
    fn add(&mut self, value: f64) {
        self.max = self.max.max(value);
        self.min = self.min.min(value);
        self.sum += value;
        self.count += 1;
    }
}

/// The canonical sample time format: UTC, to the second, ending in `+00:00` (length 25). Only this format sorts by time as a string.
const CANONICAL_UTC_FORMAT: &str = "%Y-%m-%dT%H:%M:%S+00:00";

fn canonical_utc(value: &str) -> bool {
    value.len() == 25 && value.ends_with("+00:00") && value.as_bytes()[10] == b'T'
}

/// The UTC instant where each local day in the window starts, in the same string format as sample times.
///
/// `starts[i]` is the start of day i (counting from `first`), plus one extra: the start of the day after the window ends.
pub(super) struct LocalDayBuckets {
    first: NaiveDate,
    starts: Vec<String>,
}

impl LocalDayBuckets {
    pub(super) fn new(first: NaiveDate, last: NaiveDate) -> Self {
        let days = (last - first).num_days().max(0) + 2;
        let starts = (0..days)
            .map(|offset| {
                local_day_start_utc(first + Duration::days(offset))
                    .format(CANONICAL_UTC_FORMAT)
                    .to_string()
            })
            .collect();
        Self { first, starts }
    }

    /// Number of days in the window.
    pub(super) fn len(&self) -> usize {
        self.starts.len() - 1
    }

    /// The date of day `index` (`YYYY-MM-DD`).
    pub(super) fn date(&self, index: usize) -> String {
        (self.first + Duration::days(index as i64))
            .format("%Y-%m-%d")
            .to_string()
    }

    /// Which day of the window this instant falls on. None when outside the window or not recognized.
    /// `cursor` is the day of the previous row: rows come in ascending time order, so the pointer only moves forward.
    pub(super) fn day_of(&self, timestamp: &str, cursor: &mut usize) -> Option<usize> {
        let days = self.len();
        if !canonical_utc(timestamp) {
            let day = super::ai_export::local_day_of_stored(timestamp)?;
            let date = NaiveDate::parse_from_str(&day, "%Y-%m-%d").ok()?;
            let index = usize::try_from((date - self.first).num_days()).ok()?;
            return (index < days).then_some(index);
        }
        if timestamp < self.starts[0].as_str() || timestamp >= self.starts[days].as_str() {
            return None;
        }
        while *cursor + 1 < days && timestamp >= self.starts[*cursor + 1].as_str() {
            *cursor += 1;
        }
        // If rows are not ascending (it should not happen): the pointer does not move back, use binary search instead.
        Some(if timestamp >= self.starts[*cursor].as_str() {
            *cursor
        } else {
            self.starts
                .partition_point(|start| start.as_str() <= timestamp)
                - 1
        })
    }
}

/// The first instant (UTC) of local day `day`. When midnight falls in a DST skip, use the first instant after the skip.
/// When midnight occurs twice, use the earlier one.
fn local_day_start_utc(day: NaiveDate) -> DateTime<Utc> {
    use chrono::{LocalResult, TimeZone};
    let midnight = day.and_hms_opt(0, 0, 0).expect("midnight is always valid");
    for minutes in 0..=24 * 60 {
        match Local.from_local_datetime(&(midnight + Duration::minutes(minutes))) {
            LocalResult::Single(at) | LocalResult::Ambiguous(at, _) => {
                return at.with_timezone(&Utc)
            }
            LocalResult::None => continue,
        }
    }
    midnight.and_utc()
}
