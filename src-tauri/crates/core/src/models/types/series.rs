//! Daily metric series, training load and heart rate zones (split from models/types.rs by domain; shapes unchanged).

use super::*;

/// One day of a metric, with the spread behind it when the source has one.
///
/// `min` / `max` are only populated where the data really carries them --
/// either a companion daily metric (stress, respiratory rate) or the spread of
/// that day's samples. A day with a single reading reports no spread rather
/// than a zero-width one.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MetricSeriesPoint {
    pub date: String,
    pub value: f64,
    pub min: Option<f64>,
    pub max: Option<f64>,
    /// How many readings the day's value was computed from, for sample-backed
    /// metrics. Absent for metrics the server already summarised per day.
    pub samples: Option<i64>,
}

/// One metric over a window, plus the facts the UI needs to label it honestly.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MetricSeries {
    pub metric: String,
    pub unit: String,
    /// `daily_metrics` or `metric_samples` -- which table the values came from.
    pub source: String,
    pub points: Vec<MetricSeriesPoint>,
    pub latest: Option<MetricSeriesPoint>,
    /// Mean of the daily values in the window, not of the raw samples.
    pub average: Option<f64>,
    pub minimum: Option<f64>,
    pub maximum: Option<f64>,
    /// Days in the window that carry a value, so the UI can say how much of
    /// the range is actually covered instead of drawing a line through gaps.
    pub days_with_data: i64,
    pub window_days: i64,
}

/// BP trends average readings by day, but their extrema must still describe
/// the measured readings, not the extrema of those daily averages.
pub fn metric_series_extrema(
    metric: &str,
    points: &[MetricSeriesPoint],
) -> (Option<f64>, Option<f64>) {
    let measured = metric.starts_with("blood_pressure_");
    (
        points
            .iter()
            .map(|point| {
                if measured {
                    point.min.unwrap_or(point.value)
                } else {
                    point.value
                }
            })
            .filter(|v| v.is_finite())
            .reduce(f64::min),
        points
            .iter()
            .map(|point| {
                if measured {
                    point.max.unwrap_or(point.value)
                } else {
                    point.value
                }
            })
            .filter(|v| v.is_finite())
            .reduce(f64::max),
    )
}

/// One day of acute/chronic training load.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TrainingBalancePoint {
    pub date: String,
    pub acute_7d: Option<f64>,
    pub acute_days_with_data: i64,
    pub chronic_28d: Option<f64>,
    pub chronic_days_with_data: i64,
    /// Absent unless both windows are complete and chronic load is positive.
    pub acute_chronic_ratio: Option<f64>,
}

/// One measured number a heart-rate zone model can be built on.
///
/// Every basis names where it came from and when it was measured. Nothing here
/// is estimated: there is deliberately no 220-minus-age entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HeartRateBasis {
    pub id: String,
    /// `max_hr`, `resting_hr` or `threshold_hr` -- which slot it can fill.
    pub kind: String,
    pub label: String,
    pub value: f64,
    pub unit: String,
    /// Where the number is stored, e.g. `max(workouts.max_hr)`.
    pub source: String,
    /// The day it was measured, when the source pins one down.
    pub measured_at: Option<String>,
    /// The note text. The CLI / MCP use it whatever the UI language is; the UI builds its own text from `id`.
    pub note: Option<String>,
    /// The number in the note (how many days the local resting heart rate used). The UI
    /// needs this number, so a fixed text looked up by `id` is not enough.
    #[serde(default)]
    pub note_count: Option<i64>,
}

/// One band of a zone model, as a percentage of its basis.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HeartRateZoneBand {
    pub zone: i32,
    pub label: String,
    pub low_percent: f64,
    pub high_percent: f64,
}

/// A way of turning measured heart rates into five zones.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HeartRateZoneModel {
    pub id: String,
    pub label: String,
    pub formula: String,
    /// Basis kinds this model needs before it can be computed.
    pub requires: Vec<String>,
    pub bands: Vec<HeartRateZoneBand>,
    /// False when the library holds no basis of a required kind.
    pub available: bool,
}

/// One computed zone with the time spent in it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HeartRateZoneRow {
    pub zone: i32,
    pub label: String,
    pub min_bpm: i32,
    pub max_bpm: i32,
    pub seconds: i64,
}

/// Which model and bases the user picked. Every field starts empty: the
/// application does not choose a heart-rate model on someone's behalf.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HeartRateZonePreference {
    pub model: Option<String>,
    pub max_basis: Option<String>,
    pub resting_basis: Option<String>,
    pub threshold_basis: Option<String>,
}

/// The zones for one chosen model, over one window of workout samples.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HeartRateZoneReport {
    pub model: String,
    pub model_label: String,
    pub formula: String,
    /// The bases actually used, so the reader can check the arithmetic.
    pub bases: Vec<HeartRateBasis>,
    pub zones: Vec<HeartRateZoneRow>,
    pub below_zone_1_seconds: i64,
    /// Seconds above the model's top boundary. Zepp brackets its own zones the
    /// same way, and keeping the overflow separate means the five labelled
    /// zones stay exactly what their labels say.
    pub above_zone_5_seconds: i64,
    pub total_seconds: i64,
    pub window_days: i64,
    pub source: String,
}

/// Everything the zone picker needs: what can be measured, what can be built
/// from it, what the user chose, and the result of that choice.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HeartRateZoneOptions {
    pub bases: Vec<HeartRateBasis>,
    pub models: Vec<HeartRateZoneModel>,
    pub preference: HeartRateZonePreference,
    /// Present only once the preference names a model and its bases.
    pub report: Option<HeartRateZoneReport>,
    pub window_days: i64,
}
