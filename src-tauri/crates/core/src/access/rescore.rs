//! Rescore insights by grant scope: use only the runs in scope (split out of access.rs, same logic).

use super::*;

/// The fields one granted workout needs in the baseline rescore.
#[derive(Debug, Clone)]
pub struct GrantedRun {
    pub workout_id: String,
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    pub distance_meters: Option<f64>,
    pub avg_hr: Option<i32>,
    pub training_load: Option<f64>,
}

impl GrantedRun {
    pub fn from_workout(workout: &Workout) -> Self {
        Self {
            workout_id: workout.workout_id.clone(),
            start_time: workout.start_time,
            end_time: workout.end_time,
            distance_meters: workout.distance_meters,
            avg_hr: workout.avg_hr,
            training_load: workout.training_load,
        }
    }

    /// Same rule as `insight::RunRow::duration_seconds`.
    pub(super) fn duration_seconds(&self) -> Option<f64> {
        let seconds = (self.end_time - self.start_time).num_seconds();
        (seconds > 0).then_some(seconds as f64)
    }

    /// All candidates come from rows that `workout_insight` already filtered. Pace was already checked against
    /// the valid range on ingest, so only the raw conversion is needed here.
    pub(super) fn pace_seconds_per_km(&self) -> Option<f64> {
        let distance = self.distance_meters.filter(|value| *value > 0.0)?;
        let seconds = self.duration_seconds()?;
        Some(seconds / (distance / 1000.0))
    }
}

/// Rescore one `WorkoutInsight` so that only evidence in the granted set remains.
///
/// `granted` is the union of granted workout ids. `rows` holds the details the caller fetched for those ids (covering at least
/// `baseline_included ∪ baseline_excluded ∩ granted`).
/// `heart_rate_drift` uses only the target's own samples and is not trimmed.
pub fn rescore_insight(
    insight: &mut WorkoutInsight,
    granted: &BTreeSet<String>,
    rows: &BTreeMap<String, GrantedRun>,
) {
    if !insight.supported {
        return;
    }

    // 1. Trim baseline members to the granted set first. Granted rows from `beyond_max_samples` are picked out first,
    //    and fill freed slots in the original order (newest to oldest). Rows with no free slot stay in the excluded list.
    let mut included: Vec<GrantedRun> = insight
        .baseline_included
        .iter()
        .filter(|entry| granted.contains(&entry.workout_id))
        .filter_map(|entry| rows.get(&entry.workout_id).cloned())
        .collect();
    let mut promotable: Vec<String> = Vec::new();
    let mut excluded: Vec<BaselineExclusion> = Vec::new();
    for entry in insight
        .baseline_excluded
        .iter()
        .filter(|entry| granted.contains(&entry.workout_id))
    {
        if entry.reason == "beyond_max_samples" {
            promotable.push(entry.workout_id.clone());
        } else {
            excluded.push(entry.clone());
        }
    }
    for workout_id in promotable {
        if included.len() >= insight::baseline::MAX_SAMPLES {
            excluded.push(BaselineExclusion {
                workout_id,
                reason: "beyond_max_samples".into(),
            });
        } else if let Some(row) = rows.get(&workout_id) {
            included.push(row.clone());
        }
        // If rows does not have this entry (must not happen on one connection), treat it as not granted. Listing less is better
        // than putting an id with no data into the baseline.
    }
    // The order keeps the original insight's meaning: included is already newest to oldest, and fill-in rows follow in the original excluded
    // list order (also newest to oldest). No extra sort.

    insight.baseline_included = included
        .iter()
        .map(|row| BaselineEntry {
            workout_id: row.workout_id.clone(),
            start_time: row.start_time.to_rfc3339(),
            distance_meters: row.distance_meters.unwrap_or_default(),
        })
        .collect();
    insight.baseline_excluded = excluded;

    // 2. Rescore each fact against the granted baseline. Samples that are not granted must not appear in references
    //    and must not count in the mean. A baseline_value computed from data that is not granted
    //    is a leak, however clean the list is.
    for fact in &mut insight.facts {
        rescore_fact(fact, &included);
    }
}

/// The granted-set version of the `insight::run_fact` decision table. The logic matches the original line by line.
/// `baseline_window` keeps its original value: it describes the rule itself, not the sample set.
pub(super) fn rescore_fact(fact: &mut InsightFact, included: &[GrantedRun]) {
    let extract: fn(&GrantedRun) -> Option<f64> = match fact.metric.as_str() {
        "distance" => |row| row.distance_meters,
        "duration" => |row| row.duration_seconds(),
        "pace" => |row| row.pace_seconds_per_km(),
        "avg_hr" => |row| row.avg_hr.map(f64::from),
        "training_load" => |row| row.training_load,
        _ => |_| None,
    };
    let mut values = Vec::new();
    let mut refs = Vec::new();
    for row in included {
        if let Some(sample) = extract(row) {
            values.push(sample);
            refs.push(row.workout_id.clone());
        }
    }

    let enough = values.len() >= insight::baseline::MIN_SAMPLES;
    let (comparison, reason, reason_code) = match (fact.value, mean(&values)) {
        (Some(current), Some(previous)) if enough && previous != 0.0 => {
            let delta = current - previous;
            (
                Some(Comparison {
                    baseline_value: round1(previous),
                    delta: round1(delta),
                    delta_percent: round1(delta / previous.abs() * 100.0),
                    direction: direction_of(delta),
                }),
                None,
                None,
            )
        }
        (Some(_), Some(previous)) if enough && previous == 0.0 => (
            None,
            Some("The baseline mean of this metric in granted records is 0, so the relative change cannot be computed.".into()),
            Some("workout_zero_baseline".to_string()),
        ),
        (Some(_), _) => (
            None,
            Some(format!(
                "Among granted records, runs of similar distance (±{:.0}%) with this metric number only {}, fewer than {}, so only this run's value is reported, with no comparison.",
                insight::baseline::DISTANCE_TOLERANCE * 100.0,
                values.len(),
                insight::baseline::MIN_SAMPLES
            )),
            Some("workout_thin_baseline".to_string()),
        ),
        (None, _) => (
            None,
            Some("This workout has no data for this metric.".into()),
            Some("workout_no_value".to_string()),
        ),
    };

    fact.comparison = comparison;
    fact.evidence_count = values.len() as i64;
    fact.baseline_count = values.len() as i64;
    fact.confidence = if fact.value.is_none() {
        Confidence::Insufficient
    } else {
        // Same table as `insight::Confidence::from_samples` (a private function, mirrored here).
        match values.len() {
            0..=2 => Confidence::Insufficient,
            3..=4 => Confidence::Low,
            5..=7 => Confidence::Medium,
            _ => Confidence::High,
        }
    };
    fact.reason = reason;
    fact.reason_code = reason_code;
    fact.evidence_refs = refs;
}

/// Same threshold as `insight::direction_of`: below half a display unit counts as flat.
pub(super) fn direction_of(delta: f64) -> String {
    if delta.abs() < 0.5 {
        "same".into()
    } else if delta > 0.0 {
        "higher".into()
    } else {
        "lower".into()
    }
}

pub(super) fn mean(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    Some(values.iter().sum::<f64>() / values.len() as f64)
}

pub(super) fn round1(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

/* ------------------------------ Grant loading ------------------------------

* The `ai_tasks` table stores tasks (S1, v32 migration): one task per row, with a JSON payload
* that has `schema_version`, and `mcp_shared` as an indexed column. The baseline branch does not have this table yet,
* so loading is fully defensive: a missing table, missing columns, or one broken row payload all count as "this entry is not granted".
* It is better to grant one entry less than to open the whole scope or crash because of one bad row. */
