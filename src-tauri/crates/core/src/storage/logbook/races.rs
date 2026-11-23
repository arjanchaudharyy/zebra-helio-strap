//! Race calendar and medal wall: races the user signed up for (countdown, input for AI plans) and finished races (results go on the medal wall).
//!
//! The user enters the result (the official result wins). When race day has passed and no result exists, we offer a "this is the race" candidate:
//! the workout of the same kind that day with the closest distance, timed from start to end. It is only a suggestion. We store it only when the user clicks.
use super::{clean_text, invalid, valid_date};
use crate::models::error::Result;
use crate::storage::Database;
use chrono::{Local, Utc};
use rusqlite::params;
use serde::{Deserialize, Serialize};

pub const RACE_SPORTS: [&str; 6] = [
    "running",
    "trail",
    "cycling",
    "swimming",
    "triathlon",
    "other",
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RaceInput {
    pub id: Option<i64>,
    pub name: String,
    pub race_date: String,
    pub sport: String,
    #[serde(default)]
    pub distance_m: Option<f64>,
    #[serde(default)]
    pub goal_seconds: Option<i64>,
    /// `a` main goal / `b` / `c` practice race.
    pub priority: String,
    #[serde(default)]
    pub result_seconds: Option<i64>,
    #[serde(default)]
    pub workout_id: Option<String>,
    #[serde(default)]
    pub notes: String,
}

/// A workout on race day that may be this race.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct RaceCandidate {
    pub workout_id: String,
    pub start_time: String,
    pub elapsed_seconds: i64,
    pub distance_m: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Race {
    #[serde(flatten)]
    pub input: RaceInput,
    pub created_at: String,
    pub updated_at: String,
    pub candidate: Option<RaceCandidate>,
}

/// Which workout types match a race event.
fn sport_matches(race_sport: &str, workout_type: &str) -> bool {
    match race_sport {
        "running" => matches!(workout_type, "run" | "treadmill" | "trail_running"),
        "trail" => matches!(workout_type, "trail_running" | "run" | "hiking"),
        "cycling" => workout_type.contains("ride") || workout_type.contains("cycl"),
        "swimming" => workout_type.contains("swim"),
        _ => true,
    }
}

impl Database {
    /// All races, sorted by date (oldest first). A race past its date with no result carries a candidate.
    pub fn list_races(&self) -> Result<Vec<Race>> {
        let mut statement = self.conn.prepare(
            "SELECT id, name, race_date, sport, distance_m, goal_seconds, priority, result_seconds,
                    workout_id, notes, created_at, updated_at
               FROM races ORDER BY race_date, id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(Race {
                input: RaceInput {
                    id: Some(row.get(0)?),
                    name: row.get(1)?,
                    race_date: row.get(2)?,
                    sport: row.get(3)?,
                    distance_m: row.get(4)?,
                    goal_seconds: row.get(5)?,
                    priority: row.get(6)?,
                    result_seconds: row.get(7)?,
                    workout_id: row.get(8)?,
                    notes: row.get(9)?,
                },
                created_at: row.get(10)?,
                updated_at: row.get(11)?,
                candidate: None,
            })
        })?;
        let mut races = rows.collect::<std::result::Result<Vec<_>, _>>()?;
        let today = Local::now().date_naive().to_string();
        for race in &mut races {
            if race.input.race_date <= today
                && race.input.result_seconds.is_none()
                && race.input.workout_id.is_none()
            {
                race.candidate = self.race_candidate(&race.input)?;
            }
        }
        Ok(races)
    }

    fn race_candidate(&self, race: &RaceInput) -> Result<Option<RaceCandidate>> {
        let mut statement = self.conn.prepare(
            "SELECT workout_id, COALESCE(workout_type_override, workout_type), start_time,
                    CAST(ROUND((julianday(end_time) - julianday(start_time)) * 86400) AS INTEGER),
                    distance_meters
               FROM workouts_visible WHERE date(start_time, 'localtime') = ?1",
        )?;
        let rows = statement.query_map([&race.race_date], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, Option<f64>>(4)?,
            ))
        })?;
        let mut best: Option<(f64, RaceCandidate)> = None;
        for row in rows {
            let (workout_id, kind, start_time, elapsed_seconds, distance_m) = row?;
            if !sport_matches(&race.sport, &kind) || elapsed_seconds <= 0 {
                continue;
            }
            // With a race distance, pick the closest distance. Without one, pick the longest.
            let score = match (race.distance_m, distance_m) {
                (Some(target), Some(actual)) => (actual - target).abs(),
                (None, Some(actual)) => -actual,
                _ => f64::MAX,
            };
            if best.as_ref().is_none_or(|(s, _)| score < *s) {
                best = Some((
                    score,
                    RaceCandidate {
                        workout_id,
                        start_time,
                        elapsed_seconds,
                        distance_m,
                    },
                ));
            }
        }
        Ok(best.map(|(_, candidate)| candidate))
    }

    pub fn save_race(&self, input: &RaceInput) -> Result<i64> {
        let name = clean_text(&input.name, 120, "race name is empty or too long")?;
        let notes = input.notes.trim();
        let seconds_ok = |value: Option<i64>| value.is_none_or(|s| (1..=7 * 86_400).contains(&s));
        if !valid_date(&input.race_date)
            || !RACE_SPORTS.contains(&input.sport.as_str())
            || !["a", "b", "c"].contains(&input.priority.as_str())
            || input
                .distance_m
                .is_some_and(|m| !(m.is_finite() && (50.0..=1_000_000.0).contains(&m)))
            || !seconds_ok(input.goal_seconds)
            || !seconds_ok(input.result_seconds)
            || input
                .workout_id
                .as_deref()
                .is_some_and(|id| id.is_empty() || id.len() > 120)
            || notes.chars().count() > 2000
            || input.id.is_some_and(|id| id <= 0)
        {
            return Err(invalid("race details are not valid"));
        }
        let now = Utc::now().to_rfc3339();
        match input.id {
            Some(id) => {
                let changed = self.conn.execute(
                    "UPDATE races SET name=?1, race_date=?2, sport=?3, distance_m=?4, goal_seconds=?5, priority=?6,
                         result_seconds=?7, workout_id=?8, notes=?9, updated_at=?10 WHERE id=?11",
                    params![name, input.race_date, input.sport, input.distance_m, input.goal_seconds, input.priority,
                        input.result_seconds, input.workout_id, notes, now, id],
                )?;
                if changed == 0 {
                    return Err(invalid("race not found"));
                }
                Ok(id)
            }
            None => {
                self.conn.execute(
                    "INSERT INTO races(name, race_date, sport, distance_m, goal_seconds, priority, result_seconds,
                         workout_id, notes, created_at, updated_at)
                     VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)",
                    params![name, input.race_date, input.sport, input.distance_m, input.goal_seconds, input.priority,
                        input.result_seconds, input.workout_id, notes, now],
                )?;
                Ok(self.conn.last_insert_rowid())
            }
        }
    }

    pub fn delete_race(&self, id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM races WHERE id = ?1", [id])?;
        Ok(())
    }
}
