//! Metrics the user chose to "exclude from analysis".
//!
//! User feedback: some people find Zepp algorithm estimates such as VO₂max absurd and want to exclude them with one click.
//! We do not judge "accurate or not" for the user. The UI only explains where each item comes from (sensor / fixed formula /
//! algorithm estimate), and the user decides whether to exclude it. Excluding **deletes no data**, and cards still show. It only means:
//! no baseline comparison, not in the export sent to AI, removed from MCP results, with an explicit note that the user excluded it,
//! so the AI does not treat it as missing data and ask about it or infer from it.
//!
//! The list lives in `app_meta`: MCP is another process that reopens a read-only connection per request, so it takes effect on the next call.

use super::*;

const EXCLUDED_METRICS_KEY: &str = "excluded_metrics";

/// The same metric also appears in payloads under these keys (training context and workout summaries split it by field).
const ALIASES: [(&str, &[&str]); 2] = [
    (
        "lactate_threshold",
        &["lactate_threshold_hr", "lactate_threshold_pace"],
    ),
    ("training_load", &["daily_load"]),
];

impl Database {
    /// Metrics excluded by the user, sorted by name.
    pub fn excluded_metrics(&self) -> Result<Vec<String>> {
        Ok(self
            .get_app_meta(EXCLUDED_METRICS_KEY)?
            .and_then(|raw| serde_json::from_str::<Vec<String>>(&raw).ok())
            .unwrap_or_default())
    }

    /// Excludes / restores one metric and returns the new list. Accepts only metric names of lowercase letters, digits and underscores.
    pub fn set_metric_excluded(&self, metric: &str, excluded: bool) -> Result<Vec<String>> {
        let metric = metric.trim();
        if metric.is_empty()
            || metric.len() > 64
            || !metric
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        {
            return Err(ZeppBridgeError::ConfigError(format!(
                "Unknown metric name: {metric}"
            )));
        }
        let mut list = self.excluded_metrics()?;
        list.retain(|item| item != metric);
        if excluded {
            list.push(metric.to_string());
        }
        list.sort();
        self.set_app_meta(EXCLUDED_METRICS_KEY, &serde_json::to_string(&list)?)?;
        Ok(list)
    }
}

/// Key normalization: lowercase, underscores removed. `vo2max` / `vo2Max` / `VO2_MAX` are the same.
fn normalized(key: &str) -> String {
    key.chars()
        .filter(|c| *c != '_')
        .flat_map(char::to_lowercase)
        .collect()
}

fn excluded_keys(excluded: &[String]) -> Vec<String> {
    let mut keys: Vec<String> = excluded.iter().map(|metric| normalized(metric)).collect();
    for (metric, aliases) in ALIASES {
        if excluded.iter().any(|item| item == metric) {
            keys.extend(aliases.iter().map(|alias| normalized(alias)));
        }
    }
    keys
}

/// Removes excluded metrics from JSON about to be sent out: fields with the same name (ignoring case and underscores),
/// and array elements whose `metric` field is that name. Returns whether anything was removed.
pub fn strip_excluded_metrics(value: &mut serde_json::Value, excluded: &[String]) -> bool {
    if excluded.is_empty() {
        return false;
    }
    strip(value, &excluded_keys(excluded))
}

fn strip(value: &mut serde_json::Value, keys: &[String]) -> bool {
    use serde_json::Value;
    let mut removed = false;
    match value {
        Value::Object(map) => {
            let before = map.len();
            map.retain(|key, _| !keys.contains(&normalized(key)));
            removed |= map.len() != before;
            for child in map.values_mut() {
                removed |= strip(child, keys);
            }
        }
        Value::Array(items) => {
            let before = items.len();
            items.retain(|item| {
                !item
                    .get("metric")
                    .and_then(Value::as_str)
                    .is_some_and(|metric| keys.contains(&normalized(metric)))
            });
            removed |= items.len() != before;
            for child in items.iter_mut() {
                removed |= strip(child, keys);
            }
        }
        _ => {}
    }
    removed
}

/// The note for the AI: exclusion is the user's choice, not missing data.
pub const EXCLUSION_NOTE: &str = "The user chose in ZeppBridge to exclude these metrics from analysis (for example because they find the watch estimates unreliable). The related fields were removed from this data. This is not missing data: do not infer from it, and do not ask the user to supply it.";

/// Removes excluded metrics. If anything was removed, writes `excludedByUser` at the top level.
pub fn apply_exclusions(value: &mut serde_json::Value, excluded: &[String]) {
    if strip_excluded_metrics(value, excluded) {
        if let Some(object) = value.as_object_mut() {
            object.insert(
                "excludedByUser".to_string(),
                serde_json::json!({ "metrics": excluded, "note": EXCLUSION_NOTE }),
            );
        }
    }
}
