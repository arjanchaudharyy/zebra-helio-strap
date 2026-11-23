//! Tree rings yearly review: one ring per year, one star per workout. All computed and drawn on this device.
//!
//! Real data only. Missing data stays blank:
//!
//! - One ring per year from the first year with data to this year. A year with no data in between still gets a ring, but `data_days = 0` and the UI draws it blank;
//! - One tick per week (53 per year, the last one holds only a day or two). The value is that week's workout minutes. A week with **no data on any day**
//!   (no daily metrics, no sleep, no workouts) is `None`, and the UI breaks the line there instead of drawing 0;
//! - Missing steps or sleep are `None`, never padded with 0;
//! - Duration runs from start to end (pauses included). A workout over 48 hours counts as a bad record and adds no duration.
//!
//! Dates are always local days in the device time zone (the same as the PB wall and running shoes).
use crate::models::error::Result;
use crate::storage::logbook::RUN_TYPES;
use crate::storage::Database;
use chrono::{Datelike, Local, NaiveDate};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet};

/// Number of week ticks in a year (tick 53 holds only December 30 / 31).
pub const WEEKS: usize = 53;
/// A workout longer than this counts as a bad record and adds no duration.
const MAX_WORKOUT_SECONDS: i64 = 48 * 3600;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct RingStar {
    pub workout_id: String,
    /// Local date.
    pub date: String,
    /// Effective workout type (the user's edit wins).
    pub kind: String,
    pub is_run: bool,
    pub distance_m: Option<f64>,
    pub duration_s: Option<i64>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct RingHighlight {
    pub workout_id: String,
    pub date: String,
    pub kind: String,
    pub distance_m: Option<f64>,
    pub duration_s: Option<i64>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct RingRace {
    pub name: String,
    pub date: String,
    pub result_seconds: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct RingYear {
    pub year: i32,
    /// This year is not over yet.
    pub in_progress: bool,
    /// Number of days with any data (daily metrics, sleep, workouts).
    pub data_days: u32,
    pub workouts: u32,
    /// Number of days with a workout.
    pub active_days: u32,
    pub duration_s: i64,
    pub distance_m: f64,
    pub runs: u32,
    pub run_distance_m: f64,
    /// Workout minutes per week. `None` when the week has no data on any day.
    pub weeks: Vec<Option<f64>>,
    /// 1–12, the month with the most workouts (ties pick the earlier one). `None` when there are no workouts.
    pub busiest_month: Option<u32>,
    pub busiest_month_workouts: u32,
    pub longest_run: Option<RingHighlight>,
    pub longest_workout: Option<RingHighlight>,
    /// Total steps over the days that have steps. `None` when no day has any.
    pub steps_total: Option<i64>,
    pub steps_days: u32,
    /// Mean minutes of the main sleep per night (the longest session per wake-up day).
    pub sleep_avg_minutes: Option<f64>,
    pub sleep_nights: u32,
    /// PBs set this year that still stand (`5k` / `10k` / `half` / `marathon`).
    pub personal_records: Vec<&'static str>,
    /// Races finished this year with a result entered, by date.
    pub races: Vec<RingRace>,
    pub stars: Vec<RingStar>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct AnnualRings {
    /// Oldest first (tree rings grow from the inside out).
    pub years: Vec<RingYear>,
    pub today: String,
}

/// Input of the pure function: one workout.
#[derive(Debug, Clone)]
pub struct RingWorkout {
    pub workout_id: String,
    pub date: NaiveDate,
    pub kind: String,
    pub distance_m: Option<f64>,
    pub seconds: Option<i64>,
}

/// Input of the pure function: everything except workouts.
#[derive(Debug, Clone, Default)]
pub struct RingExtras {
    /// Days with any data (workouts not included; workout days are added automatically).
    pub data_days: HashSet<NaiveDate>,
    pub steps: HashMap<NaiveDate, i64>,
    /// Wake-up day -> minutes of the longest sleep session.
    pub sleep: HashMap<NaiveDate, i64>,
    /// (PB key, date set).
    pub records: Vec<(&'static str, NaiveDate)>,
    pub races: Vec<(String, NaiveDate, i64)>,
}

/// Week tick index within a year (from 0).
pub fn week_slot(date: NaiveDate) -> usize {
    (date.ordinal0() as usize / 7).min(WEEKS - 1)
}

fn highlight(w: &RingWorkout) -> RingHighlight {
    RingHighlight {
        workout_id: w.workout_id.clone(),
        date: w.date.to_string(),
        kind: w.kind.clone(),
        distance_m: w.distance_m,
        duration_s: w.seconds,
    }
}

/// Lays out one year's items as a ring. `workouts` holds only this year's workouts.
fn ring_year(
    year: i32,
    today: NaiveDate,
    workouts: &[&RingWorkout],
    extras: &RingExtras,
) -> RingYear {
    let in_year = |d: &NaiveDate| d.year() == year;
    let mut covered: HashSet<NaiveDate> =
        extras.data_days.iter().copied().filter(in_year).collect();
    covered.extend(workouts.iter().map(|w| w.date));

    let mut weeks: Vec<Option<f64>> = vec![None; WEEKS];
    for day in &covered {
        weeks[week_slot(*day)].get_or_insert(0.0);
    }
    let mut months = [0u32; 12];
    let mut active: HashSet<NaiveDate> = HashSet::new();
    let (mut duration_s, mut distance_m, mut runs, mut run_distance_m) = (0i64, 0.0, 0u32, 0.0);
    let mut longest_run: Option<&RingWorkout> = None;
    let mut longest_workout: Option<&RingWorkout> = None;
    let mut stars = Vec::with_capacity(workouts.len());
    for w in workouts {
        active.insert(w.date);
        months[w.date.month0() as usize] += 1;
        let is_run = RUN_TYPES.contains(&w.kind.as_str());
        if let Some(seconds) = w.seconds {
            duration_s += seconds;
            *weeks[week_slot(w.date)].get_or_insert(0.0) += seconds as f64 / 60.0;
            if longest_workout.is_none_or(|l| l.seconds.is_none_or(|s| seconds > s)) {
                longest_workout = Some(w);
            }
        }
        if let Some(meters) = w.distance_m {
            distance_m += meters;
            if is_run {
                run_distance_m += meters;
                if longest_run.is_none_or(|l| l.distance_m.is_none_or(|m| meters > m)) {
                    longest_run = Some(w);
                }
            }
        }
        if is_run {
            runs += 1;
        }
        stars.push(RingStar {
            workout_id: w.workout_id.clone(),
            date: w.date.to_string(),
            kind: w.kind.clone(),
            is_run,
            distance_m: w.distance_m,
            duration_s: w.seconds,
        });
    }
    let busiest = months
        .iter()
        .enumerate()
        .filter(|(_, n)| **n > 0)
        .max_by(|a, b| a.1.cmp(b.1).then(b.0.cmp(&a.0)));

    let steps: Vec<i64> = extras
        .steps
        .iter()
        .filter(|(d, _)| in_year(d))
        .map(|(_, v)| *v)
        .collect();
    let sleep: Vec<i64> = extras
        .sleep
        .iter()
        .filter(|(d, _)| in_year(d))
        .map(|(_, v)| *v)
        .collect();
    let mut races: Vec<RingRace> = extras
        .races
        .iter()
        .filter(|(_, d, _)| in_year(d))
        .map(|(name, date, seconds)| RingRace {
            name: name.clone(),
            date: date.to_string(),
            result_seconds: *seconds,
        })
        .collect();
    races.sort_by(|a, b| a.date.cmp(&b.date));

    RingYear {
        year,
        in_progress: year >= today.year(),
        data_days: covered.len() as u32,
        workouts: workouts.len() as u32,
        active_days: active.len() as u32,
        duration_s,
        distance_m,
        runs,
        run_distance_m,
        weeks,
        busiest_month: busiest.map(|(i, _)| i as u32 + 1),
        busiest_month_workouts: busiest.map_or(0, |(_, n)| *n),
        longest_run: longest_run.map(highlight),
        longest_workout: longest_workout.map(highlight),
        steps_total: (!steps.is_empty()).then(|| steps.iter().sum()),
        steps_days: steps.len() as u32,
        sleep_avg_minutes: (!sleep.is_empty())
            .then(|| sleep.iter().sum::<i64>() as f64 / sleep.len() as f64),
        sleep_nights: sleep.len() as u32,
        personal_records: extras
            .records
            .iter()
            .filter(|(_, d)| in_year(d))
            .map(|(key, _)| *key)
            .collect(),
        races,
        stars,
    }
}

/// Pure function: one ring per year from the first year with data to this year. Empty when there is no data at all.
pub fn build_rings(workouts: &[RingWorkout], extras: &RingExtras, today: NaiveDate) -> AnnualRings {
    let first = workouts
        .iter()
        .map(|w| w.date)
        .chain(extras.data_days.iter().copied())
        .filter(|d| *d <= today)
        .min();
    let mut by_year: BTreeMap<i32, Vec<&RingWorkout>> = BTreeMap::new();
    for w in workouts.iter().filter(|w| w.date <= today) {
        by_year.entry(w.date.year()).or_default().push(w);
    }
    let years = first
        .map(|first| {
            (first.year()..=today.year())
                .map(|year| {
                    let list = by_year.get(&year).map(Vec::as_slice).unwrap_or_default();
                    ring_year(year, today, list, extras)
                })
                .collect()
        })
        .unwrap_or_default();
    AnnualRings {
        years,
        today: today.to_string(),
    }
}

fn day(value: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(value, "%Y-%m-%d").ok()
}

impl Database {
    pub fn annual_rings(&self) -> Result<AnnualRings> {
        let mut statement = self.conn.prepare(
            "SELECT workout_id, date(start_time, 'localtime'),
                    COALESCE(workout_type_override, workout_type), distance_meters,
                    CAST(ROUND((julianday(end_time) - julianday(start_time)) * 86400) AS INTEGER)
               FROM workouts_visible ORDER BY start_time",
        )?;
        let workouts = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<f64>>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                ))
            })?
            .filter_map(|row| row.ok())
            .filter_map(|(workout_id, date, kind, distance_m, seconds)| {
                Some(RingWorkout {
                    workout_id,
                    date: day(date.as_deref()?)?,
                    kind,
                    distance_m: distance_m.filter(|m| m.is_finite() && *m > 0.0),
                    seconds: seconds.filter(|s| *s > 0 && *s <= MAX_WORKOUT_SECONDS),
                })
            })
            .collect::<Vec<_>>();

        let mut extras = RingExtras::default();
        let mut statement = self.conn.prepare(
            "SELECT DISTINCT date FROM daily_metrics
             UNION SELECT DISTINCT date(end_time, 'localtime') FROM sleep_sessions",
        )?;
        extras.data_days = statement
            .query_map([], |row| row.get::<_, Option<String>>(0))?
            .filter_map(|row| row.ok().flatten())
            .filter_map(|d| day(&d))
            .collect();
        let mut statement = self.conn.prepare(
            // Several rows on one day: the merged value first, otherwise the largest. The same rule as the home card and charts.
            "SELECT date, CAST(COALESCE(MAX(CASE WHEN source_scope = 'user_fused' THEN value END),
                                        MAX(value)) AS INTEGER)
               FROM daily_metrics WHERE metric = 'steps' AND value > 0 GROUP BY date",
        )?;
        extras.steps = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })?
            .filter_map(|row| row.ok())
            .filter_map(|(d, v)| Some((day(&d)?, v)))
            .collect();
        let mut statement = self.conn.prepare(
            "SELECT date(end_time, 'localtime'), MAX(duration_minutes) FROM sleep_sessions
              WHERE duration_minutes > 0 GROUP BY date(end_time, 'localtime')",
        )?;
        extras.sleep = statement
            .query_map([], |row| {
                Ok((row.get::<_, Option<String>>(0)?, row.get::<_, i64>(1)?))
            })?
            .filter_map(|row| row.ok())
            .filter_map(|(d, v)| Some((day(d.as_deref()?)?, v)))
            .collect();
        extras.records = self
            .personal_records()?
            .records
            .into_iter()
            .filter_map(|row| Some((row.key, day(&row.best?.date)?)))
            .collect();
        extras.races = self
            .list_races()?
            .into_iter()
            .filter_map(|race| {
                let input = race.input;
                Some((input.name, day(&input.race_date)?, input.result_seconds?))
            })
            .collect();

        Ok(build_rings(&workouts, &extras, Local::now().date_naive()))
    }
}

#[cfg(test)]
mod tests;
