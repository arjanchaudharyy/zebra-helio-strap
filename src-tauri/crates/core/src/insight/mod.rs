use crate::models::error::Result;

use crate::storage::Database;

use chrono::{DateTime, Duration, Local, NaiveDate, Utc};

use serde::{Deserialize, Serialize};

pub mod anomaly;
pub mod letters;
mod weekly_report;
mod workout_insight;

impl Database {
    /// "Higher / lower than usual" findings for these metrics (compared only with the user's own data, see [`anomaly`]). Metrics without enough data are not returned.
    pub fn metric_baselines(
        &self,
        metrics: &[String],
        today: NaiveDate,
    ) -> Result<Vec<anomaly::MetricBaseline>> {
        // The long-term norm needs the full history (up to ten years). The 28-day baseline and the 90-day band are inside it.
        let series = self.metric_series(metrics, anomaly::LONG_TERM_MAX_DAYS)?;
        Ok(series
            .iter()
            .filter_map(|s| anomaly::metric_baseline(&s.metric, &dated_points(s), today))
            .collect())
    }

    /// The norms for the AI (MCP `get_baselines`): recent self-comparison findings plus the long-term norm from full history.
    /// When the latest reading is too old, the recent findings are empty. The long-term norm is still given (it does not look at "now").
    pub fn metric_norms(&self, metrics: &[String], today: NaiveDate) -> Result<Vec<MetricNorms>> {
        let series = self.metric_series(metrics, anomaly::LONG_TERM_MAX_DAYS)?;
        Ok(series
            .iter()
            .filter(|s| !s.points.is_empty())
            .map(|s| {
                let points = dated_points(s);
                let mut recent = anomaly::metric_baseline(&s.metric, &points, today);
                let long_term = match recent.as_mut() {
                    Some(recent) => recent.long_term.take(),
                    None => anomaly::long_term_norm(&points, today + Duration::days(1)),
                };
                MetricNorms {
                    metric: s.metric.clone(),
                    unit: s.unit.clone(),
                    reading_days: points.len(),
                    recent,
                    long_term,
                }
            })
            .collect())
    }
}

/// The norm of one metric, for the AI to read.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MetricNorms {
    pub metric: String,
    pub unit: String,
    /// How many days of readings this device has in total (up to ten years).
    pub reading_days: usize,
    /// The last-28-day comparison and the 90-day norm band. `None` when the latest reading is not within 3 days.
    pub recent: Option<anomaly::MetricBaseline>,
    /// The long-term norm from full history and the median per year. `None` with less than one year.
    pub long_term: Option<anomaly::LongTermNorm>,
}

fn dated_points(series: &crate::models::MetricSeries) -> Vec<(NaiveDate, f64)> {
    series
        .points
        .iter()
        .filter_map(|p| {
            Some((
                NaiveDate::parse_from_str(&p.date, "%Y-%m-%d").ok()?,
                p.value,
            ))
        })
        .collect()
}

/// Baseline rules for a single-run insight. They are constants, not literals spread in SQL,
/// because they decide the findings and tests must pin them.
pub mod baseline {
    /// Distance tolerance for comparable runs: +/-20%.
    pub const DISTANCE_TOLERANCE: f64 = 0.20;
    /// The minimum number of comparable runs before a comparison is given.
    pub const MIN_SAMPLES: usize = 3;
    /// The maximum number of recent runs to use. More is not more accurate; it only mixes in older form.
    pub const MAX_SAMPLES: usize = 10;
    /// Only look back this many days.
    pub const WINDOW_DAYS: i64 = 180;
}

/// Weekly report window: the last 7 days against the personal baseline of the 28 days before.
pub mod weekly {
    pub const RECENT_DAYS: i64 = 7;
    pub const BASELINE_DAYS: i64 = 28;
    /// The baseline needs at least this many days with data. Otherwise the finding reports the current value only, with no comparison.
    pub const MIN_BASELINE_DAYS: i64 = 7;
}

/// The conditions for heart rate drift (first half against second half).
///
/// These constants decide "does it count", so they all live here and not in functions. Tests
/// must pin them, and readers must be able to check them.
pub mod drift {
    /// Shorter than this does not count. In the first ten minutes the heart rate is still rising.
    /// A comparison with the second half then measures the warm-up, not drift.
    pub const MIN_DURATION_SECONDS: i64 = 20 * 60;
    /// Each half needs at least this many samples with both heart rate and speed.
    pub const MIN_SAMPLES_PER_HALF: usize = 60;
    /// Above this speed coefficient of variation, it does not count. Intervals, traffic lights and hills
    /// scatter the speed. Then the difference between halves comes from the route, not the body.
    pub const MAX_SPEED_CV: f64 = 0.20;
    /// Samples with a heart rate below this value count as not measured (a poor fit can drop to single digits).
    pub const MIN_PLAUSIBLE_HR: f64 = 40.0;
    /// Speed below this value counts as stopped and is left out.
    pub const MIN_PLAUSIBLE_SPEED_MPS: f64 = 0.5;
}

/// A "pace x heart rate" comparison of the first and second half of a workout.
///
/// It measures **meters per heartbeat** (speed / heart rate). A lower value in the second half means
/// the same speed costs more heartbeats. This is usually called heart rate drift or decoupling.
///
/// Route conditions easily spoil this metric: traffic lights, hills, intervals and GPS drift make the two
/// halves not comparable. So when the conditions fail, it returns `None` and a reason code, and **does not force a percentage**.
/// This is the same rule as elsewhere in this module: when evidence is thin, say so.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HeartRateDrift {
    /// Meters per heartbeat in the first half.
    pub first_half_metres_per_beat: f64,
    /// Meters per heartbeat in the second half.
    pub second_half_metres_per_beat: f64,
    /// The change of the second half against the first half, in percent. A negative value means the same speed costs more heartbeats.
    pub drift_percent: f64,
    pub first_half_avg_hr: f64,
    pub second_half_avg_hr: f64,
    pub first_half_avg_speed_mps: f64,
    pub second_half_avg_speed_mps: f64,
    /// The number of samples used in each half.
    pub first_half_samples: i64,
    pub second_half_samples: i64,
    /// The speed coefficient of variation, so the reader can judge how steady this run was.
    pub speed_cv: f64,
}

/// A sample point with both heart rate and speed.
struct DriftSample {
    unix: i64,
    heart_rate: f64,
    speed_mps: f64,
}

/// One fact and its evidence.
///
/// A `value` of `None` means there is no local data for this item. It means "none", not 0.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InsightFact {
    /// A stable machine-readable id, for example `run.pace` or `weekly.resting_hr`.
    /// The UI and the AI branch on it. Do not branch on the text.
    pub fact_id: String,
    /// The metric name, aligned with the metric name in the database.
    pub metric: String,
    pub value: Option<f64>,
    pub unit: String,
    /// The comparison with the personal baseline. `None` when the evidence is thin.
    pub comparison: Option<Comparison>,
    pub baseline_window: Option<BaselineWindow>,
    /// How many samples the baseline actually used.
    pub evidence_count: i64,
    /// `device` / `user_fused` / `unknown`.
    pub source: String,
    pub confidence: Confidence,
    /// Why there is no comparison, or why the confidence is low. A finding can also have a note.
    ///
    /// The plain text for the CLI / MCP / export. It does not follow the UI language. The UI reads
    /// `reason_code` below and writes its own sentence with `baseline_window` and `baseline_count`.
    pub reason: Option<String>,
    /// The stable code of the note: `weekly_thin_baseline` / `weekly_no_recent_data` /
    /// `weekly_zero_baseline` / `workout_thin_baseline` / `workout_no_value` /
    /// `workout_zero_baseline`.
    #[serde(default)]
    pub reason_code: Option<String>,
    /// How many samples the baseline actually found. `evidence_count` counts the samples of this period.
    /// They are different. The UI needs this one to write "only M of the previous N days have data".
    #[serde(default)]
    pub baseline_count: i64,
    /// The database rows this fact points back to (workout id or date), so each one can be checked.
    pub evidence_refs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Comparison {
    pub baseline_value: f64,
    pub delta: f64,
    pub delta_percent: f64,
    /// `higher` / `lower` / `same`. The direction is a fact. The UI decides good or bad from the metric:
    /// a smaller pace number is faster, and a lower resting heart rate is usually good. This layer makes no value judgment.
    pub direction: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BaselineWindow {
    /// `comparable_runs` or `previous_days`.
    pub kind: String,
    pub days: i64,
    pub min_samples: i64,
    pub max_samples: i64,
    /// Distance tolerance. Only `comparable_runs` has it.
    pub distance_tolerance_percent: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    High,
    Medium,
    Low,
    /// The evidence is thin. This finding reports the current value only, with no comparison.
    Insufficient,
}

impl Confidence {
    fn from_samples(count: usize) -> Self {
        match count {
            0..=2 => Confidence::Insufficient,
            3..=4 => Confidence::Low,
            5..=7 => Confidence::Medium,
            _ => Confidence::High,
        }
    }
}

/// The insight for one workout.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkoutInsight {
    pub workout_id: String,
    /// The workout type that is currently in effect for this record.
    pub workout_type: String,
    /// Whether insights support this type. The first version only supports verified running.
    pub supported: bool,
    /// The reason when it is not supported. `None` when it is supported.
    pub unsupported_reason: Option<String>,
    /// The stable code of the unsupported reason. At this time only `unsupported_workout_type` exists.
    #[serde(default)]
    pub unsupported_code: Option<String>,
    pub facts: Vec<InsightFact>,
    /// The records that the baseline actually includes.
    pub baseline_included: Vec<BaselineEntry>,
    /// The excluded records and the reason. The user can see "why that run was not counted".
    pub baseline_excluded: Vec<BaselineExclusion>,
    /// The "pace x heart rate" comparison of the first and second half. `None` when the conditions fail.
    #[serde(default)]
    pub heart_rate_drift: Option<HeartRateDrift>,
    /// When it cannot compute, it gives a stable reason code: `not_enough_samples` / `too_short` /
    /// `pace_too_variable`. The rule here is: **do not force a percentage**.
    #[serde(default)]
    pub heart_rate_drift_unavailable: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BaselineEntry {
    pub workout_id: String,
    pub start_time: String,
    pub distance_meters: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BaselineExclusion {
    pub workout_id: String,
    /// The stable exclusion reason: `distance_out_of_tolerance` / `outside_window` /
    /// `missing_distance` / `missing_duration` / `implausible_pace` /
    /// `beyond_max_samples`.
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WeeklyReport {
    pub generated_at: String,
    pub recent_start: String,
    pub recent_end: String,
    pub baseline_start: String,
    pub baseline_end: String,
    pub facts: Vec<InsightFact>,
}

/// The first version of single-workout insights only covers running. Running is the only type that has
/// per-point samples, pace, power and running form, all verified with real data. Other types still show,
/// correct and export as usual, but here we say "not supported" and do not apply unverified rules.
const SUPPORTED_WORKOUT_TYPES: [&str; 1] = ["run"];

#[derive(Debug, Clone)]
struct RunRow {
    workout_id: String,
    start_time: DateTime<Utc>,
    end_time: DateTime<Utc>,
    distance_meters: Option<f64>,
    avg_hr: Option<i32>,
    training_load: Option<f64>,
    source_scope: String,
}

impl RunRow {
    fn duration_seconds(&self) -> Option<f64> {
        let seconds = (self.end_time - self.start_time).num_seconds();
        (seconds > 0).then_some(seconds as f64)
    }

    /// Pace in seconds per kilometer. With no distance or duration there is no pace; 0 does not stand in for it.
    fn pace_seconds_per_km(&self) -> Option<f64> {
        let distance = self.distance_meters.filter(|value| *value > 0.0)?;
        let seconds = self.duration_seconds()?;
        let pace = seconds / (distance / 1000.0);
        // The world record is about 130 s/km, and walking is about 900 s/km. A value outside this range usually
        // means the distance or duration is wrong. Using it in an average only spoils the baseline.
        (120.0..=1800.0).contains(&pace).then_some(pace)
    }
}

fn mean(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    Some(values.iter().sum::<f64>() / values.len() as f64)
}

fn stdev(values: &[f64]) -> Option<f64> {
    if values.len() < 2 {
        return None;
    }
    let average = mean(values)?;
    let variance = values
        .iter()
        .map(|value| (value - average).powi(2))
        .sum::<f64>()
        / values.len() as f64;
    Some(variance.sqrt())
}

fn round1(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

fn saturating_days_before(date: NaiveDate, days: i64) -> NaiveDate {
    Duration::try_days(days)
        .and_then(|delta| date.checked_sub_signed(delta))
        .unwrap_or(date)
}

/// Direction of change.
///
/// The UI rounds metrics such as heart rate to whole numbers. With "2% of baseline" as the "same" threshold,
/// 50 vs 51 bpm (-1.3%) became `same`, so this week's bar had no color while the numbers
/// next to it showed two different values. Only less than half a display unit is noise.
fn direction_of(delta: f64, _previous: f64) -> String {
    if delta.abs() < 0.5 {
        "same".into()
    } else if delta > 0.0 {
        "higher".into()
    } else {
        "lower".into()
    }
}

#[cfg(test)]
mod tests;
