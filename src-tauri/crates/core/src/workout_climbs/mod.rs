//! The main climbs and descents in one workout.
//!
//! It only uses the per-point altitude the watch recorded: no map, no added points. With no altitude samples it is `None` (the UI shows no card).
//! With altitude but no qualifying section, it is an empty list (the UI says "no clear climb"). Distance comes from integrating per-point speed.
//! When too much speed is missing, the distance is not reliable, so it is also `None`. Time never stands in for distance.
//!
//! Method: first smooth the altitude with a centered moving average over time (removes barometer / GPS noise), then find turning points with a pullback threshold
//! (a climb ends only when it drops more than the threshold from its highest point; a descent is the reverse). Each pair of neighboring turning points is one section.
//! Only sections with enough height change and average grade stay. All thresholds come with the result in [`ClimbMethod`],
//! so the UI "how it is computed" text reads them and does not keep its own numbers.

use chrono::DateTime;
use serde::{Deserialize, Serialize};

use crate::models::WorkoutSeriesSample;

/// Altitude smoothing window (seconds, centered).
pub const SMOOTHING_WINDOW_S: f64 = 30.0;
/// A section ends only after a pullback of more than this many meters from its highest (lowest) point.
pub const REVERSAL_M: f64 = 10.0;
/// The minimum height change of a section.
pub const MIN_CHANGE_M: f64 = 30.0;
/// The minimum average grade (absolute value) of a section.
pub const MIN_GRADE_PCT: f64 = 3.0;
/// Time below this speed is not moving time (vertical speed and pace both use moving time).
pub const MOVING_SPEED_M_S: f64 = 0.5;

/// The minimum distance of a section; otherwise the grade is only noise from the speed integral.
const MIN_DISTANCE_M: f64 = 100.0;
/// When two samples are further apart than this, distance is only integrated for this long (no made-up distance during pauses or signal loss).
const MAX_GAP_S: f64 = 10.0;
/// A jump between two neighboring altitude readings larger than this many meters is a sensor break and does not count in the height change (same rule as the summary total ascent).
const MAX_STEP_M: f64 = 50.0;
/// When the share of samples with speed is below this, the distance is not reliable.
const MIN_SPEED_COVERAGE: f64 = 0.8;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ClimbKind {
    Climb,
    Descent,
}

/// The detection parameters. They come with the result, and the explanation text reads only these.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ClimbMethod {
    pub smoothing_window_s: f64,
    pub reversal_m: f64,
    pub min_change_m: f64,
    pub min_grade_pct: f64,
    pub moving_speed_m_s: f64,
}

impl Default for ClimbMethod {
    fn default() -> Self {
        Self {
            smoothing_window_s: SMOOTHING_WINDOW_S,
            reversal_m: REVERSAL_M,
            min_change_m: MIN_CHANGE_M,
            min_grade_pct: MIN_GRADE_PCT,
            moving_speed_m_s: MOVING_SPEED_M_S,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ClimbSegment {
    pub kind: ClimbKind,
    pub start_time: String,
    pub end_time: String,
    /// Distance from the workout start (meters, from the per-point speed integral).
    pub start_distance_m: f64,
    pub end_distance_m: f64,
    /// Smoothed height change: positive for a climb, negative for a descent.
    pub elevation_change_m: f64,
    /// Average grade (%), with the same sign as the height change.
    pub average_grade_pct: f64,
    pub moving_seconds: i64,
    /// Vertical speed (meters / hour, absolute value), from moving time. Empty when there is no moving time.
    pub vertical_speed_m_per_h: Option<f64>,
    pub average_hr: Option<f64>,
    /// Average pace in the section (minutes / km), from moving time.
    pub average_pace_min_per_km: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkoutClimbs {
    pub method: ClimbMethod,
    /// In time order. Empty list = altitude exists, but no qualifying climb or descent.
    pub segments: Vec<ClimbSegment>,
}

/// A sample point with an altitude reading.
struct Point {
    sample: usize,
    t: f64,
    /// Continuous altitude with sensor breaks removed.
    altitude: f64,
}

/// `recorded_distance_m` is the total recorded distance of the workout. When given, the speed-integrated distance is scaled to it
/// (per-point speed is low when the signal is poor: one 13 km hike integrated to only 11.2 km, which overstates the grade).
/// When the two differ too much (not between 0.5x and 2x), no scaling is done, and the integrated value is used.
pub fn detect_climbs(
    samples: &[WorkoutSeriesSample],
    recorded_distance_m: Option<f64>,
) -> Option<WorkoutClimbs> {
    let times: Vec<Option<f64>> = samples
        .iter()
        .map(|sample| {
            DateTime::parse_from_rfc3339(&sample.timestamp)
                .ok()
                .map(|time| time.timestamp_millis() as f64 / 1000.0)
        })
        .collect();

    let timed = times.iter().filter(|time| time.is_some()).count();
    let with_speed = samples
        .iter()
        .zip(&times)
        .filter(|(sample, time)| time.is_some() && plausible_speed(sample.speed).is_some())
        .count();
    if timed < 2 || (with_speed as f64) < (timed as f64) * MIN_SPEED_COVERAGE {
        return None;
    }

    // Cumulative distance and cumulative moving time, with indexes matching samples.
    let mut distance = vec![0.0; samples.len()];
    let mut moving = vec![0.0; samples.len()];
    let mut previous: Option<f64> = None;
    for (index, sample) in samples.iter().enumerate() {
        if index > 0 {
            distance[index] = distance[index - 1];
            moving[index] = moving[index - 1];
        }
        let Some(t) = times[index] else { continue };
        if let Some(prev) = previous {
            let dt = (t - prev).clamp(0.0, MAX_GAP_S);
            if let Some(speed) = plausible_speed(sample.speed) {
                distance[index] += speed * dt;
                if speed >= MOVING_SPEED_M_S {
                    moving[index] += dt;
                }
            }
        }
        previous = Some(t);
    }
    let integrated = distance.last().copied().unwrap_or(0.0);
    if let Some(recorded) = recorded_distance_m.filter(|value| value.is_finite() && *value > 0.0) {
        let scale = recorded / integrated;
        if integrated > 0.0 && (0.5..=2.0).contains(&scale) {
            distance.iter_mut().for_each(|value| *value *= scale);
        }
    }

    let mut points: Vec<Point> = Vec::new();
    let mut last_raw: Option<f64> = None;
    for (index, sample) in samples.iter().enumerate() {
        let (Some(t), Some(raw)) = (times[index], plausible_altitude(sample.altitude_m)) else {
            continue;
        };
        let altitude = match (last_raw, points.last()) {
            (Some(prev_raw), Some(prev)) => {
                let step = raw - prev_raw;
                prev.altitude + if step.abs() > MAX_STEP_M { 0.0 } else { step }
            }
            _ => raw,
        };
        last_raw = Some(raw);
        points.push(Point {
            sample: index,
            t,
            altitude,
        });
    }
    if points.len() < 2 {
        return None;
    }

    let smoothed = smooth(&points);
    let pivots = pivots(&smoothed);

    let mut segments = Vec::new();
    for pair in pivots.windows(2) {
        let (from, to) = (pair[0], pair[1]);
        let change = smoothed[to] - smoothed[from];
        let (start, end) = (points[from].sample, points[to].sample);
        let run = distance[end] - distance[start];
        if change.abs() < MIN_CHANGE_M || run < MIN_DISTANCE_M {
            continue;
        }
        let grade = change / run * 100.0;
        if grade.abs() < MIN_GRADE_PCT {
            continue;
        }
        let moving_s = moving[end] - moving[start];
        let heart_rates: Vec<f64> = samples[start..=end]
            .iter()
            .filter_map(|sample| sample.heart_rate)
            .filter(|hr| (30..=250).contains(hr))
            .map(f64::from)
            .collect();
        segments.push(ClimbSegment {
            kind: if change > 0.0 {
                ClimbKind::Climb
            } else {
                ClimbKind::Descent
            },
            start_time: samples[start].timestamp.clone(),
            end_time: samples[end].timestamp.clone(),
            start_distance_m: round1(distance[start]),
            end_distance_m: round1(distance[end]),
            elevation_change_m: round1(change),
            average_grade_pct: round1(grade),
            moving_seconds: moving_s.round() as i64,
            vertical_speed_m_per_h: (moving_s >= 1.0)
                .then(|| round1(change.abs() / moving_s * 3600.0)),
            average_hr: (!heart_rates.is_empty())
                .then(|| round1(heart_rates.iter().sum::<f64>() / heart_rates.len() as f64)),
            average_pace_min_per_km: (moving_s >= 1.0)
                .then(|| (moving_s / 60.0 / (run / 1000.0) * 100.0).round() / 100.0),
        });
    }

    Some(WorkoutClimbs {
        method: ClimbMethod::default(),
        segments,
    })
}

/// Centered moving average over time; it also works with uneven sample spacing.
fn smooth(points: &[Point]) -> Vec<f64> {
    let half = SMOOTHING_WINDOW_S / 2.0;
    let (mut lo, mut hi) = (0usize, 0usize);
    let mut sum = 0.0;
    let mut out = Vec::with_capacity(points.len());
    for point in points {
        while hi < points.len() && points[hi].t <= point.t + half {
            sum += points[hi].altitude;
            hi += 1;
        }
        while points[lo].t < point.t - half {
            sum -= points[lo].altitude;
            lo += 1;
        }
        out.push(sum / (hi - lo) as f64);
    }
    out
}

/// Find turning points with a pullback threshold. The returned indexes increase, and between two neighbors the altitude only rises or only falls (pullbacks below the threshold are allowed).
fn pivots(altitude: &[f64]) -> Vec<usize> {
    let mut out = Vec::new();
    let (mut high, mut low) = (0usize, 0usize);
    // 0 = no direction yet, 1 = climbing, -1 = descending; extreme is the highest (lowest) point of this section so far.
    let mut direction = 0i8;
    let mut extreme = 0usize;
    for index in 1..altitude.len() {
        let value = altitude[index];
        match direction {
            0 => {
                if value > altitude[high] {
                    high = index;
                }
                if value < altitude[low] {
                    low = index;
                }
                if altitude[high] - altitude[low] >= REVERSAL_M {
                    if high > low {
                        out.push(low);
                        direction = 1;
                        extreme = high;
                    } else {
                        out.push(high);
                        direction = -1;
                        extreme = low;
                    }
                }
            }
            1 => {
                if value >= altitude[extreme] {
                    extreme = index;
                } else if altitude[extreme] - value >= REVERSAL_M {
                    out.push(extreme);
                    direction = -1;
                    extreme = index;
                }
            }
            _ => {
                if value <= altitude[extreme] {
                    extreme = index;
                } else if value - altitude[extreme] >= REVERSAL_M {
                    out.push(extreme);
                    direction = 1;
                    extreme = index;
                }
            }
        }
    }
    if direction != 0 {
        out.push(extreme);
    }
    out
}

fn plausible_speed(speed: Option<f64>) -> Option<f64> {
    speed.filter(|value| value.is_finite() && (0.0..=60.0).contains(value))
}

fn plausible_altitude(altitude: Option<f64>) -> Option<f64> {
    altitude.filter(|value| value.is_finite() && (-500.0..=10_000.0).contains(value))
}

fn round1(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

#[cfg(test)]
mod tests;
