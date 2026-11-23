//! Injury log: where, from which day to which day, how bad. `end_date` is empty while it is not healed.
//!
//! The AI handoff gets only injuries not healed yet and those healed in the last 90 days (the AI needs them to plan; older ones stay on this machine).
use super::{clean_text, invalid, valid_body_part, valid_date};
use crate::models::error::Result;
use crate::storage::Database;
use chrono::Utc;
use rusqlite::params;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InjuryInput {
    pub id: Option<i64>,
    pub title: String,
    #[serde(default)]
    pub body_part: Option<String>,
    /// 1 mild / 2 moderate / 3 severe (affects training).
    #[serde(default)]
    pub severity: Option<u8>,
    pub start_date: String,
    #[serde(default)]
    pub end_date: Option<String>,
    #[serde(default)]
    pub notes: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Injury {
    #[serde(flatten)]
    pub input: InjuryInput,
    pub created_at: String,
    pub updated_at: String,
}

impl Database {
    /// Not healed first, then the rest by start date, newest first.
    pub fn list_injuries(&self) -> Result<Vec<Injury>> {
        let mut statement = self.conn.prepare(
            "SELECT id, title, body_part, severity, start_date, end_date, notes, created_at, updated_at
               FROM injuries ORDER BY end_date IS NOT NULL, start_date DESC, id DESC",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(Injury {
                input: InjuryInput {
                    id: Some(row.get(0)?),
                    title: row.get(1)?,
                    body_part: row.get(2)?,
                    severity: row.get(3)?,
                    start_date: row.get(4)?,
                    end_date: row.get(5)?,
                    notes: row.get(6)?,
                },
                created_at: row.get(7)?,
                updated_at: row.get(8)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn save_injury(&self, input: &InjuryInput) -> Result<i64> {
        let title = clean_text(&input.title, 120, "injury title is empty or too long")?;
        let notes = input.notes.trim();
        if input
            .body_part
            .as_deref()
            .is_some_and(|part| !valid_body_part(part))
            || input.severity.is_some_and(|s| !(1..=3).contains(&s))
            || !valid_date(&input.start_date)
            || input
                .end_date
                .as_deref()
                .is_some_and(|end| !valid_date(end) || end < input.start_date.as_str())
            || notes.chars().count() > 2000
            || input.id.is_some_and(|id| id <= 0)
        {
            return Err(invalid("the injury record content is invalid"));
        }
        let now = Utc::now().to_rfc3339();
        match input.id {
            Some(id) => {
                let changed = self.conn.execute(
                    "UPDATE injuries SET title=?1, body_part=?2, severity=?3, start_date=?4, end_date=?5,
                         notes=?6, updated_at=?7 WHERE id=?8",
                    params![title, input.body_part, input.severity, input.start_date, input.end_date, notes, now, id],
                )?;
                if changed == 0 {
                    return Err(invalid("injury record not found"));
                }
                Ok(id)
            }
            None => {
                self.conn.execute(
                    "INSERT INTO injuries(title, body_part, severity, start_date, end_date, notes, created_at, updated_at)
                     VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
                    params![title, input.body_part, input.severity, input.start_date, input.end_date, notes, now],
                )?;
                Ok(self.conn.last_insert_rowid())
            }
        }
    }

    pub fn delete_injury(&self, id: i64) -> Result<()> {
        self.conn
            .execute("DELETE FROM injuries WHERE id = ?1", [id])?;
        Ok(())
    }
}
