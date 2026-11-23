//! Local weekly report: collect samples per day and compare them with the previous period.

use super::*;

impl Database {
    /// Local weekly report: the last 7 days against the **personal** baseline of the 28 days before.
    ///
    /// Each finding carries the sample count, the source and the confidence. When evidence is thin, it says so. It does not compare with any population norm,
    /// and it gives no diagnosis, treatment or risk prediction.
    pub fn weekly_report(&self, now: DateTime<Local>) -> Result<WeeklyReport> {
        // `daily_metrics.date` is a local day, so "today" must also be a local day.
        self.weekly_report_for_day(now.date_naive(), now.with_timezone(&Utc))
    }

    /// Same as above, but the caller gives "today". Tests use it to pin the date, independent of the machine time zone.
    pub fn weekly_report_for_day(
        &self,
        today: NaiveDate,
        generated_at: DateTime<Utc>,
    ) -> Result<WeeklyReport> {
        let recent_start = saturating_days_before(today, weekly::RECENT_DAYS - 1);
        let baseline_end = saturating_days_before(recent_start, 1);
        let baseline_start = saturating_days_before(baseline_end, weekly::BASELINE_DAYS - 1);

        let mut facts = Vec::new();
        for (fact_id, metric, unit) in [
            ("weekly.resting_hr", "resting_hr", "bpm"),
            ("weekly.hrv", "hrv", "ms"),
            ("weekly.stress", "stress", "score"),
            ("weekly.sleep_duration", "sleep_duration", "min"),
            (
                "weekly.sleep_start_regularity",
                "sleep_start_regularity",
                "min",
            ),
            ("weekly.workout_count", "workout_count", "workouts"),
            ("weekly.training_load", "training_load", "load"),
        ] {
            facts.push(self.weekly_fact(
                fact_id,
                metric,
                unit,
                recent_start,
                today,
                baseline_start,
                baseline_end,
            )?);
        }

        Ok(WeeklyReport {
            generated_at: generated_at.to_rfc3339(),
            recent_start: recent_start.to_string(),
            recent_end: today.to_string(),
            baseline_start: baseline_start.to_string(),
            baseline_end: baseline_end.to_string(),
            facts,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn weekly_fact(
        &self,
        fact_id: &str,
        metric: &str,
        unit: &str,
        recent_start: NaiveDate,
        recent_end: NaiveDate,
        baseline_start: NaiveDate,
        baseline_end: NaiveDate,
    ) -> Result<InsightFact> {
        let recent = self.weekly_samples(metric, recent_start, recent_end)?;
        let baseline = self.weekly_samples(metric, baseline_start, baseline_end)?;
        let window = BaselineWindow {
            kind: "previous_days".into(),
            days: weekly::BASELINE_DAYS,
            min_samples: weekly::MIN_BASELINE_DAYS,
            max_samples: weekly::BASELINE_DAYS,
            distance_tolerance_percent: None,
        };

        let value = mean(&recent.values);
        let source = recent
            .source
            .clone()
            .or_else(|| baseline.source.clone())
            .unwrap_or_else(|| "unknown".into());

        let enough_baseline = baseline.day_count as i64 >= weekly::MIN_BASELINE_DAYS;
        let baseline_count = baseline.day_count as i64;
        let (comparison, confidence, reason, reason_code) = match (value, mean(&baseline.values)) {
            (Some(current), Some(previous)) if enough_baseline && previous != 0.0 => {
                let delta = current - previous;
                (
                    Some(Comparison {
                        baseline_value: round1(previous),
                        delta: round1(delta),
                        delta_percent: round1(delta / previous.abs() * 100.0),
                        direction: direction_of(delta, previous),
                    }),
                    Confidence::from_samples(baseline.day_count),
                    None,
                    None,
                )
            }
            // The baseline has enough days but its mean is 0. A relative change with 0 as the divisor is not possible.
            // This is not the same as "too few samples", so it has its own code and does not reuse thin_baseline.
            (Some(_), Some(previous)) if enough_baseline && previous == 0.0 => (
                None,
                Confidence::Insufficient,
                Some("The previous baseline mean is 0, so a relative change cannot be calculated.".into()),
                Some("weekly_zero_baseline".to_string()),
            ),
            (Some(_), _) => (
                None,
                Confidence::Insufficient,
                Some(format!(
                    "In the previous {} days, only {} days have this data, fewer than {} days, so only the current value is reported, with no comparison.",
                    weekly::BASELINE_DAYS,
                    baseline.day_count,
                    weekly::MIN_BASELINE_DAYS
                )),
                Some("weekly_thin_baseline".to_string()),
            ),
            (None, _) => (
                None,
                Confidence::Insufficient,
                Some("This device has no data for this item in the last 7 days.".into()),
                Some("weekly_no_recent_data".to_string()),
            ),
        };

        Ok(InsightFact {
            fact_id: fact_id.into(),
            metric: metric.into(),
            value: value.map(round1),
            unit: unit.into(),
            comparison,
            baseline_window: Some(window),
            evidence_count: recent.day_count as i64,
            source,
            confidence,
            reason,
            reason_code,
            baseline_count,
            evidence_refs: recent.dates,
        })
    }

    pub(super) fn weekly_samples(
        &self,
        metric: &str,
        start: NaiveDate,
        end: NaiveDate,
    ) -> Result<WeeklySamples> {
        let start_text = start.to_string();
        let end_text = end.to_string();
        match metric {
            "sleep_duration" => self.collect_samples(
                "SELECT date(end_time, 'localtime'), CAST(duration_minutes AS REAL), source_scope
                 FROM sleep_sessions_shown
                 WHERE date(end_time, 'localtime') BETWEEN ?1 AND ?2",
                &start_text,
                &end_text,
            ),
            // Sleep start regularity: the sleep start minute of each night (from local midnight) is itself
            // a comparable daily value. Each window takes its standard deviation for the comparison.
            "sleep_start_regularity" => {
                let mut raw = self.collect_raw_samples(
                    "SELECT date(start_time, 'localtime'),
                            CAST(strftime('%H', start_time, 'localtime') AS REAL) * 60
                              + CAST(strftime('%M', start_time, 'localtime') AS REAL),
                            source_scope
                     FROM sleep_sessions_shown
                     WHERE date(start_time, 'localtime') BETWEEN ?1 AND ?2",
                    &start_text,
                    &end_text,
                )?;
                // A sleep start across midnight jumps between 0 and 1440. A direct standard deviation sees
                // "23:50 and 00:10" as 23 hours apart. So all values shift to minutes relative to 18:00.
                // Shift first, then average per day: the raw average of 23:50 and 00:10 on one day is noon;
                // after the shift it is near midnight.
                for (_, minutes) in &mut raw.rows {
                    let shifted = *minutes - 18.0 * 60.0;
                    *minutes = if shifted < -12.0 * 60.0 {
                        shifted + 24.0 * 60.0
                    } else {
                        shifted
                    };
                }
                let samples = collapse_per_day(raw);
                let spread = stdev(&samples.values);
                Ok(WeeklySamples {
                    values: spread.map(|value| vec![value]).unwrap_or_default(),
                    day_count: samples.day_count,
                    dates: samples.dates,
                    source: samples.source,
                })
            }
            "workout_count" => {
                let raw = self.collect_raw_samples(
                    "SELECT date(start_time, 'localtime'), 1.0, source_scope
                     FROM workouts_visible
                     WHERE date(start_time, 'localtime') BETWEEN ?1 AND ?2",
                    &start_text,
                    &end_text,
                )?;
                // "Count" is the total in the window, not a daily average. It also cannot count values after
                // the per-day fold (two workouts on one day become 1.0). So record the total first, and
                // fold only to get the distinct days.
                //
                // The baseline window (28 days) is longer than the current window (7 days), so the totals cannot
                // be compared directly. Otherwise the same training frequency reads as "dropped". Scale by the
                // actual window days to an equivalent count "per RECENT_DAYS days". The current window
                // is exactly RECENT_DAYS days, so its value does not change.
                let window_days = (end - start).num_days() + 1;
                let total = raw.rows.len() as f64;
                let scaled_total = if window_days > 0 {
                    total * weekly::RECENT_DAYS as f64 / window_days as f64
                } else {
                    total
                };
                let samples = collapse_per_day(raw);
                Ok(WeeklySamples {
                    values: if total > 0.0 {
                        vec![scaled_total]
                    } else {
                        Vec::new()
                    },
                    day_count: samples.day_count,
                    dates: samples.dates,
                    source: samples.source,
                })
            }
            "training_load" => self.collect_samples(
                "SELECT date, value, source_scope FROM daily_metrics
                 WHERE metric = 'training_load' AND date BETWEEN ?1 AND ?2",
                &start_text,
                &end_text,
            ),
            // HRV is stored under two names: the `hrv` stream writes `hrv`, and the wellness
            // `hrvRmssd` item writes `hrv_rmssd`. Query both. Missing one means
            // those days look like they have no HRV data.
            "hrv" => {
                let daily = self.collect_samples(
                    "SELECT date, value, source_scope FROM daily_metrics
                     WHERE metric IN ('hrv', 'hrv_rmssd') AND date BETWEEN ?1 AND ?2",
                    &start_text,
                    &end_text,
                )?;
                if !daily.values.is_empty() {
                    return Ok(daily);
                }
                self.collect_samples(
                    "SELECT date(timestamp, 'localtime'), value, source_scope FROM metric_samples
                     WHERE metric IN ('hrv', 'hrv_rmssd')
                       AND timestamp >= ?3 AND timestamp < ?4
                       AND date(timestamp, 'localtime') BETWEEN ?1 AND ?2",
                    &start_text,
                    &end_text,
                )
            }
            other => {
                // Check the daily table first, then fall back to the sample table.
                let daily = self.collect_samples(
                    &format!(
                        "SELECT date, value, source_scope FROM daily_metrics
                         WHERE metric = '{other}' AND date BETWEEN ?1 AND ?2"
                    ),
                    &start_text,
                    &end_text,
                )?;
                if !daily.values.is_empty() {
                    return Ok(daily);
                }
                self.collect_samples(
                    &format!(
                        "SELECT date(timestamp, 'localtime'), value, source_scope FROM metric_samples
                         WHERE metric = '{other}' AND timestamp >= ?3 AND timestamp < ?4
                           AND date(timestamp, 'localtime') BETWEEN ?1 AND ?2"
                    ),
                    &start_text,
                    &end_text,
                )
            }
        }
    }

    pub(super) fn collect_samples(
        &self,
        sql: &str,
        start: &str,
        end: &str,
    ) -> Result<WeeklySamples> {
        Ok(collapse_per_day(self.collect_raw_samples(sql, start, end)?))
    }

    pub(super) fn collect_raw_samples(
        &self,
        sql: &str,
        start: &str,
        end: &str,
    ) -> Result<RawWeeklySamples> {
        let mut stmt = self.conn.prepare(sql)?;
        // The sample-table query adds a pair of UTC time bounds (?3 / ?4). `date(timestamp, 'localtime')`
        // on the column cannot use the index and scans all rows of the metric. The time bounds first
        // narrow the range on the index, then date() filters exactly to the local day. The result is the same.
        let (lower, upper) = crate::storage::local_day_range_utc_bounds(start, end)
            .unwrap_or_else(|| ("0000".to_string(), "9999".to_string()));
        let mapper = |row: &rusqlite::Row<'_>| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, f64>(1)?,
                row.get::<_, String>(2)?,
            ))
        };
        let rows: Vec<_> = if sql.contains("?3") {
            stmt.query_map(rusqlite::params![start, end, lower, upper], mapper)?
                .collect()
        } else {
            stmt.query_map(rusqlite::params![start, end], mapper)?
                .collect()
        };
        let mut samples = Vec::new();
        let mut scopes = std::collections::BTreeSet::new();
        for row in rows {
            let (date, value, scope) = row?;
            if !value.is_finite() {
                continue;
            }
            samples.push((date, value));
            scopes.insert(scope);
        }
        Ok(RawWeeklySamples {
            rows: samples,
            scopes,
        })
    }
}

pub(super) struct RawWeeklySamples {
    pub(super) rows: Vec<(String, f64)>,
    pub(super) scopes: std::collections::BTreeSet<String>,
}

/// One finite value per calendar day. Rows on the same day (two scopes, hrv + hrv_rmssd, several
/// samples a day) are averaged per day first, then these daily values make the window mean. Otherwise two rows
/// on one day skew the mean and count "one day of evidence" as two days.
pub(super) fn collapse_per_day(raw: RawWeeklySamples) -> WeeklySamples {
    let mut by_date: std::collections::BTreeMap<String, Vec<f64>> =
        std::collections::BTreeMap::new();
    for (date, value) in raw.rows {
        by_date.entry(date).or_default().push(value);
    }
    let mut values = Vec::with_capacity(by_date.len());
    let mut dates = Vec::with_capacity(by_date.len());
    for (date, day_values) in by_date {
        values.push(day_values.iter().sum::<f64>() / day_values.len() as f64);
        dates.push(date);
    }
    let day_count = dates.len();
    WeeklySamples {
        values,
        day_count,
        dates,
        // When a window mixes several sources, do not pick one. Say mixed.
        source: match raw.scopes.len() {
            0 => None,
            1 => raw.scopes.into_iter().next(),
            _ => Some("mixed".into()),
        },
    }
}

pub(super) struct WeeklySamples {
    /// One value per calendar day with data (rows on the same day are already averaged).
    /// `sleep_start_regularity` / `workout_count` aggregate it again into one value.
    pub(super) values: Vec<f64>,
    /// The number of distinct dates with data in the window. Day counts must not count values.
    pub(super) day_count: usize,
    pub(super) dates: Vec<String>,
    pub(super) source: Option<String>,
}
