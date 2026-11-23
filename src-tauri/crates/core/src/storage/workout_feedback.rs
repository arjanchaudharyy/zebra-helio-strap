//! Post-run rating: after a plan pushed to the watch is done, ask once "How did it feel?".
//!
//! Rules:
//!
//! - **Ask only for a match**: same day, same broad type, and the plan is the one the ledger shows as really delivered (the [`Database::plan_adherence`]
//!   definition). Workouts without a plan get no question: with nothing to compare, the question is only a check-in.
//! - **Do not nag**: stop asking 48 hours after the workout ends; at most one card at a time; "Not this week" lasts until next Monday;
//!   a dismissed workout is not asked again. No streaks, no "days missed" count.
//! - **Compare only what can be compared**: a plan written by time compares duration, by distance compares distance, a mixed plan gives no ratio. No pace estimate.
//! - The rating belongs to the user: no foreign key to `workouts`, and replay and retention cleanup never touch it.
use super::Database;
use crate::models::{error::Result, ZeppBridgeError};
use crate::storage::plan_adherence::PlannedDay;
use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, Utc};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// How long after the workout ends the question still shows.
pub const FEEDBACK_WINDOW_HOURS: i64 = 48;
/// Tags the user can add. The UI translates the codes; the backend produces no display text.
pub const FEEDBACK_TAGS: [&str; 3] = ["fatigue", "pain", "weather"];
/// Maximum note length in characters.
pub const FEEDBACK_NOTE_CHARS: usize = 500;
/// Last day of "Not this week" (exclusive).
const SNOOZE_KEY: &str = "workout_feedback_snooze_until";
/// Adjustments made because of a rating.
pub const ADJUSTMENTS: [&str; 1] = ["lighter_10"];

/// Codes for the five levels, for MCP and exports (the database stores 1-5).
pub fn rating_code(rating: u8) -> &'static str {
    match rating {
        1 => "too_easy",
        2 => "easy",
        3 => "right",
        4 => "hard",
        _ => "too_hard",
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct FeedbackActual {
    pub workout_id: String,
    pub sport: String,
    pub start_time: String,
    pub end_time: String,
    /// Workout duration (seconds): moving time when present, else end minus start.
    pub seconds: Option<u64>,
    pub meters: Option<f64>,
    pub avg_hr: Option<u16>,
}

/// Everything the light card on the overview needs.
#[derive(Debug, Clone, Serialize)]
pub struct FeedbackPrompt {
    /// Local date.
    pub date: String,
    pub planned: PlannedDay,
    pub actual: FeedbackActual,
    /// Actual / planned. `None`: the two sides have no comparable measure in common.
    pub completion: Option<f64>,
    /// `duration` / `distance`.
    pub completion_basis: Option<&'static str>,
    /// After this time the question no longer shows (workout end + 48 hours).
    pub expires_at: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FeedbackInput {
    pub workout_id: String,
    pub rating: u8,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkoutFeedback {
    pub workout_id: String,
    pub plan_workout_id: Option<i64>,
    /// `rated` / `dismissed`.
    pub state: String,
    pub rating: Option<u8>,
    pub tags: Vec<String>,
    pub note: Option<String>,
    pub adjustment: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    /// The fields below come from the workout table and the plan ledger. They are empty when the workout row was removed.
    pub start_time: Option<String>,
    pub sport: Option<String>,
    pub plan_name: Option<String>,
    pub plan_date: Option<String>,
}

fn invalid(message: &str) -> ZeppBridgeError {
    ZeppBridgeError::ConfigError(message.into())
}

fn parse_time(text: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|t| t.with_timezone(&Utc))
}

/// Next Monday (local calendar). "Not this week" lasts until that day.
pub fn next_monday(today: NaiveDate) -> NaiveDate {
    today + Duration::days(7 - i64::from(today.weekday().num_days_from_monday()))
}

pub fn completion(
    planned: &PlannedDay,
    actual: &FeedbackActual,
) -> (Option<f64>, Option<&'static str>) {
    if let (Some(plan), Some(done)) = (planned.seconds, actual.seconds) {
        if plan > 0 {
            return (Some(done as f64 / plan as f64), Some("duration"));
        }
    }
    if let (Some(plan), Some(done)) = (planned.meters, actual.meters) {
        if plan > 0 && done.is_finite() && done >= 0.0 {
            return (Some(done / plan as f64), Some("distance"));
        }
    }
    (None, None)
}

impl Database {
    fn feedback_snoozed(&self, today: NaiveDate) -> Result<bool> {
        Ok(self
            .get_app_meta(SNOOZE_KEY)?
            .and_then(|text| NaiveDate::parse_from_str(&text, "%Y-%m-%d").ok())
            .is_some_and(|until| today < until))
    }

    /// Which workout to ask about now: the latest one that ended within 48 hours, matches a plan, and is not rated or dismissed.
    /// At most one. None during "Not this week".
    pub fn workout_feedback_prompt(&self, now: DateTime<Utc>) -> Result<Option<FeedbackPrompt>> {
        let today = now.with_timezone(&Local).date_naive();
        if self.feedback_snoozed(today)? {
            return Ok(None);
        }
        let since = now - Duration::hours(FEEDBACK_WINDOW_HOURS);
        let mut statement = self.conn.prepare(
            "SELECT w.workout_id, date(w.start_time,'localtime'), w.start_time, w.end_time,
                    COALESCE(w.workout_type_override, w.workout_type), w.moving_seconds,
                    w.distance_meters, w.avg_hr
               FROM workouts_visible w
              WHERE w.end_time >= ?1 AND w.start_time <= ?2
                AND NOT EXISTS (SELECT 1 FROM workout_feedback f WHERE f.workout_id = w.workout_id)
              ORDER BY w.end_time DESC LIMIT 20",
        )?;
        let rows = statement
            .query_map(params![since.to_rfc3339(), now.to_rfc3339()], |row| {
                Ok((
                    row.get::<_, String>(1)?,
                    FeedbackActual {
                        workout_id: row.get(0)?,
                        start_time: row.get(2)?,
                        end_time: row.get(3)?,
                        sport: row.get(4)?,
                        seconds: row.get::<_, Option<i64>>(5)?.map(|v| v.max(0) as u64),
                        meters: row.get(6)?,
                        avg_hr: row
                            .get::<_, Option<i64>>(7)?
                            .and_then(|v| u16::try_from(v).ok()),
                    },
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let mut plans: HashMap<String, Option<PlannedDay>> = HashMap::new();
        for (date, mut actual) in rows {
            let Some(end) = parse_time(&actual.end_time) else {
                continue;
            };
            // The string comparison filtered once. Rows with a different time zone format are checked again here by instant.
            if end < since || end > now + Duration::minutes(5) {
                continue;
            }
            if !plans.contains_key(&date) {
                let Ok(day) = NaiveDate::parse_from_str(&date, "%Y-%m-%d") else {
                    continue;
                };
                let planned = self
                    .plan_adherence(day, day)?
                    .into_iter()
                    .next()
                    .and_then(|row| row.planned);
                plans.insert(date.clone(), planned);
            }
            let Some(Some(planned)) = plans.get(&date) else {
                continue;
            };
            if !super::plan_adherence::compatible_sport(planned.sport, &actual.sport) {
                continue;
            }
            if actual.seconds.is_none() {
                actual.seconds = parse_time(&actual.start_time)
                    .map(|start| (end - start).num_seconds())
                    .filter(|s| *s > 0)
                    .map(|s| s as u64);
            }
            let (ratio, basis) = completion(planned, &actual);
            return Ok(Some(FeedbackPrompt {
                date,
                planned: planned.clone(),
                completion: ratio,
                completion_basis: basis,
                expires_at: (end + Duration::hours(FEEDBACK_WINDOW_HOURS)).to_rfc3339(),
                actual,
            }));
        }
        Ok(None)
    }

    /// The plan row for this workout's day and broad type (the backend finds it itself at rating time and does not trust an id from the UI).
    fn plan_for_workout(&self, workout_id: &str) -> Result<Option<i64>> {
        let row: Option<(String, String)> = self
            .conn
            .query_row(
                "SELECT date(start_time,'localtime'), COALESCE(workout_type_override, workout_type)
                   FROM workouts_visible WHERE workout_id = ?1",
                [workout_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((date, sport)) = row else {
            return Err(invalid("Workout not found"));
        };
        let Ok(day) = NaiveDate::parse_from_str(&date, "%Y-%m-%d") else {
            return Ok(None);
        };
        Ok(self
            .plan_adherence(day, day)?
            .into_iter()
            .next()
            .and_then(|row| row.planned)
            .filter(|planned| super::plan_adherence::compatible_sport(planned.sport, &sport))
            .map(|planned| planned.plan_workout_id))
    }

    /// Rate (or change the rating). Tags accept only known codes. The note is trimmed, and an empty note is not stored.
    pub fn save_workout_feedback(&self, input: &FeedbackInput) -> Result<()> {
        if !(1..=5).contains(&input.rating) {
            return Err(invalid("Rating must be 1-5"));
        }
        let mut tags: Vec<String> = Vec::new();
        for tag in &input.tags {
            if !FEEDBACK_TAGS.contains(&tag.as_str()) {
                return Err(invalid("Unknown tag"));
            }
            if !tags.contains(tag) {
                tags.push(tag.clone());
            }
        }
        let note = input
            .note
            .as_deref()
            .map(str::trim)
            .filter(|text| !text.is_empty());
        if note.is_some_and(|text| text.chars().count() > FEEDBACK_NOTE_CHARS) {
            return Err(invalid("Note is too long"));
        }
        let plan = self.plan_for_workout(&input.workout_id)?;
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO workout_feedback
                (workout_id, plan_workout_id, state, rating, tags, note, created_at, updated_at)
             VALUES(?1, ?2, 'rated', ?3, ?4, ?5, ?6, ?6)
             ON CONFLICT(workout_id) DO UPDATE SET
                plan_workout_id = COALESCE(workout_feedback.plan_workout_id, excluded.plan_workout_id),
                state = 'rated', rating = excluded.rating, tags = excluded.tags,
                note = excluded.note, updated_at = excluded.updated_at",
            params![
                input.workout_id,
                plan,
                i64::from(input.rating),
                serde_json::to_string(&tags)?,
                note,
                now
            ],
        )?;
        Ok(())
    }

    /// Dismiss without a rating: this workout is not asked again. A workout already rated does not change.
    pub fn dismiss_workout_feedback(&self, workout_id: &str) -> Result<()> {
        let plan = self.plan_for_workout(workout_id)?;
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO workout_feedback(workout_id, plan_workout_id, state, created_at, updated_at)
             VALUES(?1, ?2, 'dismissed', ?3, ?3)
             ON CONFLICT(workout_id) DO NOTHING",
            params![workout_id, plan, now],
        )?;
        Ok(())
    }

    /// Record what this rating changed in the plan (for now only "next of the same type 10% lighter", as a draft).
    pub fn mark_feedback_adjustment(&self, workout_id: &str, adjustment: &str) -> Result<()> {
        if !ADJUSTMENTS.contains(&adjustment) {
            return Err(invalid("Unknown adjustment"));
        }
        let changed = self.conn.execute(
            "UPDATE workout_feedback SET adjustment = ?2, updated_at = ?3
              WHERE workout_id = ?1 AND state = 'rated'",
            params![workout_id, adjustment, Utc::now().to_rfc3339()],
        )?;
        if changed == 0 {
            return Err(invalid("This workout has no rating yet"));
        }
        Ok(())
    }

    /// "Not this week": until next Monday. Returns the end date.
    pub fn snooze_workout_feedback(&self, today: NaiveDate) -> Result<NaiveDate> {
        let until = next_monday(today);
        self.set_app_meta(SNOOZE_KEY, &until.to_string())?;
        Ok(until)
    }

    pub fn workout_feedback(&self, workout_id: &str) -> Result<Option<WorkoutFeedback>> {
        Ok(self
            .workout_feedback_rows(Some(workout_id), false, 1)?
            .into_iter()
            .next())
    }

    /// Ratings given, newest first. Does not include "dismissed without a rating".
    pub fn workout_feedback_history(&self, limit: usize) -> Result<Vec<WorkoutFeedback>> {
        self.workout_feedback_rows(None, true, limit.clamp(1, 500))
    }

    fn workout_feedback_rows(
        &self,
        workout_id: Option<&str>,
        rated_only: bool,
        limit: usize,
    ) -> Result<Vec<WorkoutFeedback>> {
        let mut statement = self.conn.prepare(
            "SELECT f.workout_id, f.plan_workout_id, f.state, f.rating, f.tags, f.note,
                    f.adjustment, f.created_at, f.updated_at, w.start_time,
                    COALESCE(w.workout_type_override, w.workout_type), p.workout, p.workout_date
               FROM workout_feedback f
               LEFT JOIN workouts_visible w ON w.workout_id = f.workout_id
               LEFT JOIN training_plan_workouts p ON p.id = f.plan_workout_id
              WHERE (?1 IS NULL OR f.workout_id = ?1) AND (?2 = 0 OR f.state = 'rated')
                AND NOT EXISTS (SELECT 1 FROM workout_local_state s
                                WHERE s.workout_id = f.workout_id AND s.hidden = 1)
              ORDER BY COALESCE(w.start_time, f.created_at) DESC LIMIT ?3",
        )?;
        let rows = statement.query_map(
            params![workout_id, i64::from(rated_only), limit as i64],
            |row| {
                let plan: Option<String> = row.get(11)?;
                Ok(WorkoutFeedback {
                    workout_id: row.get(0)?,
                    plan_workout_id: row.get(1)?,
                    state: row.get(2)?,
                    rating: row.get::<_, Option<i64>>(3)?.map(|v| v as u8),
                    tags: serde_json::from_str(&row.get::<_, String>(4)?).unwrap_or_default(),
                    note: row.get(5)?,
                    adjustment: row.get(6)?,
                    created_at: row.get(7)?,
                    updated_at: row.get(8)?,
                    start_time: row.get(9)?,
                    sport: row.get(10)?,
                    plan_name: plan
                        .and_then(|text| {
                            serde_json::from_str::<crate::training_plan::Workout>(&text).ok()
                        })
                        .map(|workout| workout.name),
                    plan_date: row.get(12)?,
                })
            },
        )?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }
}
