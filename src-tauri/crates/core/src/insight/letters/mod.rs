//! Data letters in the letter box: the weekly data letter, the time capsule, and "on this day".
//!
//! All three only state facts. They generate no natural language and compare with no population, the same rule as the local weekly report.
//! The weekly letter and the capsule are snapshots of the moment they were written, stored in `data_letters` (v44). A later backfill or reparse
//! that changes derived data does not change old letters. "On this day" is computed live, and its data is all on this device.
//!
//! Metrics the user excludes ([`crate::storage::metric_prefs`]) are removed on read, not on write:
//! when the user includes them in analysis again, that item comes back in old letters.

use super::*;
use crate::models::error::ZeppBridgeError;
use chrono::Datelike;
use rusqlite::{params, OptionalExtension};

/// The limit for the note to the future in a capsule. It is the same as for a reply.
pub const CAPSULE_NOTE_MAX_CHARS: usize = 1000;
/// The longest time a capsule can stay sealed.
pub const CAPSULE_MAX_YEARS: i32 = 10;
/// The capsule snapshot averages this many days before the day it is written.
pub const SNAPSHOT_DAYS: i64 = 28;
/// The maximum number of years "on this day" looks back.
pub const ON_THIS_DAY_MAX_YEARS: i32 = 10;

/// The items in a capsule snapshot: one copy when written and one when opened, shown side by side.
/// `workout_count` is folded per window into "workouts per 7 days" (the same method as the weekly report).
const SNAPSHOT_METRICS: [(&str, &str); 6] = [
    ("resting_hr", "bpm"),
    ("hrv", "ms"),
    ("sleep_duration", "min"),
    ("steps", "steps"),
    ("workout_count", "per_week"),
    ("training_load", "load"),
];

const ON_THIS_DAY_METRICS: [(&str, &str); 3] = [
    ("steps", "steps"),
    ("sleep_duration", "min"),
    ("resting_hr", "bpm"),
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SnapshotValue {
    pub metric: String,
    pub unit: String,
    /// The average of the days with data in the window. `None` when there is none, not 0.
    pub value: Option<f64>,
    pub days: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LetterSnapshot {
    pub start: String,
    pub end: String,
    pub values: Vec<SnapshotValue>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct CapsuleBody {
    written: Option<LetterSnapshot>,
    #[serde(default)]
    opened: Option<LetterSnapshot>,
}

/// One data letter in the letter box. Before its date, a capsule's `note` is `None`: the text is in the database, but the UI cannot get it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DataLetter {
    pub id: i64,
    pub kind: String,
    pub written_at: String,
    pub open_on: Option<String>,
    pub opened_at: Option<String>,
    pub sealed: bool,
    pub note: Option<String>,
    pub weekly: Option<WeeklyReport>,
    pub written_snapshot: Option<LetterSnapshot>,
    pub opened_snapshot: Option<LetterSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OnThisDayWorkout {
    pub workout_id: String,
    pub workout_type: String,
    pub start_time: String,
    pub distance_meters: Option<f64>,
    pub duration_seconds: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OnThisDayYear {
    pub date: String,
    pub years_ago: i32,
    pub values: Vec<SnapshotValue>,
    pub workouts: Vec<OnThisDayWorkout>,
}

/// The two things the letter reminder checks: the latest weekly letter, and capsules that are due but still sealed.
/// "Read or not" is not decided here. The UI side stores the read state of weekly letters; this only reports facts.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MailStatus {
    pub latest_weekly_id: Option<i64>,
    pub due_capsule_ids: Vec<i64>,
}

/// The last day of the most recent completed week (Monday to Sunday). On a Sunday that week is not complete yet, so use the previous Sunday.
pub fn last_completed_week_end(today: NaiveDate) -> NaiveDate {
    let back = match today.weekday().num_days_from_sunday() {
        0 => 7,
        n => n,
    };
    today - Duration::days(i64::from(back))
}

/// The same day a number of years back. February 29 falls on February 28 in a non-leap year.
fn same_day_years_ago(today: NaiveDate, years: i32) -> Option<NaiveDate> {
    let year = today.year() - years;
    today
        .with_year(year)
        .or_else(|| NaiveDate::from_ymd_opt(year, today.month(), 28))
}

fn invalid(message: &str) -> ZeppBridgeError {
    ZeppBridgeError::ConfigError(message.into())
}

impl Database {
    /// If the letter for last week is not sent yet, send it. If that week has no data at all, do not send (there are no facts to state).
    /// Returns whether a letter was sent.
    pub fn ensure_weekly_letter(&self, today: NaiveDate, now: DateTime<Utc>) -> Result<bool> {
        let week_end = last_completed_week_end(today);
        let key = format!("weekly:{week_end}");
        let exists: bool = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM data_letters WHERE period_key = ?1)",
            [&key],
            |row| row.get(0),
        )?;
        if exists {
            return Ok(false);
        }
        let report = self.weekly_report_for_day(week_end, now)?;
        if report.facts.iter().all(|fact| fact.value.is_none()) {
            return Ok(false);
        }
        self.conn.execute(
            "INSERT OR IGNORE INTO data_letters(kind, period_key, written_at, body)
             VALUES('weekly', ?1, ?2, ?3)",
            params![key, now.to_rfc3339(), serde_json::to_string(&report)?],
        )?;
        Ok(true)
    }

    /// Write a time capsule: a note to the future + a snapshot of the 28 days before the day it is written. It cannot open before `open_on`.
    pub fn seal_capsule(
        &self,
        note: &str,
        open_on: NaiveDate,
        today: NaiveDate,
        now: DateTime<Utc>,
    ) -> Result<i64> {
        let note = note.trim();
        if note.is_empty() || note.chars().count() > CAPSULE_NOTE_MAX_CHARS {
            return Err(invalid("The capsule note is empty or too long"));
        }
        let latest = same_day_years_ago(today, -CAPSULE_MAX_YEARS).unwrap_or(today);
        if open_on <= today || open_on > latest {
            return Err(invalid("The open date must be after today and within ten years"));
        }
        let body = CapsuleBody {
            written: Some(self.letter_snapshot(today)?),
            opened: None,
        };
        self.conn.execute(
            "INSERT INTO data_letters(kind, written_at, open_on, note, body)
             VALUES('capsule', ?1, ?2, ?3, ?4)",
            params![
                now.to_rfc3339(),
                open_on.to_string(),
                note,
                serde_json::to_string(&body)?
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Open a due capsule: record the open time and a snapshot of that moment. An already opened capsule returns unchanged.
    pub fn open_capsule(
        &self,
        id: i64,
        today: NaiveDate,
        now: DateTime<Utc>,
    ) -> Result<DataLetter> {
        let row: Option<(Option<String>, Option<String>, String)> = self
            .conn
            .query_row(
                "SELECT open_on, opened_at, body FROM data_letters WHERE id = ?1 AND kind = 'capsule'",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        let Some((open_on, opened_at, body)) = row else {
            return Err(invalid("No such capsule"));
        };
        if opened_at.is_none() {
            let due = open_on
                .as_deref()
                .and_then(|date| NaiveDate::parse_from_str(date, "%Y-%m-%d").ok())
                .is_some_and(|date| date <= today);
            if !due {
                return Err(invalid("The capsule is not due to open yet"));
            }
            let mut body: CapsuleBody = serde_json::from_str(&body).unwrap_or_default();
            body.opened = Some(self.letter_snapshot(today)?);
            self.conn.execute(
                "UPDATE data_letters SET opened_at = ?2, body = ?3 WHERE id = ?1",
                params![id, now.to_rfc3339(), serde_json::to_string(&body)?],
            )?;
        }
        self.data_letters()?
            .into_iter()
            .find(|letter| letter.id == id)
            .ok_or_else(|| invalid("No such capsule"))
    }

    /// The state for the letter reminder: the id of the latest weekly letter, and the capsules that are due but still sealed (by open date, earliest first).
    /// It does not read the text. A sealed capsule gives no text at all.
    pub fn data_letter_mail_status(&self, today: NaiveDate) -> Result<MailStatus> {
        let latest_weekly_id = self
            .conn
            .query_row(
                "SELECT id FROM data_letters WHERE kind = 'weekly'
                 ORDER BY written_at DESC, id DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .optional()?;
        let mut stmt = self.conn.prepare(
            "SELECT id FROM data_letters
             WHERE kind = 'capsule' AND opened_at IS NULL
               AND open_on IS NOT NULL AND open_on <= ?1
             ORDER BY open_on, id",
        )?;
        let due_capsule_ids = stmt
            .query_map([today.to_string()], |row| row.get(0))?
            .collect::<std::result::Result<_, _>>()?;
        Ok(MailStatus {
            latest_weekly_id,
            due_capsule_ids,
        })
    }

    /// All data letters, newest first, without the metrics the user excludes.
    pub fn data_letters(&self) -> Result<Vec<DataLetter>> {
        let excluded = self.excluded_metrics()?;
        let keep = |metric: &str| !excluded.iter().any(|item| item == metric);
        let mut stmt = self.conn.prepare(
            "SELECT id, kind, written_at, open_on, opened_at, note, body
             FROM data_letters ORDER BY written_at DESC, id DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
            ))
        })?;
        let mut letters = Vec::new();
        for row in rows {
            let (id, kind, written_at, open_on, opened_at, note, body) = row?;
            let mut letter = DataLetter {
                id,
                kind: kind.clone(),
                written_at,
                open_on: open_on.clone(),
                opened_at: opened_at.clone(),
                sealed: false,
                note: None,
                weekly: None,
                written_snapshot: None,
                opened_snapshot: None,
            };
            if kind == "weekly" {
                let Ok(mut report) = serde_json::from_str::<WeeklyReport>(&body) else {
                    continue;
                };
                report.facts.retain(|fact| keep(&fact.metric));
                letter.weekly = Some(report);
            } else {
                let body: CapsuleBody = serde_json::from_str(&body).unwrap_or_default();
                let strip = |snapshot: Option<LetterSnapshot>| {
                    snapshot.map(|mut snapshot| {
                        snapshot.values.retain(|value| keep(&value.metric));
                        snapshot
                    })
                };
                letter.sealed = opened_at.is_none();
                // A sealed capsule only tells the UI "sealed, and the day it can open". It gives no note and no snapshot.
                if !letter.sealed {
                    letter.note = Some(note);
                    letter.written_snapshot = strip(body.written);
                    letter.opened_snapshot = strip(body.opened);
                }
            }
            letters.push(letter);
        }
        Ok(letters)
    }

    /// On this day: steps, sleep, resting heart rate and workouts on the same day in past years. Years with no data at all are not returned.
    pub fn on_this_day(&self, today: NaiveDate) -> Result<Vec<OnThisDayYear>> {
        let excluded = self.excluded_metrics()?;
        let mut years = Vec::new();
        for years_ago in 1..=ON_THIS_DAY_MAX_YEARS {
            let Some(date) = same_day_years_ago(today, years_ago) else {
                continue;
            };
            let mut values = Vec::new();
            for (metric, unit) in ON_THIS_DAY_METRICS {
                if excluded.iter().any(|item| item == metric) {
                    continue;
                }
                let samples = self.weekly_samples(metric, date, date)?;
                if let Some(value) = mean(&samples.values) {
                    values.push(SnapshotValue {
                        metric: metric.into(),
                        unit: unit.into(),
                        value: Some(round1(value)),
                        days: samples.day_count as i64,
                    });
                }
            }
            let workouts = self.workouts_on(date)?;
            if values.is_empty() && workouts.is_empty() {
                continue;
            }
            years.push(OnThisDayYear {
                date: date.to_string(),
                years_ago,
                values,
                workouts,
            });
        }
        Ok(years)
    }

    fn workouts_on(&self, date: NaiveDate) -> Result<Vec<OnThisDayWorkout>> {
        let mut stmt = self.conn.prepare(
            "SELECT workout_id, COALESCE(workout_type_override, workout_type), start_time,
                    distance_meters,
                    CAST((julianday(end_time) - julianday(start_time)) * 86400 AS INTEGER)
             FROM workouts_visible
             WHERE date(start_time, 'localtime') = ?1
             ORDER BY start_time",
        )?;
        let rows = stmt.query_map([date.to_string()], |row| {
            Ok(OnThisDayWorkout {
                workout_id: row.get(0)?,
                workout_type: row.get(1)?,
                start_time: row.get(2)?,
                distance_meters: row.get(3)?,
                duration_seconds: row.get::<_, Option<i64>>(4)?.filter(|value| *value > 0),
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// The 28-day snapshot up to and including `end`.
    fn letter_snapshot(&self, end: NaiveDate) -> Result<LetterSnapshot> {
        let start = saturating_days_before(end, SNAPSHOT_DAYS - 1);
        let mut values = Vec::new();
        for (metric, unit) in SNAPSHOT_METRICS {
            let samples = self.weekly_samples(metric, start, end)?;
            values.push(SnapshotValue {
                metric: metric.into(),
                unit: unit.into(),
                value: mean(&samples.values).map(round1),
                days: samples.day_count as i64,
            });
        }
        Ok(LetterSnapshot {
            start: start.to_string(),
            end: end.to_string(),
            values,
        })
    }
}

#[cfg(test)]
mod tests;
