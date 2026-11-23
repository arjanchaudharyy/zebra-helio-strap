//! Capability board: which data streams have evidence and which the cloud rejects (split out of storage/mod.rs by domain, logic unchanged).

use super::*;

/// Where a capability's evidence lives, so the overview can count it without
/// a network request.
pub(super) enum CapabilityEvidence {
    /// Distinct days in `daily_metrics` whose metric matches a prefix.
    DailyPrefix(&'static str),
    /// Rows in `metric_samples` for one metric name.
    Samples(&'static str),
    /// Rows in a table with a timestamp column.
    Table(&'static str, &'static str),
}

/// The capability list, in display order.
///
/// Nine of these are answered entirely from stored data — the strongest
/// evidence available, since "you have 32 days of stress readings" beats any
/// probe. Only the three with no local trace need a request, and those are the
/// ones where silence is genuinely ambiguous.
pub(super) const CAPABILITY_ROWS: [(&str, CapabilityEvidence, i64); 17] = [
    ("heart_rate", CapabilityEvidence::Samples("heart_rate"), 30),
    (
        "sleep",
        CapabilityEvidence::Table("sleep_sessions", "start_time"),
        30,
    ),
    (
        "workouts",
        CapabilityEvidence::Table("workouts", "start_time"),
        90,
    ),
    ("steps", CapabilityEvidence::DailyPrefix("steps"), 30),
    (
        "daily_activity",
        CapabilityEvidence::DailyPrefix("distance"),
        30,
    ),
    ("stress", CapabilityEvidence::DailyPrefix("stress"), 30),
    ("spo2", CapabilityEvidence::DailyPrefix("spo2"), 30),
    (
        "respiratory_rate",
        CapabilityEvidence::DailyPrefix("respiratory"),
        30,
    ),
    ("hrv", CapabilityEvidence::Samples("hrv"), 30),
    ("hrv_rmssd", CapabilityEvidence::Samples("hrv_rmssd"), 30),
    ("recovery", CapabilityEvidence::DailyPrefix("readiness"), 30),
    (
        "training_load",
        CapabilityEvidence::DailyPrefix("training_load"),
        30,
    ),
    ("vo2max", CapabilityEvidence::DailyPrefix("vo2max"), 365),
    (
        "lactate_threshold",
        CapabilityEvidence::DailyPrefix("lactate_threshold"),
        365,
    ),
    ("pai", CapabilityEvidence::DailyPrefix("pai"), 30),
    // Weight is no longer a "probe only" row. It is now really stored, so the evidence is the
    // samples in the database. This is the end of the issue four people reported: before, this row always showed the probe's
    // result "no measurements in the last 365 days", because the probe hit an endpoint that is empty for everyone.
    ("weight", CapabilityEvidence::Samples("weight"), 365),
    // Food logs work the same way. The window is 365 days: manual logs have gaps, and one week with no log does not mean the
    // account lacks this feature.
    ("food", CapabilityEvidence::DailyPrefix("intake_"), 365),
];

/// The metric names one weigh-in produces.
///
/// Exports select by *type* (`--types weight`) while `metric_samples` stores by
/// *metric*, and unlike heart rate the two do not share a name — a weigh-in
/// yields eleven differently-named rows. Kept in step with `BODY_METRICS` in
/// the normalizer: a name added there and forgotten here is written to the
/// database and then silently missing from every export.
pub const BODY_COMPOSITION_METRICS: [&str; 11] = [
    "weight",
    "bmi",
    "height",
    "body_fat_rate",
    "body_water_rate",
    "muscle_mass",
    "bone_mass",
    "protein_rate",
    "visceral_fat",
    "bmr",
    "body_balance_score",
];

/// Capabilities that still get periodic network probes. Blood pressure uses
/// local samples when present; empty libraries still need candidate probes so
/// newly available cloud data can be discovered.
pub const PROBE_ONLY_CAPABILITIES: [&str; 2] = ["blood_pressure", "emotion"];

/// The observation range for local blood pressure evidence. Each candidate endpoint's request range comes from probe.window_days.
pub(super) const PROBE_WINDOW_DAYS: i64 = 365;

pub(super) const CAPABILITY_PROBE_RESULT_KEY: &str = "capability_probe_result";

pub(super) const CAPABILITY_PROBE_AT_KEY: &str = "capability_probe_at";

impl Database {
    /// Build the capability overview: read what the library already proves,
    /// then fold in the stored result of the last probe for the rest.
    pub fn capability_overview(&self, today: NaiveDate) -> Result<CapabilityOverview> {
        let saved: Vec<CapabilityProbe> = self
            .get_app_meta(CAPABILITY_PROBE_RESULT_KEY)?
            .and_then(|raw| serde_json::from_str::<Vec<CapabilityProbe>>(&raw).ok())
            .unwrap_or_default();
        let mut probed: BTreeMap<String, &CapabilityProbe> = BTreeMap::new();
        for probe in &saved {
            let current = probed.entry(probe.stream.clone()).or_insert(probe);
            if probe_rank(probe) > probe_rank(current) {
                *current = probe;
            }
        }
        let mut items = Vec::new();
        for (stream, evidence, window_days) in CAPABILITY_ROWS {
            // Inclusive local calendar window of exactly `window_days`:
            // `today - (window_days - 1)` .. `today`.
            let start = today - Duration::days(window_days - 1);
            let start_text = start.to_string();
            let end_text = today.to_string();
            // The UTC grace bounds let the index cut the range first, and date() only refines it: metric_samples
            // uses the uq prefix (metric, timestamp), workouts uses v33's
            // idx_workouts_start. sleep_sessions has no start_time index yet, so the bound is
            // only a harmless extra predicate for it. The shape stays the same, and it gains when that index exists.
            let (utc_lower, utc_upper) = utc_bounds_or_unbounded(&start_text, &end_text);
            let (records, latest, unit) = match evidence {
                CapabilityEvidence::DailyPrefix(prefix) => {
                    let pattern = format!("{prefix}%");
                    let row = self.conn.query_row(
                        "SELECT COUNT(DISTINCT date), MAX(date) FROM daily_metrics
                         WHERE metric LIKE ?1 AND date BETWEEN ?2 AND ?3",
                        params![pattern, start_text, end_text],
                        |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Option<String>>(1)?)),
                    )?;
                    (row.0, row.1, ("days", "days"))
                }
                CapabilityEvidence::Samples(metric) => {
                    let row = self.conn.query_row(
                        "SELECT COUNT(*), MAX(date(timestamp, 'localtime')) FROM metric_samples
                         WHERE metric = ?1
                           AND timestamp >= ?4 AND timestamp < ?5
                           AND date(timestamp, 'localtime') BETWEEN ?2 AND ?3",
                        params![metric, start_text, end_text, utc_lower, utc_upper],
                        |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Option<String>>(1)?)),
                    )?;
                    (row.0, row.1, ("records", "records"))
                }
                CapabilityEvidence::Table(table, column) => {
                    let sql = format!(
                        "SELECT COUNT(*), MAX(date({column}, 'localtime')) FROM {table}
                         WHERE {column} >= ?1 AND {column} < ?2
                           AND date({column}, 'localtime') BETWEEN ?3 AND ?4"
                    );
                    let row = self.conn.query_row(
                        &sql,
                        params![utc_lower, utc_upper, start_text, end_text],
                        |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Option<String>>(1)?)),
                    )?;
                    (row.0, row.1, ("records", "records"))
                }
            };
            let (unit_label, unit_code) = unit;
            // A positive Food probe proves records exist even when their
            // shape is not yet understood. Never turn a parser gap into
            // "nothing recorded", or present cloud records as local data.
            if stream == "food" && records == 0 {
                if let Some(probe) = probed.get(stream).filter(|probe| {
                    probe.status == "available"
                        && probe.records > 0
                        && probe.latest_date.as_deref().is_some_and(|date| {
                            date >= start_text.as_str() && date <= end_text.as_str()
                        })
                }) {
                    items.push(CapabilityItem {
                        stream: stream.to_string(),
                        status: "available".to_string(),
                        records: probe.records as i64,
                        records_unit: "records".to_string(),
                        records_unit_code: "records".to_string(),
                        window_days: probe.window_days,
                        latest_date: probe.latest_date.clone(),
                        note: Some("The cloud has records, but this machine has no usable data yet. Sync or backfill. If the data is still missing, the payload format may need support.".to_string()),
                        source: "probed".to_string(),
                        ingested: false,
                    });
                    continue;
                }
            }
            items.push(CapabilityItem {
                stream: stream.to_string(),
                status: if records > 0 {
                    "available"
                } else {
                    "no_records"
                }
                .to_string(),
                records,
                records_unit: unit_label.to_string(),
                records_unit_code: unit_code.to_string(),
                window_days,
                latest_date: latest,
                note: (records == 0).then(|| format!("No records in the last {window_days} days")),
                source: "derived".to_string(),
                // The evidence for these rows is data in the database, so by definition it is stored.
                ingested: true,
            });
        }

        // Streams that leave no local trace: report the last probe, or say
        // plainly that they have not been checked yet.
        for stream in PROBE_ONLY_CAPABILITIES {
            if stream == "blood_pressure" {
                let (records, latest): (i64, Option<String>) = self.conn.query_row(
                    "SELECT COUNT(*), MAX(date(timestamp, 'localtime')) FROM metric_samples
                     WHERE metric = 'blood_pressure_systolic'
                     AND date(timestamp, 'localtime') BETWEEN ?1 AND ?2",
                    params![
                        (today - Duration::days(PROBE_WINDOW_DAYS - 1)).to_string(),
                        today.to_string()
                    ],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )?;
                if records > 0 {
                    items.push(CapabilityItem {
                        stream: stream.into(),
                        status: "available".into(),
                        records,
                        records_unit: "records".into(),
                        records_unit_code: "records".into(),
                        window_days: PROBE_WINDOW_DAYS,
                        latest_date: latest,
                        note: Some(
                            "Recognized blood pressure readings are stored. The candidate endpoints still need a check on an account that really measures.".into(),
                        ),
                        source: "derived".into(),
                        ingested: true,
                    });
                    continue;
                }
            }
            let item = match probed.get(stream) {
                Some(probe) if probe.status == "available" => CapabilityItem {
                    stream: stream.to_string(),
                    status: "available".to_string(),
                    records: probe.records as i64,
                    records_unit: "records".to_string(),
                    records_unit_code: "records".to_string(),
                    window_days: probe.window_days,
                    latest_date: probe.latest_date.clone(),
                    // Make clear this is the cloud count, not the local count.
                    note: Some(
                        "The probe endpoint returned records, but this machine has no usable data yet. Sync or backfill. Unknown payloads are kept until their format gets support."
                            .to_string(),
                    ),
                    source: "probed".to_string(),
                    ingested: false,
                },
                // Even a rejection is about this candidate's request contract,
                // not the device: the bloodPressure from/to variant returns
                // 400 while days/toDate succeeds on the same account.
                Some(probe) if probe.status == "unavailable" => CapabilityItem {
                    stream: stream.to_string(),
                    status: "unknown".to_string(),
                    records: 0,
                    records_unit: "records".to_string(),
                    records_unit_code: "records".to_string(),
                    window_days: probe.window_days,
                    latest_date: None,
                    note: Some("The candidate endpoints we queried are not available. We cannot tell yet whether the account or device supports this.".to_string()),
                    source: "probed".to_string(),
                    ingested: false,
                },
                Some(probe) => CapabilityItem {
                    stream: stream.to_string(),
                    status: if probe.status == "empty" { "no_records" } else { "unknown" }.to_string(),
                    records: 0,
                    records_unit: "records".to_string(),
                    records_unit_code: "records".to_string(),
                    window_days: probe.window_days,
                    latest_date: None,
                    note: Some(if probe.status == "empty" {
                        format!("The candidate endpoints we queried returned no records in the last {} days. This does not prove the device took no measurements.", probe.window_days)
                    } else if probe.status == "unrecognized" {
                        "The endpoint returned a structure we do not recognize yet. We cannot treat it as no records.".into()
                    } else {
                        "The last probe request failed. The data capability is not confirmed yet.".into()
                    }),
                    source: "probed".to_string(),
                    ingested: false,
                },
                None => CapabilityItem {
                    stream: stream.to_string(),
                    status: "unknown".to_string(),
                    records: 0,
                    records_unit: "records".to_string(),
                    records_unit_code: "records".to_string(),
                    window_days: PROBE_WINDOW_DAYS,
                    latest_date: None,
                    note: Some("Not checked yet".to_string()),
                    source: "probed".to_string(),
                    ingested: false,
                },
            };
            items.push(item);
        }

        Ok(CapabilityOverview {
            items,
            probed_at: self.get_app_meta(CAPABILITY_PROBE_AT_KEY)?,
        })
    }

    pub fn save_capability_probe(&self, probes: &[CapabilityProbe]) -> Result<()> {
        // The scheduled sync probes only request-only streams, while the
        // diagnostics button probes every stream. Replacing the whole list
        // here used to erase a positive Food result on the next sync.
        let mut merged: BTreeMap<(String, String, String, String), CapabilityProbe> = self
            .get_app_meta(CAPABILITY_PROBE_RESULT_KEY)?
            .and_then(|raw| serde_json::from_str::<Vec<CapabilityProbe>>(&raw).ok())
            .unwrap_or_default()
            .into_iter()
            .map(|probe| (probe_key(&probe), probe))
            .collect();
        let mut updated = false;
        for probe in probes {
            // A failed request does not erase previously observed data on
            // the same surface. A first-ever failure remains unknown.
            if probe.status == "error" && merged.contains_key(&probe_key(probe)) {
                continue;
            }
            let mut saved = probe.clone();
            // Field names are useful in the immediate diagnostic result,
            // but the board needs only the count and date.
            saved.fields.clear();
            merged.insert(probe_key(probe), saved);
            updated = true;
        }
        if !updated {
            return Ok(());
        }
        let encoded = serde_json::to_string(&merged.into_values().collect::<Vec<_>>())
            .map_err(|error| ZeppBridgeError::ParseError(error.to_string()))?;
        self.set_app_meta(CAPABILITY_PROBE_RESULT_KEY, &encoded)?;
        self.set_app_meta(CAPABILITY_PROBE_AT_KEY, &Utc::now().to_rfc3339())
    }

    /// Whether the request-only streams are due a re-check.
    ///
    /// A first answer is not a permanent one: someone may start measuring
    /// blood pressure, or connect a scale, long after install.
    pub fn capability_probe_is_stale(&self, max_age_days: i64) -> Result<bool> {
        let Some(raw) = self.get_app_meta(CAPABILITY_PROBE_AT_KEY)? else {
            return Ok(true);
        };
        let Ok(probed_at) = DateTime::parse_from_rfc3339(&raw) else {
            return Ok(true);
        };
        Ok((Utc::now() - probed_at.with_timezone(&Utc)).num_days() >= max_age_days)
    }
}

fn probe_key(probe: &CapabilityProbe) -> (String, String, String, String) {
    (
        probe.stream.clone(),
        probe.surface.clone(),
        probe.event_type.clone(),
        probe.sub_type.clone(),
    )
}

fn probe_rank(probe: &CapabilityProbe) -> u8 {
    match probe.status.as_str() {
        "available" => 4,
        "error" | "unrecognized" => 3,
        "empty" => 2,
        _ => 1,
    }
}
