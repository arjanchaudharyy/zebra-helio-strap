//! AI handoff export: shared types, device labels and helper queries (split from storage/mod.rs by domain, same logic).

mod build;

use super::*;

/// Metrics dense enough that a month of them dwarfs everything else in an
/// export. In `Summary` detail these collapse to one row per hour; sparse
/// streams such as HRV keep their exact sample times, which is the whole point
/// of measuring them.
pub(in crate::storage) const HOURLY_AGGREGATED_METRICS: [&str; 3] =
    ["heart_rate", "spo2", "stress"];

/// Day-level export types: a single workout has no fields like "how long you slept last night" or "steps that day",
/// so these types must not appear in a single-workout scope. `build_ai_export` / `estimate_ai_export`
/// and the sample metric name expansion share this one table. Versions with a separate copy in each function drifted apart.
pub(in crate::storage) const EXPORT_DAY_LEVEL_TYPES: [&str; 11] = [
    "blood_pressure",
    "sleep",
    "steps",
    "daily_activity",
    "recovery",
    "training_load",
    "vo2max",
    "lactate_threshold",
    "pai",
    "weight",
    "food",
];

/// Export types whose raw payloads are fetched but whose field-by-field
/// normalization has not been verified against a real response yet.
///
/// These need their own status. Reporting `empty_in_range` would say "the
/// stream is wired, you simply have no data", and for these that is false —
/// the data is on disk as a retained raw response, only the parse is pending.
/// Each entry maps an export type to the `wellness` source-key labels that
/// carry its raw payloads.
pub(in crate::storage) const RAW_PENDING_STREAMS: [(&str, &[&str]); 7] = [
    ("blood_pressure", &["blood_pressure"]),
    ("spo2", &["spo2", "spo2_auto", "spo2_odi"]),
    ("stress", &["stress"]),
    ("respiratory_rate", &["respiratory_rate"]),
    ("hrv_rmssd", &["hrv_rmssd"]),
    ("pai", &["pai"]),
    ("lactate_threshold", &["lactate_threshold"]),
];

#[derive(Debug, Clone, Default)]
pub(in crate::storage) struct ExportDeviceProfile {
    pub(in crate::storage) model: Option<String>,
    pub(in crate::storage) kind: Option<String>,
}

/// Per-export device aliasing. Labels are positional (`device_1`, `device_2`)
/// and carry no identifying information, so they survive the AI-handoff
/// redaction pass that strips serials and device ids.
#[derive(Debug, Default)]
pub(in crate::storage) struct ExportDevices {
    pub(in crate::storage) label_by_alias: BTreeMap<String, String>,
    pub(in crate::storage) profiles: BTreeMap<String, ExportDeviceProfile>,
}

impl ExportDevices {
    pub(in crate::storage) fn label(&self, device_id: Option<&str>) -> Option<String> {
        device_id.and_then(|alias| self.label_by_alias.get(alias).cloned())
    }
}

/// One hour of a dense metric, reduced to the shape a reader can actually use.
#[derive(Debug)]
pub(in crate::storage) struct HourBucket {
    pub(in crate::storage) selected_type: String,
    pub(in crate::storage) unit: String,
    pub(in crate::storage) source_scope: String,
    pub(in crate::storage) device_label: Option<String>,
    pub(in crate::storage) min: f64,
    pub(in crate::storage) max: f64,
    pub(in crate::storage) sum: f64,
    pub(in crate::storage) count: usize,
}

impl HourBucket {
    pub(in crate::storage) fn new(
        selected_type: String,
        unit: String,
        source_scope: String,
        device_label: Option<String>,
    ) -> Self {
        Self {
            selected_type,
            unit,
            source_scope,
            device_label,
            min: f64::INFINITY,
            max: f64::NEG_INFINITY,
            sum: 0.0,
            count: 0,
        }
    }

    pub(in crate::storage) fn push(&mut self, value: f64) {
        self.min = self.min.min(value);
        self.max = self.max.max(value);
        self.sum += value;
        self.count += 1;
    }

    pub(in crate::storage) fn render(&self, metric: &str, hour: &str) -> serde_json::Value {
        let average = if self.count == 0 {
            None
        } else {
            Some((self.sum / self.count as f64 * 10.0).round() / 10.0)
        };
        serde_json::json!({
            "metric": metric,
            "hour": hour,
            "min": self.count.gt(&0).then_some(self.min),
            "avg": average,
            "max": self.count.gt(&0).then_some(self.max),
            "samples": self.count,
            "unit": self.unit,
            "source_scope": self.source_scope,
            "device_label": self.device_label,
        })
    }
}

/// All readings of one `(date, metric)` pair across sources.
///
/// Since account-level aggregates stopped being mislabelled as device data,
/// the same day's step count can arrive twice: once fused, once from the watch
/// that measured it. Picking one silently would hide a disagreement, so the
/// fused reading leads and anything that differs is kept beside it.
#[derive(Debug)]
pub(in crate::storage) struct DailyMetricGroup {
    pub(in crate::storage) date: String,
    pub(in crate::storage) metric: String,
    pub(in crate::storage) selected_type: String,
    pub(in crate::storage) readings: Vec<(f64, String, String, Option<String>)>,
}

impl DailyMetricGroup {
    pub(in crate::storage) fn new(date: String, metric: String, selected_type: &str) -> Self {
        Self {
            date,
            metric,
            selected_type: selected_type.to_string(),
            readings: Vec::new(),
        }
    }

    pub(in crate::storage) fn push(
        &mut self,
        value: f64,
        unit: String,
        source_scope: String,
        device_label: Option<String>,
    ) {
        self.readings
            .push((value, unit, source_scope, device_label));
    }

    pub(in crate::storage) fn render(&self) -> serde_json::Value {
        // user_fused is the account's own reconciliation of its devices, so it
        // leads when present; otherwise the first reading in query order does.
        let primary_index = self
            .readings
            .iter()
            .position(|(_, _, scope, _)| scope == "user_fused")
            .unwrap_or(0);
        let Some((value, unit, source_scope, device_label)) = self.readings.get(primary_index)
        else {
            return serde_json::Value::Null;
        };
        let alternates = self
            .readings
            .iter()
            .enumerate()
            .filter(|(index, (other, _, _, _))| {
                *index != primary_index && (other - value).abs() > f64::EPSILON
            })
            .map(|(_, (other, _, scope, label))| {
                serde_json::json!({
                    "value": other,
                    "source_scope": scope,
                    "device_label": label,
                })
            })
            .collect::<Vec<_>>();
        let mut record = serde_json::json!({
            "date": self.date,
            "metric": self.metric,
            "value": value,
            "unit": unit,
            "source_scope": source_scope,
            "device_label": device_label,
        });
        if !alternates.is_empty() {
            if let Some(object) = record.as_object_mut() {
                object.insert("alternates".into(), serde_json::Value::Array(alternates));
            }
        }
        record
    }
}

/// Convert a "local date range" into a pair of UTC timestamp bounds that compare directly as strings.
///
/// `metric_samples.timestamp` stores RFC3339 UTC (always ending in `+00:00`), so
/// lexical order is time order. The problem is the filter: `date(timestamp,'localtime')` is a function
/// call, and SQLite cannot use it for a range lookup in the index. It must scan **all** samples of the metric.
/// A year of heart rate is more than 200,000 rows, only to pick seven days.
///
/// A loose timestamp bound lets the index first narrow the range to a few days (measured: heart rate, 7 days, 92 ms to 5 ms).
/// Each bound is one day wider, to cover any time zone offset (-12..+14 hours). So it only "scans less"
/// and does not change the result: the later `date(...,'localtime')` still decides which day each row belongs to.
///
/// `pub(crate)`: the per-day window queries in ai_tasks need the same loose bounds to use the time index.
pub(crate) fn local_day_range_utc_bounds(start: &str, end: &str) -> Option<(String, String)> {
    let start = NaiveDate::parse_from_str(start, "%Y-%m-%d").ok()?;
    let end = NaiveDate::parse_from_str(end, "%Y-%m-%d").ok()?;
    // Use checked add and subtract: for dates near NaiveDate::MIN / MAX, return no bound and let the caller handle it,
    // instead of a panic here.
    let lower = start
        .checked_sub_signed(Duration::days(1))?
        .format("%Y-%m-%dT00:00:00")
        .to_string();
    let upper = end
        .checked_add_signed(Duration::days(2))?
        .format("%Y-%m-%dT00:00:00")
        .to_string();
    Some((lower, upper))
}

/// Caller fallback for `local_day_range_utc_bounds`: the dates here were already validated as
/// NaiveDate, so a missing bound can only be an internal error. Even then the query must not
/// shrink to an empty set. Fall back to bounds that block nothing, and the exact predicate still filters.
/// The cost is only a full table scan, not silently missing data.
pub(in crate::storage) fn utc_bounds_or_unbounded(start: &str, end: &str) -> (String, String) {
    local_day_range_utc_bounds(start, end)
        .unwrap_or_else(|| ("0000-01-01".to_string(), "9999-12-31".to_string()))
}

/// Stored timestamp to local calendar day, with the same rule as SQLite `date(x, 'localtime')`.
/// Rows that cannot be parsed return None. Before, date() turned such rows into NULL, and they did not take part in
/// MIN/MAX. This has the same meaning.
pub(in crate::storage) fn local_day_of_stored(value: &str) -> Option<String> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|dt| dt.with_timezone(&Local).format("%Y-%m-%d").to_string())
}

/// Selected export type to the type that a `metric_samples` row belongs to.
///
/// Types are the user's words ("spo2"). Metric names in the database are the watch's words ("spo2_apnea_low").
/// This mapping works in two directions: it assigns a type to each sample at export, and it expands selected types
/// into `metric IN (...)`, so the sample query uses the (metric, timestamp) index.
pub(in crate::storage) fn export_sample_matched_type(
    metric: &str,
    selected: &BTreeSet<String>,
) -> Option<String> {
    if selected.contains(metric) {
        Some(metric.to_string())
    } else if metric.contains("spo2") && selected.contains("spo2") {
        Some("spo2".to_string())
    } else if metric.contains("stress") && selected.contains("stress") {
        Some("stress".to_string())
    } else if metric.starts_with("respiratory") && selected.contains("respiratory_rate") {
        Some("respiratory_rate".to_string())
    } else if metric == "hrv_rmssd" && selected.contains("hrv_rmssd") {
        Some("hrv_rmssd".to_string())
    } else if metric == "steps_hourly" && selected.contains("steps") {
        Some("steps".to_string())
    } else if selected.contains("weight") && BODY_COMPOSITION_METRICS.contains(&metric) {
        Some("weight".to_string())
    } else if metric == "bmr" && selected.contains("daily_activity") {
        // Resting energy (user report): people who selected only daily activity must get it too. Only bmr from a body scale is
        // a real value. The daily metric `calories` in a real library equals `active_calories` every day (checked),
        // and is not the total daily energy. So do not export "total energy", and do not derive resting from total minus active.
        Some("daily_activity".to_string())
    } else if selected.contains("blood_pressure")
        && matches!(
            metric,
            "blood_pressure_systolic" | "blood_pressure_diastolic" | "blood_pressure_pulse"
        )
    {
        Some("blood_pressure".to_string())
    } else {
        None
    }
}

/// The WHERE tail shared by `build_ai_export` / `estimate_ai_export`.
///
/// `metric IN` is the key that makes uq_metric_sample_key (leading column metric) work.
/// Before, WHERE had only time bounds and no metric predicate, so every export was a full table scan. Parameters come
/// after the IN list. A single workout uses its real start and end (a workout can cross local midnight, so the right end
/// uses `<=`). A date range narrows with loose UTC bounds, then refines with the local day predicate.
pub(in crate::storage) fn ai_export_samples_where_tail(
    metric_count: usize,
    single_workout: bool,
) -> String {
    debug_assert!(metric_count > 0);
    let in_list = (1..=metric_count)
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(", ");
    let lower = metric_count + 1;
    let upper = metric_count + 2;
    if single_workout {
        format!("metric IN ({in_list}) AND timestamp >= ?{lower} AND timestamp <= ?{upper}")
    } else {
        let day_start = metric_count + 3;
        let day_end = metric_count + 4;
        format!(
            "metric IN ({in_list})
               AND timestamp >= ?{lower} AND timestamp < ?{upper}
               AND date(timestamp, 'localtime') BETWEEN ?{day_start} AND ?{day_end}"
        )
    }
}

impl Database {
    /// Non-identifying device labels for one export.
    ///
    /// Zepp addresses one physical device by several aliases — the Helio Strap
    /// is `2445B138005129` in band summaries and `D85403FFFEE4D576` in
    /// readiness events — so aliases are folded onto a single label via
    /// `device_identities`. Only the catalog's canonical model name and kind
    /// leave the machine; the serial and the user's nickname for the device
    /// never do.
    pub(in crate::storage) fn export_devices(&self) -> Result<ExportDevices> {
        let mut stmt = self
            .conn
            .prepare("SELECT alias, name, serial, device_id FROM device_identities")?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        })?;
        let mut groups: BTreeMap<String, (BTreeSet<String>, Option<String>)> = BTreeMap::new();
        for row in rows {
            let (alias, name, serial, device_id) = row?;
            // The serial is the stable identity of a physical device: the
            // strap's rows share `2445B138005129` but differ in device_id
            // (`2445B138005129` vs `D85403FFFEE4D576`), so keying on both
            // would report one device twice.
            let key = serial
                .clone()
                .or_else(|| device_id.clone())
                .unwrap_or_else(|| alias.clone());
            let entry = groups.entry(key).or_default();
            entry.0.insert(alias);
            if let Some(serial) = serial {
                entry.0.insert(serial);
            }
            if let Some(device_id) = device_id {
                entry.0.insert(device_id);
            }
            if entry.1.is_none() {
                entry.1 = name;
            }
        }

        let mut devices = ExportDevices::default();
        for (index, (_, (aliases, name))) in groups.into_iter().enumerate() {
            let label = format!("device_{}", index + 1);
            // The stored name is the user's nickname, so it is only ever used
            // to look the product up in the bundled catalog.
            let matched = name.as_deref().and_then(|name| {
                crate::device_catalog::match_catalog(&crate::device_catalog::CatalogMatchInput {
                    device_names: vec![name],
                    display_name: Some(name),
                    ..Default::default()
                })
            });
            devices.profiles.insert(
                label.clone(),
                ExportDeviceProfile {
                    model: matched
                        .as_ref()
                        .map(|found| found.entry.canonical_name.clone()),
                    kind: matched.as_ref().map(|found| found.entry.kind.clone()),
                },
            );
            for alias in aliases {
                devices.label_by_alias.insert(alias, label.clone());
            }
        }
        Ok(devices)
    }

    /// Locally derived analysis that needs no extra network call.
    ///
    /// Everything here is computed from data already on disk and states its own
    /// basis, so a reader can tell a measurement from a derivation.
    pub(in crate::storage) fn export_analysis(
        &self,
        start_text: &str,
        end_text: &str,
        selected: &BTreeSet<String>,
        workout_id: Option<&str>,
    ) -> Result<serde_json::Map<String, serde_json::Value>> {
        let mut analysis = serde_json::Map::new();

        if selected.contains("workouts") {
            if let Some(zones) = self.heart_rate_zone_variants(start_text, end_text, workout_id)? {
                analysis.insert("heart_rate_zones".into(), zones);
            }
        }

        // Training load balance is a 28-day window statistic. A single-workout export is about "this one workout".
        // Adding four weeks of daily load would bring back a range the user did not ask for, so skip it here.
        if workout_id.is_some() {
            return Ok(analysis);
        }

        if selected.contains("training_load") || selected.contains("recovery") {
            // Acute:chronic workload ratio. The chronic window reaches 27 days
            // before the export range, so the first day in range is already
            // backed by a full window instead of ramping up from zero.
            let Some(range_start) = NaiveDate::parse_from_str(start_text, "%Y-%m-%d").ok() else {
                return Ok(analysis);
            };
            let end_date = NaiveDate::parse_from_str(end_text, "%Y-%m-%d")
                .map_err(|_| ZeppBridgeError::ConfigError("export end date is invalid".into()))?;
            // Same computation the training screen shows, so a chart and an
            // exported file can never quote different ratios for one day.
            let balance = self.training_load_balance(range_start, end_date)?;

            if !balance.is_empty() {
                analysis.insert(
                    "training_load_balance".into(),
                    serde_json::json!({
                        "source": "workouts.training_load",
                        "note": "Per-workout load summed by the local date of each workout start. acute = sum of the last 7 days; chronic = sum of the last 28 days; ratio = acute / (chronic / 4). A local day counts as covered only when the workout list is fully synced and every workout on it has a valid load; 0 means no workouts were confirmed. A load is null when its 7- or 28-day window is incomplete; ratio is given only when both windows are complete and chronic load is above 0.",
                        "days": balance,
                    }),
                );
            }
        }

        Ok(analysis)
    }

    /// Expand the selected export types into the metric names that really exist in `metric_samples`.
    ///
    /// `SELECT DISTINCT metric` uses only the leading column of uq_metric_sample_key (a covering
    /// index, with as many rows as metric names). In return, the main query gets `metric IN (...)`. That is
    /// the step that stops exports from scanning all of metric_samples. Type matching and the row loop share
    /// `export_sample_matched_type`. `single_workout` already removes the metric names of day-level types here,
    /// so the row loop does not need to check again.
    pub(in crate::storage) fn export_sample_metric_names(
        &self,
        selected: &BTreeSet<String>,
        single_workout: bool,
    ) -> Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT DISTINCT metric FROM metric_samples ORDER BY metric")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        let mut names = Vec::new();
        for row in rows {
            let metric = row?;
            let Some(matched) = export_sample_matched_type(&metric, selected) else {
                continue;
            };
            if single_workout && EXPORT_DAY_LEVEL_TYPES.contains(&matched.as_str()) {
                continue;
            }
            names.push(metric);
        }
        Ok(names)
    }
}
