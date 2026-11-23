//! "Higher / lower than usual": a deterministic flag that compares the user only with themselves.
//!
//! All rules are constants, and tests pin them:
//! - Baseline = readings in the 28 days **before** the latest reading (the latest day is not included);
//! - Fewer than 14 days with readings: no flag (not enough data to say what "usual" is);
//! - Usual = median. Spread = 1.4826 x median absolute deviation (not sensitive to one or two extreme days). When too small, 2% of the median is the floor;
//! - Flag "higher / lower than usual" only when the latest reading is more than 2 spreads from the median, and give the difference;
//! - The latest reading must be within the last 3 days. An older one is not "now";
//! - Also give a **norm band** (the card's bottom row, "status word <-> range"): the middle 80% of readings in the 90 days before the latest reading
//!   (10th to 90th percentile). It also compares only with the user, not with population norms. Fewer than 14 days: not given.
//! - **Long-term norm** ("your norm from ten years of history"): the middle 80% of **all** history before the latest reading (up to ten years),
//!   plus the median and band per year. Not given when the history spans less than one year or has fewer than 120 days of readings.
//!   A "norm" of less than one year is only a season, not you. The official API only gives a recent window, so only this device can give this.
//!
//! No diagnosis and no training advice: only direction, difference and the baseline itself. The UI uses `message_code` to say "6 higher than usual".

use chrono::{Duration, NaiveDate};
use serde::Serialize;

pub const BASELINE_DAYS: i64 = 28;
pub const MIN_DAYS: usize = 14;
pub const THRESHOLD: f64 = 2.0;
pub const FRESH_DAYS: i64 = 3;
const MAD_SCALE: f64 = 1.4826;
const MIN_RELATIVE_SPREAD: f64 = 0.02;
/// How many days the norm band looks back.
pub const USUAL_RANGE_DAYS: i64 = 90;
const USUAL_LOW_QUANTILE: f64 = 0.1;
const USUAL_HIGH_QUANTILE: f64 = 0.9;
/// The minimum span of the long-term norm (first reading to latest reading).
pub const LONG_TERM_MIN_SPAN_DAYS: i64 = 365;
/// The minimum number of days with readings for the long-term norm.
pub const LONG_TERM_MIN_DAYS: usize = 120;
/// The maximum look-back of the long-term norm (ten years).
pub const LONG_TERM_MAX_DAYS: i64 = 3660;

/// The norm in one calendar year.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct YearNorm {
    pub year: i32,
    pub days: usize,
    pub median: f64,
    pub low: f64,
    pub high: f64,
}

/// Your long-term norm: the middle 80% of all history.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LongTermNorm {
    /// The earliest reading.
    pub since: NaiveDate,
    /// How many days of readings were used.
    pub days: usize,
    pub median: f64,
    /// The 10th / 90th percentile.
    pub low: f64,
    pub high: f64,
    /// Per year (years with at least 14 days of readings), from earliest to latest.
    pub years: Vec<YearNorm>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BaselineDirection {
    Above,
    Below,
    Usual,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MetricBaseline {
    pub metric: String,
    pub latest_date: NaiveDate,
    pub latest: f64,
    /// The median of the 28 days before the latest reading.
    pub median: f64,
    /// The robust spread (1.4826 x MAD, with a floor).
    pub spread: f64,
    /// The number of days with readings in the baseline.
    pub days: usize,
    pub direction: BaselineDirection,
    /// Latest reading - median.
    pub delta: f64,
    /// `ui.metric_baseline.above` / `below` / `usual`.
    pub message_code: &'static str,
    /// The norm band: the 10th / 90th percentile of the previous 90 days. `None` when there are not enough days.
    pub usual_low: Option<f64>,
    pub usual_high: Option<f64>,
    /// How many days of readings the norm band used.
    pub usual_days: usize,
    /// The long-term norm (all history before the latest reading). `None` with less than one year of history.
    pub long_term: Option<LongTermNorm>,
}

fn band(values: &mut [f64]) -> (f64, f64, f64) {
    values.sort_by(|a, b| a.total_cmp(b));
    (
        quantile(values, USUAL_LOW_QUANTILE),
        median(values),
        quantile(values, USUAL_HIGH_QUANTILE),
    )
}

/// The long-term norm of one metric from all readings before `before` (any order, one per day).
pub fn long_term_norm(points: &[(NaiveDate, f64)], before: NaiveDate) -> Option<LongTermNorm> {
    use chrono::Datelike;
    let mut history: Vec<(NaiveDate, f64)> = points
        .iter()
        .copied()
        .filter(|(date, v)| v.is_finite() && *date < before)
        .collect();
    history.sort_by_key(|(date, _)| *date);
    let (since, _) = *history.first()?;
    let (last, _) = *history.last()?;
    if history.len() < LONG_TERM_MIN_DAYS || (last - since).num_days() < LONG_TERM_MIN_SPAN_DAYS {
        return None;
    }
    let mut years = Vec::new();
    for year in since.year()..=last.year() {
        let mut values: Vec<f64> = history
            .iter()
            .filter(|(date, _)| date.year() == year)
            .map(|(_, v)| *v)
            .collect();
        if values.len() < MIN_DAYS {
            continue;
        }
        let (low, median, high) = band(&mut values);
        years.push(YearNorm {
            year,
            days: values.len(),
            median,
            low,
            high,
        });
    }
    let mut values: Vec<f64> = history.iter().map(|(_, v)| *v).collect();
    let (low, median, high) = band(&mut values);
    Some(LongTermNorm {
        since,
        days: values.len(),
        median,
        low,
        high,
        years,
    })
}

/// Linear interpolation quantile of a sorted array.
fn quantile(sorted: &[f64], q: f64) -> f64 {
    let at = q * (sorted.len() - 1) as f64;
    let lo = at.floor() as usize;
    let hi = at.ceil() as usize;
    sorted[lo] + (sorted[hi] - sorted[lo]) * (at - lo as f64)
}

fn median(sorted: &[f64]) -> f64 {
    let n = sorted.len();
    if n % 2 == 1 {
        sorted[n / 2]
    } else {
        (sorted[n / 2 - 1] + sorted[n / 2]) / 2.0
    }
}

/// Daily readings of one metric (any order, one per day) -> a self-comparison finding. `None` when data is thin or the latest reading is too old.
pub fn metric_baseline(
    metric: &str,
    points: &[(NaiveDate, f64)],
    today: NaiveDate,
) -> Option<MetricBaseline> {
    let mut points: Vec<(NaiveDate, f64)> = points
        .iter()
        .copied()
        .filter(|(_, v)| v.is_finite())
        .collect();
    points.sort_by_key(|(date, _)| *date);
    let &(latest_date, latest) = points.last()?;
    if latest_date < today - Duration::days(FRESH_DAYS) {
        return None;
    }
    let from = latest_date - Duration::days(BASELINE_DAYS);
    let mut window: Vec<f64> = points
        .iter()
        .filter(|(date, _)| *date >= from && *date < latest_date)
        .map(|(_, v)| *v)
        .collect();
    if window.len() < MIN_DAYS {
        return None;
    }
    window.sort_by(|a, b| a.total_cmp(b));
    let center = median(&window);
    let mut deviations: Vec<f64> = window.iter().map(|v| (v - center).abs()).collect();
    deviations.sort_by(|a, b| a.total_cmp(b));
    let spread = (MAD_SCALE * median(&deviations))
        .max(center.abs() * MIN_RELATIVE_SPREAD)
        .max(1e-9);
    let delta = latest - center;
    let usual_from = latest_date - Duration::days(USUAL_RANGE_DAYS);
    let mut usual: Vec<f64> = points
        .iter()
        .filter(|(date, _)| *date >= usual_from && *date < latest_date)
        .map(|(_, v)| *v)
        .collect();
    usual.sort_by(|a, b| a.total_cmp(b));
    let (usual_low, usual_high) = if usual.len() >= MIN_DAYS {
        (
            Some(quantile(&usual, USUAL_LOW_QUANTILE)),
            Some(quantile(&usual, USUAL_HIGH_QUANTILE)),
        )
    } else {
        (None, None)
    };
    let direction = if delta / spread >= THRESHOLD {
        BaselineDirection::Above
    } else if delta / spread <= -THRESHOLD {
        BaselineDirection::Below
    } else {
        BaselineDirection::Usual
    };
    Some(MetricBaseline {
        metric: metric.to_string(),
        latest_date,
        latest,
        median: center,
        spread,
        days: window.len(),
        direction,
        delta,
        message_code: match direction {
            BaselineDirection::Above => "ui.metric_baseline.above",
            BaselineDirection::Below => "ui.metric_baseline.below",
            BaselineDirection::Usual => "ui.metric_baseline.usual",
        },
        usual_low,
        usual_high,
        usual_days: usual.len(),
        long_term: long_term_norm(&points, latest_date),
    })
}
