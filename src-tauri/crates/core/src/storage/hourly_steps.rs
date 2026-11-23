//! Hourly steps: the legacy channel's per-minute records come first. The official hourly totals fill the gaps.
//!
//! Why the legacy channel comes first: we checked a real database day by day. Legacy `band_data` detail has one record per minute, and the hourly sums
//! **equal the day total on every day** (`summary.stp.ttl`). The official `interval=hourly` returns only
//! a few scattered hours (on 9/29 the official hours sum to 770 steps, the real day total is 1727), and it only goes back one month
//! from the authorization date. That is why the daily activity page showed only about twenty days at 6 months.
//!
//! Legacy payloads already stay in `raw_records` (heart rate is decoded from the same payload). We decode them
//! at read time, with no change to normalization and no replay (same approach as `food_entries`). We accept only the checked format:
//! 1440 minutes per day, 8 bytes per minute, byte 3 is the steps for that minute. We never guess other lengths.

use super::*;
use base64::{engine::general_purpose::STANDARD, Engine as _};

/// Key of legacy detail payloads: `band_data:detail:<first day>:<last day>`.
const LEGACY_KEY_PREFIX: &str = "band_data:detail:";
const MINUTES: usize = 1440;
const BYTES_PER_MINUTE: usize = 8;
const STEP_BYTE: usize = 2;

/// 24 hourly step counts decoded from one legacy day record. Returns None for other formats (wrong length, no date).
pub(super) fn legacy_hourly_steps(item: &serde_json::Value) -> Option<(String, String, [f64; 24])> {
    let object = item.as_object()?;
    let date = object.get("date_time")?.as_str()?;
    NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()?;
    let encoded = object.get("data")?.as_str()?;
    let bytes = STANDARD.decode(encoded.trim()).ok()?;
    if bytes.len() != MINUTES * BYTES_PER_MINUTE {
        return None;
    }
    let mut hours = [0.0; 24];
    for minute in 0..MINUTES {
        hours[minute / 60] += f64::from(bytes[minute * BYTES_PER_MINUTE + STEP_BYTE]);
    }
    let device = object
        .get("device_id")
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .to_string();
    Some((date.to_string(), device, hours))
}

/// First and last day from the key. Returns None for an unexpected format (the caller still decodes, but cannot skip by date).
fn key_span(source_key: &str) -> Option<(NaiveDate, NaiveDate)> {
    let rest = source_key.strip_prefix(LEGACY_KEY_PREFIX)?;
    let (start, end) = rest.split_once(':')?;
    Some((
        NaiveDate::parse_from_str(start, "%Y-%m-%d").ok()?,
        NaiveDate::parse_from_str(end, "%Y-%m-%d").ok()?,
    ))
}

impl Database {
    /// Hourly steps for each day in a local date range (inclusive). When the legacy channel has per-minute records for a day, use them
    /// (with several payloads for one day, take the newest fetch; with several watches on one day, take the hourly maximum, do not add:
    /// the same walk must not count twice). Use the official hourly totals only for days with no legacy data.
    /// Hours with no steps are not in the result. The UI draws them empty, not as 0.
    pub fn hourly_steps(&self, start: &str, end: &str) -> Result<Vec<HourlySteps>> {
        let parse = |value: &str| {
            NaiveDate::parse_from_str(value, "%Y-%m-%d")
                .map_err(|_| ZeppBridgeError::ConfigError("Date must be YYYY-MM-DD".into()))
        };
        let (first, last) = (parse(start)?, parse(end)?);
        if first > last {
            return Err(ZeppBridgeError::ConfigError("Start date is after end date".into()));
        }

        let legacy = self.legacy_hourly_steps(first, last)?;
        let mut by_day: BTreeMap<String, BTreeMap<i64, f64>> = BTreeMap::new();
        for (date, hours) in &legacy {
            let slot = by_day.entry(date.clone()).or_default();
            for (hour, steps) in hours.iter().enumerate() {
                if *steps > 0.0 {
                    slot.insert(hour as i64, *steps);
                }
            }
        }

        let (utc_lower, utc_upper) = utc_bounds_or_unbounded(start, end);
        let mut stmt = self.conn.prepare(
            "SELECT date(timestamp, 'localtime'), CAST(strftime('%H', timestamp, 'localtime') AS INTEGER), MAX(value)
             FROM metric_samples
             WHERE metric = 'steps_hourly' AND timestamp >= ?3 AND timestamp < ?4
               AND date(timestamp, 'localtime') BETWEEN ?1 AND ?2
             GROUP BY 1, 2 ORDER BY 1, 2",
        )?;
        let official = stmt
            .query_map(params![start, end, utc_lower, utc_upper], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, f64>(2)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        for (date, hour, steps) in official {
            if legacy.contains_key(&date) {
                continue;
            }
            by_day.entry(date).or_default().insert(hour, steps);
        }

        Ok(by_day
            .into_iter()
            .flat_map(|(date, hours)| {
                hours.into_iter().map(move |(hour, steps)| HourlySteps {
                    date: date.clone(),
                    hour,
                    steps,
                })
            })
            .collect())
    }

    /// Legacy 24-hour steps for each day (within `[first, last]`). Payloads are read from newest to oldest fetch.
    /// Once a day has data, older payloads do not overwrite it. When a payload covers only days we already have, skip it without decompressing.
    fn legacy_hourly_steps(
        &self,
        first: NaiveDate,
        last: NaiveDate,
    ) -> Result<BTreeMap<String, [f64; 24]>> {
        let mut stmt = self.conn.prepare(
            "SELECT source_key, payload, payload_zip FROM raw_records
             WHERE source_key LIKE 'band_data:detail:%'
             ORDER BY fetched_at DESC, id DESC",
        )?;
        let mut rows = stmt.query([])?;
        let mut days: BTreeMap<String, [f64; 24]> = BTreeMap::new();
        // For the same day and watch, take only the newest. Across watches, take the hourly maximum.
        let mut seen: BTreeSet<(String, String)> = BTreeSet::new();
        while let Some(row) = rows.next()? {
            let key: String = row.get(0)?;
            if let Some((span_start, span_end)) = key_span(&key) {
                if span_end < first || span_start > last {
                    continue;
                }
                let mut day = span_start.max(first);
                let mut covered = true;
                while day <= span_end.min(last) {
                    if !days.contains_key(&day.format("%Y-%m-%d").to_string()) {
                        covered = false;
                        break;
                    }
                    day += Duration::days(1);
                }
                if covered {
                    continue;
                }
            }
            let Ok(payload) = decode_raw_payload(row.get(1)?, row.get(2)?) else {
                continue;
            };
            let Ok(payload) = serde_json::from_str::<serde_json::Value>(&payload) else {
                continue;
            };
            let Some(items) = payload.get("data").and_then(|value| value.as_array()) else {
                continue;
            };
            for item in items {
                let Some((date, device, hours)) = legacy_hourly_steps(item) else {
                    continue;
                };
                let Ok(day) = NaiveDate::parse_from_str(&date, "%Y-%m-%d") else {
                    continue;
                };
                if day < first || day > last || !seen.insert((date.clone(), device)) {
                    continue;
                }
                let slot = days.entry(date).or_insert([0.0; 24]);
                for (hour, steps) in hours.iter().enumerate() {
                    slot[hour] = slot[hour].max(*steps);
                }
            }
        }
        Ok(days)
    }
}
