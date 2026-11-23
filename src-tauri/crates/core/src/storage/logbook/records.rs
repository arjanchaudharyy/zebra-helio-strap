//! PB wall: the fastest segment for 5K / 10K / half marathon / marathon, all computed on this machine.
//!
//! Each run is searched in this order, and the search stops at the first hit (`method` says which one):
//!
//! 1. `samples`: cumulative distance from per-second samples. A sliding window finds the shortest time that covers the target distance. Both window ends fall on sample points
//!    with no interpolation, so the distance can only be a little longer, never shorter. The time can only be slower, never faster than the truth. Pause time is included.
//! 2. `splits`: for old records without sample distance, 5K / 10K add up consecutive full-kilometre splits (each exactly 1000 m).
//! 3. `whole`: the whole workout distance is between the target distance and 103% of it. Use the time from start to end (common for races).
//!
//! Also, races in the race calendar where the user entered a result and the distance matches (`race`) count too. The official result is more accurate than the watch.
//!
//! Impossibly fast ones (pace faster than 2:30/km, faster than the world record) count as GPS drift. They are dropped and do not enter the wall.
use super::RUN_TYPES;
use crate::models::error::Result;
use crate::storage::Database;
use serde::Serialize;
use std::collections::HashMap;

/// (key, target distance m).
pub const TARGETS: [(&str, f64); 4] = [
    ("5k", 5000.0),
    ("10k", 10000.0),
    ("half", 21097.5),
    ("marathon", 42195.0),
];
/// A "fastest segment" faster than this pace (seconds / km) is not accepted.
const IMPOSSIBLE_PACE: f64 = 150.0;
/// How many more entries to list for each distance besides the fastest.
const RUNNERS_UP: usize = 2;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Effort {
    pub seconds: i64,
    /// `samples` / `splits` / `whole` / `race`.
    pub method: &'static str,
    pub workout_id: Option<String>,
    pub race_id: Option<i64>,
    pub race_name: Option<String>,
    /// Local date.
    pub date: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RecordRow {
    pub key: &'static str,
    pub distance_m: f64,
    pub best: Option<Effort>,
    /// The other fastest efforts (from different workouts), slower ones last.
    pub runners_up: Vec<Effort>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PersonalRecords {
    pub records: Vec<RecordRow>,
    /// How many runs were checked (long enough ones).
    pub runs_scanned: usize,
}

/// Pure function: the shortest time covering `target` metres on a cumulative distance series. `points` = (seconds, cumulative metres), sorted by time.
/// A point where the distance goes back (reset, drift) cuts the window, and the search starts again from there.
pub fn fastest_span(points: &[(i64, f64)], target: f64) -> Option<i64> {
    let mut best: Option<i64> = None;
    let mut start = 0usize;
    for end in 0..points.len() {
        if end > 0 && points[end].1 < points[end - 1].1 {
            start = end;
            continue;
        }
        // Move the start forward until "one more step would be short of the target distance".
        while start + 1 < end && points[end].1 - points[start + 1].1 >= target {
            start += 1;
        }
        if points[end].1 - points[start].1 >= target {
            let span = points[end].0 - points[start].0;
            if span > 0 && best.is_none_or(|b| span < b) {
                best = Some(span);
            }
        }
    }
    best
}

/// Pure function: the shortest total time of `count` consecutive full-kilometre splits. `splits` = (index, metres, seconds, incomplete).
pub fn fastest_splits(splits: &[(i64, f64, i64, bool)], count: usize) -> Option<i64> {
    let mut best: Option<i64> = None;
    let mut run: Vec<i64> = Vec::new();
    let mut previous: Option<i64> = None;
    for &(index, meters, seconds, partial) in splits {
        let whole_km = !partial && (meters - 1000.0).abs() <= 1.0 && seconds > 0;
        if !whole_km || previous.is_some_and(|p| index != p + 1) {
            run.clear();
        }
        previous = Some(index);
        if !whole_km {
            continue;
        }
        run.push(seconds);
        if run.len() >= count {
            let sum: i64 = run[run.len() - count..].iter().sum();
            if best.is_none_or(|b| sum < b) {
                best = Some(sum);
            }
        }
    }
    best
}

fn plausible(seconds: i64, distance: f64) -> bool {
    seconds > 0 && (seconds as f64) / (distance / 1000.0) >= IMPOSSIBLE_PACE
}

impl Database {
    pub fn personal_records(&self) -> Result<PersonalRecords> {
        // Runs that are long enough.
        let mut statement = self.conn.prepare(&format!(
            "SELECT workout_id, date(start_time, 'localtime'), distance_meters,
                    CAST(ROUND((julianday(end_time) - julianday(start_time)) * 86400) AS INTEGER)
               FROM workouts_visible
              WHERE COALESCE(workout_type_override, workout_type) IN ({})
                AND distance_meters >= ?1",
            RUN_TYPES
                .iter()
                .map(|t| format!("'{t}'"))
                .collect::<Vec<_>>()
                .join(",")
        ))?;
        let runs = statement
            .query_map([TARGETS[0].1 * 0.99], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, f64>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        let mut efforts: HashMap<&'static str, Vec<Effort>> = HashMap::new();
        let mut samples = self.conn.prepare(
            "SELECT CAST(strftime('%s', timestamp) AS INTEGER), distance_m FROM workout_samples
              WHERE workout_id = ?1 AND distance_m IS NOT NULL ORDER BY timestamp",
        )?;
        let mut splits = self.conn.prepare(
            "SELECT split_index, distance_m, duration_seconds, partial FROM workout_splits
              WHERE workout_id = ?1 ORDER BY split_index",
        )?;
        for (workout_id, date, distance, elapsed) in &runs {
            let points = samples
                .query_map([workout_id], |row| {
                    Ok((row.get::<_, i64>(0)?, row.get::<_, f64>(1)?))
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            let km_splits = if points.len() < 2 {
                splits
                    .query_map([workout_id], |row| {
                        Ok((
                            row.get::<_, i64>(0)?,
                            row.get::<_, f64>(1)?,
                            row.get::<_, i64>(2)?,
                            row.get::<_, i64>(3)? != 0,
                        ))
                    })?
                    .collect::<std::result::Result<Vec<_>, _>>()?
            } else {
                Vec::new()
            };
            for (key, target) in TARGETS {
                if *distance < target * 0.99 {
                    continue;
                }
                let found = if points.len() >= 2 {
                    fastest_span(&points, target).map(|s| (s, "samples"))
                } else if target <= 10_000.0 {
                    fastest_splits(&km_splits, (target / 1000.0) as usize).map(|s| (s, "splits"))
                } else {
                    None
                }
                .or_else(|| {
                    (*distance >= target && *distance <= target * 1.03)
                        .then_some((*elapsed, "whole"))
                });
                if let Some((seconds, method)) = found.filter(|(s, _)| plausible(*s, target)) {
                    efforts.entry(key).or_default().push(Effort {
                        seconds,
                        method,
                        workout_id: Some(workout_id.clone()),
                        race_id: None,
                        race_name: None,
                        date: date.clone(),
                    });
                }
            }
        }

        // Races where the user entered a result: count for this distance only when the distance is within +/-1% of the target.
        for race in self.list_races()? {
            let (Some(distance), Some(seconds)) =
                (race.input.distance_m, race.input.result_seconds)
            else {
                continue;
            };
            for (key, target) in TARGETS {
                if (distance - target).abs() <= target * 0.01 && plausible(seconds, target) {
                    let list = efforts.entry(key).or_default();
                    // The workout matched to a race uses the race result and is not listed again.
                    if let Some(id) = race.input.workout_id.as_deref() {
                        list.retain(|e| e.workout_id.as_deref() != Some(id));
                    }
                    list.push(Effort {
                        seconds,
                        method: "race",
                        workout_id: race.input.workout_id.clone(),
                        race_id: race.input.id,
                        race_name: Some(race.input.name.clone()),
                        date: race.input.race_date.clone(),
                    });
                }
            }
        }

        let records = TARGETS
            .iter()
            .map(|&(key, distance_m)| {
                let mut list = efforts.remove(key).unwrap_or_default();
                list.sort_by(|a, b| a.seconds.cmp(&b.seconds).then_with(|| a.date.cmp(&b.date)));
                let mut list = list.into_iter();
                let best = list.next();
                RecordRow {
                    key,
                    distance_m,
                    best,
                    runners_up: list.take(RUNNERS_UP).collect(),
                }
            })
            .collect();
        Ok(PersonalRecords {
            records,
            runs_scanned: runs.len(),
        })
    }
}
