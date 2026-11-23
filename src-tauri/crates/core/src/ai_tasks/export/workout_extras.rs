//! Two extra field blocks on the anchor workout: a comparison with recent runs of the same kind plus first/second half (`comparison`, from the workout page insight card),
//! and climb / descent segments (`climbs`, from the climb card). Before, the export had only one summary row, so the AI could not say "faster or slower than usual"
//! or "which part was uphill". These are added fields only. No existing field changes. They start at the standard level.

use super::*;
use crate::insight::WorkoutInsight;
use crate::workout_climbs::WorkoutClimbs;

/// The insight part for the AI: each reading compared with the baseline, how many runs the baseline used, and the two halves. Absent for unsupported sports (only running for now).
pub(super) fn comparison_json(insight: &WorkoutInsight) -> Result<Option<Value>> {
    if !insight.supported {
        return Ok(None);
    }
    let mut facts = Vec::with_capacity(insight.facts.len());
    for fact in &insight.facts {
        let mut row = Map::new();
        row.insert("fact_id".into(), json!(fact.fact_id));
        row.insert("metric".into(), json!(fact.metric));
        row.insert("value".into(), json!(fact.value));
        row.insert("unit".into(), json!(fact.unit));
        if let Some(comparison) = &fact.comparison {
            row.insert("baseline_value".into(), json!(comparison.baseline_value));
            row.insert("delta".into(), json!(comparison.delta));
            row.insert("delta_percent".into(), json!(comparison.delta_percent));
            row.insert("direction".into(), json!(comparison.direction));
        }
        row.insert("evidence_count".into(), json!(fact.evidence_count));
        row.insert("confidence".into(), serde_json::to_value(fact.confidence)?);
        facts.push(Value::Object(row));
    }
    let mut object = Map::new();
    object.insert(
        "baseline_runs".into(),
        json!(insight.baseline_included.len()),
    );
    if let Some(window) = insight
        .facts
        .iter()
        .find_map(|fact| fact.baseline_window.as_ref())
    {
        object.insert("baseline_window".into(), serde_json::to_value(window)?);
    }
    object.insert("facts".into(), Value::Array(facts));
    if let Some(drift) = &insight.heart_rate_drift {
        object.insert("heart_rate_drift".into(), serde_json::to_value(drift)?);
    } else if let Some(code) = &insight.heart_rate_drift_unavailable {
        object.insert("heart_rate_drift_unavailable".into(), json!(code));
    }
    Ok(Some(Value::Object(object)))
}

/// Climb segments and the detection thresholds. Empty `segments` = altitude exists, but no segment qualifies.
pub(super) fn climbs_json(climbs: &WorkoutClimbs) -> Result<Value> {
    Ok(serde_json::to_value(climbs)?)
}

/// When heart rate is excluded: remove the heart rate items in the comparison, the two halves (meters per beat can give back heart rate), and each segment's average heart rate.
pub(super) fn strip_heart_rate(object: &mut Map<String, Value>) {
    let is_heart = |metric: &str| {
        metric.ends_with("_hr") || metric.starts_with("hr_") || metric.contains("heart")
    };
    if let Some(comparison) = object.get_mut("comparison").and_then(Value::as_object_mut) {
        comparison.remove("heart_rate_drift");
        comparison.remove("heart_rate_drift_unavailable");
        if let Some(facts) = comparison.get_mut("facts").and_then(Value::as_array_mut) {
            facts.retain(
                |fact| !matches!(fact.get("metric").and_then(Value::as_str), Some(metric) if is_heart(metric)),
            );
        }
    }
    if let Some(segments) = object
        .get_mut("climbs")
        .and_then(|climbs| climbs.get_mut("segments"))
        .and_then(Value::as_array_mut)
    {
        for segment in segments {
            if let Some(segment) = segment.as_object_mut() {
                segment.remove("average_hr");
            }
        }
    }
}
