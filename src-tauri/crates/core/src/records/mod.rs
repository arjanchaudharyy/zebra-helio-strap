//! The single entry point to read records: `query_records(types, from, to, since)`.
//!
//! CLI `export --format json`, the local REST `/v1/records` and the post-sync push all use it,
//! so "which data, units, missing values, source" is defined in one place, and the three outputs do not drift apart.
//!
//! It does **not use its own SQL**: `Database::build_ai_export_value` still builds the records
//! (the AI export and MCP also use it). This only adds three things to the result:
//!
//! 1. A top-level `records_version`, so a consumer can check the contract version;
//! 2. A `source` on each record: `official` (Zepp official open platform) or `advanced` (account connector,
//!    the old channel). When both sides have a reading for the same metric on the same day and they merge into one, it is `mixed`; the
//!    calendar context the user wrote is `user`. The source comes from each table's `provider` column, not a guess;
//! 3. An optional `since`: keep only records whose source payload came from the cloud after that time.
//!
//! The output is a superset of the old export: no old field is missing and no meaning changed, so old CLI callers (the PA widget) still read it.
//!
//! GPS: with `include_gps = false`, the workout `route` is removed. The per-point samples have no coordinates.

use crate::models::error::{Result, ZeppBridgeError};
use crate::models::{ExportDetail, ExportScope, ExportSelection};
use crate::storage::{Database, EXPORT_DATA_TYPES};
use chrono::{DateTime, Duration as ChronoDuration, NaiveDate, Utc};
use serde_json::Value;
use std::collections::HashMap;

#[cfg(test)]
mod tests;

/// The record contract version. Fields are only added, never changed; a change of meaning gets a new version number.
pub const RECORDS_VERSION: &str = "zeppbridge.records.v2";

/// The six data classes that consumers care about, and their export type names.
///
/// The REST `types` parameter accepts these short names and also the original export type names (see [`expand_types`]).
pub const RECORD_TYPE_ALIASES: [(&str, &[&str]); 6] = [
    ("sleep", &["sleep"]),
    ("heart_rate", &["heart_rate"]),
    ("hrv", &["hrv", "hrv_rmssd"]),
    ("activity", &["daily_activity", "steps"]),
    ("workouts", &["workouts"]),
    ("weight", &["weight"]),
];

/// The default REST types when `types` is empty: the six classes above.
pub const DEFAULT_RECORD_TYPES: [&str; 6] = [
    "sleep",
    "heart_rate",
    "hrv",
    "activity",
    "workouts",
    "weight",
];

/// One record read request.
#[derive(Debug, Clone)]
pub struct RecordQuery {
    /// Export type names (aliases already expanded).
    pub types: Vec<String>,
    /// A date range (local days, both ends included) or a single workout.
    pub scope: ExportScope,
    /// Only records whose source payload came from the cloud after this time. `None` = no filter.
    pub since: Option<DateTime<Utc>>,
    pub detail: ExportDetail,
    /// Whether workouts include `route` (GPS track).
    pub include_gps: bool,
}

impl RecordQuery {
    /// A shortcut for a date range: the four parameters of `query_records(types, from, to, since)`.
    pub fn range(types: &[&str], from: &str, to: &str, since: Option<DateTime<Utc>>) -> Self {
        Self {
            types: types.iter().map(|value| value.to_string()).collect(),
            scope: ExportScope::DateRange {
                start: from.to_string(),
                end: to.to_string(),
            },
            since,
            detail: ExportDetail::Summary,
            include_gps: false,
        }
    }
}

/// The query result: the full JSON tree and the number of records in it.
#[derive(Debug, Clone)]
pub struct RecordSet {
    pub value: Value,
    pub record_count: usize,
}

/// Expand the REST type parameter into export type names. Unknown names come back as is in `Err`.
pub fn expand_types<'a, I>(requested: I) -> std::result::Result<Vec<String>, Vec<String>>
where
    I: IntoIterator<Item = &'a str>,
{
    let mut out: Vec<String> = Vec::new();
    let mut unknown = Vec::new();
    for raw in requested {
        let name = raw.trim().to_ascii_lowercase();
        if name.is_empty() {
            continue;
        }
        let expanded: Vec<&str> = match RECORD_TYPE_ALIASES.iter().find(|(alias, _)| *alias == name)
        {
            Some((_, targets)) => targets.to_vec(),
            None if EXPORT_DATA_TYPES.contains(&name.as_str()) => vec![name.as_str()],
            None => {
                unknown.push(name);
                continue;
            }
        };
        for value in expanded {
            if !out.iter().any(|existing| existing == value) {
                out.push(value.to_string());
            }
        }
    }
    if unknown.is_empty() {
        Ok(out)
    } else {
        Err(unknown)
    }
}

/// Read records. See the module comment.
pub fn query_records(db: &Database, query: &RecordQuery) -> Result<RecordSet> {
    let selection = ExportSelection {
        scope: Some(query.scope.clone()),
        start_date: None,
        end_date: None,
        data_types: query.types.clone(),
        detail: query.detail,
    };
    // Export rows and their source metadata must see the same version of the library.
    let snapshot = db
        .conn
        .is_autocommit()
        .then(|| db.conn.unchecked_transaction())
        .transpose()?;
    let (mut value, mut record_count) =
        db.build_records_export_value(&selection, query.since.is_some())?;
    let (start, end) = provenance_window(&value)?;
    let index = ProvenanceIndex::load(db, &start, &end, &value["data"], query.since.is_some())?;
    if let Some(snapshot) = snapshot {
        snapshot.commit()?;
    }
    let since = query.since;

    let Some(root) = value.as_object_mut() else {
        return Err(ZeppBridgeError::ParseError("The export result is not a JSON object".into()));
    };
    root.insert(
        "records_version".into(),
        Value::String(RECORDS_VERSION.to_string()),
    );
    root.insert(
        "source_note".into(),
        Value::String(
            "source: official = Zepp Open Platform authorization; advanced = account connector; imported = local file import; mixed = one folded value combines multiple sources; user = calendar context written by the user."
                .into(),
        ),
    );
    if let Some(since) = since {
        root.insert("since".into(), Value::String(since.to_rfc3339()));
    }
    root.insert("gps_included".into(), Value::Bool(query.include_gps));

    let mut kept_total = 0usize;
    let mut kept_by_type = HashMap::new();
    if let Some(data) = root.get_mut("data").and_then(Value::as_object_mut) {
        for (key, list) in data.iter_mut() {
            let Some(items) = list.as_array_mut() else {
                continue;
            };
            let kind = key.as_str();
            items.retain_mut(|item| {
                let found = if kind == "metric_samples" {
                    let changed_at = item
                        .as_object_mut()
                        .and_then(|item| item.remove("_sample_fetched_at"))
                        .and_then(|value| value.as_str().map(str::to_string));
                    let source = match item.get("source").and_then(Value::as_str) {
                        Some("official") => "official",
                        Some("mixed") => "mixed",
                        Some("imported") => "imported",
                        _ => "advanced",
                    };
                    Provenance { source, changed_at }
                } else {
                    index.lookup(kind, item)
                };
                if let Some(object) = item.as_object_mut() {
                    object.insert("source".into(), Value::String(found.source.to_string()));
                    if kind == "workouts" && !query.include_gps {
                        object.remove("route");
                    }
                }
                let keep = match since {
                    // Calendar context the user wrote does not come from the cloud and has no fetch time to compare, so it is always kept.
                    Some(since) if found.source != "user" => {
                        found.changed_at.as_deref().is_some_and(|changed| {
                            // For an unknown time, give more rather than let a record disappear silently.
                            DateTime::parse_from_rfc3339(changed)
                                .map_or(true, |changed| changed.with_timezone(&Utc) >= since)
                        })
                    }
                    _ => true,
                };
                if since.is_some() {
                    let category = item
                        .as_object_mut()
                        .and_then(|object| object.remove("_export_type"))
                        .and_then(|value| value.as_str().map(str::to_owned))
                        .or_else(|| match kind {
                            "sleep_sessions" => Some("sleep".into()),
                            "workouts" | "life_events" => Some(kind.into()),
                            _ => None,
                        });
                    if let Some(category) = category.filter(|_| keep) {
                        *kept_by_type.entry(category).or_insert(0usize) += 1;
                    }
                }
                keep
            });
            kept_total += items.len();
        }
    }
    if since.is_some() {
        record_count = kept_total;
        root.insert("record_count".into(), Value::from(record_count));
        if let Some(capabilities) = root.get_mut("capabilities").and_then(Value::as_object_mut) {
            for (category, entry) in capabilities {
                if entry.get("rows_in_export").is_some() {
                    entry["rows_in_export"] =
                        Value::from(kept_by_type.get(category).copied().unwrap_or_default());
                }
            }
        }
    }
    Ok(RecordSet {
        value,
        record_count,
    })
}

/// One flattened record, for REST paging.
#[derive(Debug, Clone)]
pub struct FlatRecord {
    pub record_type: &'static str,
    pub value: Value,
}

/// The arrays under `data` are flattened into one list in a fixed order, and each item gets a `record_type`.
pub fn flatten(set: &RecordSet) -> Vec<FlatRecord> {
    const ORDER: [(&str, &str); 5] = [
        ("sleep_sessions", "sleep_session"),
        ("workouts", "workout"),
        ("daily_metrics", "daily_metric"),
        ("metric_samples", "metric_sample"),
        ("life_events", "life_event"),
    ];
    let mut out = Vec::new();
    let Some(data) = set.value.get("data").and_then(Value::as_object) else {
        return out;
    };
    for (key, record_type) in ORDER {
        let Some(items) = data.get(key).and_then(Value::as_array) else {
            continue;
        };
        for item in items {
            let mut value = item.clone();
            if let Some(object) = value.as_object_mut() {
                object.insert("record_type".into(), Value::String(record_type.into()));
            }
            out.push(FlatRecord { record_type, value });
        }
    }
    out
}

/// The source of a record and the latest time it came from the cloud.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Provenance {
    source: &'static str,
    changed_at: Option<String>,
}

#[derive(Debug, Default)]
pub(crate) struct ProviderFlags {
    official: bool,
    legacy: bool,
    imported: bool,
    changed_at: Option<String>,
}

impl ProviderFlags {
    pub(crate) fn add(&mut self, provider: &str, fetched_at: Option<String>) {
        if provider == crate::official::fetch::PROVIDER {
            self.official = true;
        } else if provider == "imported" {
            self.imported = true;
        } else {
            self.legacy = true;
        }
        if let Some(fetched_at) = fetched_at {
            if self
                .changed_at
                .as_deref()
                .is_none_or(|known| fetched_at.as_str() > known)
            {
                self.changed_at = Some(fetched_at);
            }
        }
    }

    pub(crate) fn annotate_sample(&self, sample: &mut Value) {
        let provenance = self.resolve();
        sample["source"] = Value::from(provenance.source);
        if let Some(fetched_at) = provenance.changed_at {
            sample["_sample_fetched_at"] = Value::from(fetched_at);
        }
    }

    fn resolve(&self) -> Provenance {
        let source = match (self.official, self.legacy, self.imported) {
            (true, false, false) => "official",
            (false, false, true) => "imported",
            (true, _, _) | (_, true, true) => "mixed",
            _ => "advanced",
        };
        Provenance {
            source,
            changed_at: self.changed_at.clone(),
        }
    }
}

/// The `provider` and payload fetch time of each table in a query range.
///
/// This is read apart from the export because the export builder does not output these two columns. It reads only keys and source, not values.
#[derive(Debug, Default)]
struct ProvenanceIndex {
    sleep: HashMap<String, ProviderFlags>,
    workouts: HashMap<String, ProviderFlags>,
    daily: HashMap<(String, String), ProviderFlags>,
}

impl ProvenanceIndex {
    fn load(db: &Database, start: &str, end: &str, data: &Value, with_times: bool) -> Result<Self> {
        let mut index = Self::default();
        // Widen the bounds by one day: the export uses local days, and this compares UTC strings. Reading a bit more only adds a few unused keys.
        let (lower, upper) = widened_utc_bounds(start, end)?;
        let fetched = if with_times { "r.fetched_at" } else { "NULL" };

        let has = |key: &str| data[key].as_array().is_some_and(|items| !items.is_empty());
        if has("sleep_sessions") {
            let mut stmt = db.conn.prepare(&format!(
                "SELECT s.sleep_id, CASE WHEN s.import_batch_id IS NOT NULL THEN 'imported' ELSE s.provider END, {fetched}
             FROM sleep_sessions s LEFT JOIN raw_records r ON r.id = s.raw_record_id
             WHERE s.start_time >= ?1 AND s.start_time < ?2"
            ))?;
            for row in stmt.query_map([&lower, &upper], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            })? {
                let (id, provider, at) = row?;
                index.sleep.entry(id).or_default().add(&provider, at);
            }
        }
        if has("workouts") {
            // Workout details (per second, track) have their own payload. Late details also count as "this workout changed".
            let detail = if with_times {
                "(SELECT MAX(d.fetched_at) FROM raw_records d
               WHERE d.stream = 'workout_detail'
                 AND (d.source_key = 'official:sport_detail:' || w.workout_id
                      OR (d.source_key >= 'workout_detail:' || w.workout_id || ':'
                          AND d.source_key < 'workout_detail:' || w.workout_id || ';')))"
            } else {
                "NULL"
            };
            let mut stmt = db.conn.prepare(&format!(
                "SELECT w.workout_id, CASE WHEN i.workout_id IS NOT NULL THEN 'imported' ELSE w.provider END, {fetched}, {detail}
             FROM workouts w LEFT JOIN raw_records r ON r.id = w.raw_record_id
             LEFT JOIN workout_imports i ON i.workout_id = w.workout_id
             WHERE w.start_time >= ?1 AND w.start_time < ?2"
            ))?;
            for row in stmt.query_map([&lower, &upper], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            })? {
                let (id, provider, at, detail_at) = row?;
                let entry = index.workouts.entry(id).or_default();
                entry.add(&provider, at);
                entry.add(&provider, detail_at);
            }
        }
        if has("daily_metrics") {
            let mut stmt = db.conn.prepare(&format!(
                "SELECT d.date, d.metric, CASE WHEN d.import_batch_id IS NOT NULL THEN 'imported' ELSE d.provider END, {fetched}
             FROM daily_metrics d LEFT JOIN raw_records r ON r.id = d.raw_record_id
             WHERE d.date BETWEEN ?1 AND ?2"
            ))?;
            for row in stmt.query_map([start, end], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            })? {
                let (date, metric, provider, at) = row?;
                index
                    .daily
                    .entry((date, metric))
                    .or_default()
                    .add(&provider, at);
            }
        }
        Ok(index)
    }

    fn lookup(&self, kind: &str, item: &Value) -> Provenance {
        let text = |key: &str| item.get(key).and_then(Value::as_str).map(str::to_string);
        let found = match kind {
            "sleep_sessions" => text("sleep_id").and_then(|id| self.sleep.get(&id)),
            "workouts" => text("workout_id").and_then(|id| self.workouts.get(&id)),
            "daily_metrics" => match (text("date"), text("metric")) {
                (Some(date), Some(metric)) => self.daily.get(&(date, metric)),
                _ => None,
            },
            "life_events" => {
                return Provenance {
                    source: "user",
                    changed_at: None,
                }
            }
            _ => None,
        };
        found.map(ProviderFlags::resolve).unwrap_or(Provenance {
            source: "advanced",
            changed_at: None,
        })
    }
}

/// Read back the actual date range from the export result (for a single workout, the day of that workout).
fn provenance_window(value: &Value) -> Result<(String, String)> {
    let range = value.get("date_range");
    let pick = |key: &str| {
        range
            .and_then(|range| range.get(key))
            .and_then(Value::as_str)
            .map(str::to_string)
    };
    match (pick("start"), pick("end")) {
        (Some(start), Some(end)) => Ok((start, end)),
        _ => Err(ZeppBridgeError::ParseError("The export result has no date range".into())),
    }
}

fn widened_utc_bounds(start: &str, end: &str) -> Result<(String, String)> {
    let parse = |value: &str| {
        NaiveDate::parse_from_str(value, "%Y-%m-%d")
            .map_err(|_| ZeppBridgeError::ConfigError("Invalid date".into()))
    };
    let lower = parse(start)? - ChronoDuration::days(1);
    let upper = parse(end)? + ChronoDuration::days(2);
    Ok((
        format!("{}T00:00:00+00:00", lower.format("%Y-%m-%d")),
        format!("{}T00:00:00+00:00", upper.format("%Y-%m-%d")),
    ))
}
