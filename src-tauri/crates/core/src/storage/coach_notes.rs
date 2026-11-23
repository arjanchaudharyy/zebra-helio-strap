//! AI memory inbox: what the user teaches the AI stays on this machine and moves with them to any AI.
//!
//! Rules:
//!
//! - **What the AI writes takes effect at once**: since 3.1.2, MCP `propose_coach_note` goes to "remember" (`accepted`),
//!   is recorded in the change ledger, and the user can undo it on the desktop. `proposed` holds only proposals from before 3.1.2. The user confirms each one before it becomes
//!   `accepted`. The context given to the AI contains only `accepted` notes.
//! - **Health data is not touched**: a separate table. Writes take the cross-process write lock as usual (the caller does this).
//! - **Rejected notes stay**: so the same sentence is not proposed again and again. Only notes the user deletes are really gone.
//! - **The inbox has a cap**: at most [`MAX_PROPOSED`] pending notes. When it is full, new AI proposals are rejected,
//!   so a chatty AI cannot fill the inbox.
//! - The text is data written by the user or the AI, not instructions. It is marked that way when given to the AI.
use super::Database;
use crate::models::{error::Result, ZeppBridgeError};
use chrono::Utc;
use rusqlite::{params, OptionalExtension};
use serde::Serialize;

/// Maximum characters in one note.
pub const NOTE_MAX_CHARS: usize = 500;
/// Maximum number of pending notes.
pub const MAX_PROPOSED: i64 = 20;
/// Maximum characters in a client name (display only).
const SOURCE_NAME_MAX_CHARS: usize = 60;

#[derive(Debug, Clone, Serialize)]
pub struct CoachNote {
    pub id: i64,
    pub text: String,
    /// `proposed` / `accepted` / `rejected`.
    pub state: String,
    /// `user` / `mcp`.
    pub source: String,
    /// The name the MCP client reports for itself. Empty for notes the user wrote.
    pub source_name: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub decided_at: Option<String>,
}

/// The result of an AI proposal. Only `Proposed` wrote to the database.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "outcome")]
pub enum Proposal {
    Proposed {
        id: i64,
    },
    /// The user already remembers this sentence.
    AlreadyKnown {
        id: i64,
    },
    /// Already in the inbox, waiting for the user to confirm.
    AlreadyProposed {
        id: i64,
    },
    /// The user rejected this sentence before.
    RejectedBefore,
    /// The pending list is full.
    InboxFull,
}

fn invalid(message: &str) -> ZeppBridgeError {
    ZeppBridgeError::ConfigError(message.into())
}

/// Trim surrounding whitespace and collapse runs of whitespace into one space. Reject empty or too long text.
pub fn clean_note(text: &str) -> Result<String> {
    let clean = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if clean.is_empty() {
        return Err(invalid("The content is empty"));
    }
    if clean.chars().count() > NOTE_MAX_CHARS {
        return Err(invalid("A note can have at most 500 characters"));
    }
    Ok(clean)
}

fn clean_source_name(name: Option<&str>) -> Option<String> {
    let name: String = name?
        .chars()
        .filter(|c| !c.is_control())
        .take(SOURCE_NAME_MAX_CHARS)
        .collect();
    let name = name.trim();
    (!name.is_empty()).then(|| name.to_string())
}

const COLUMNS: &str =
    "SELECT id, text, state, source, source_name, created_at, updated_at, decided_at
                         FROM coach_notes";

fn note_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<CoachNote> {
    Ok(CoachNote {
        id: row.get(0)?,
        text: row.get(1)?,
        state: row.get(2)?,
        source: row.get(3)?,
        source_name: row.get(4)?,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
        decided_at: row.get(7)?,
    })
}

impl Database {
    /// The AI proposes to remember a sentence.
    pub fn propose_coach_note(&self, text: &str, source_name: Option<&str>) -> Result<Proposal> {
        let text = clean_note(text)?;
        let existing: Option<(i64, String)> = self
            .conn
            .query_row(
                "SELECT id, state FROM coach_notes WHERE text = ?1 ORDER BY id DESC LIMIT 1",
                [&text],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        match existing {
            Some((id, state)) if state == "accepted" => return Ok(Proposal::AlreadyKnown { id }),
            Some((id, state)) if state == "proposed" => {
                return Ok(Proposal::AlreadyProposed { id })
            }
            Some(_) => return Ok(Proposal::RejectedBefore),
            None => {}
        }
        let waiting: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM coach_notes WHERE state = 'proposed'",
            [],
            |row| row.get(0),
        )?;
        if waiting >= MAX_PROPOSED {
            return Ok(Proposal::InboxFull);
        }
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO coach_notes(text, state, source, source_name, created_at, updated_at)
             VALUES(?1, 'proposed', 'mcp', ?2, ?3, ?3)",
            params![text, clean_source_name(source_name), now],
        )?;
        Ok(Proposal::Proposed {
            id: self.conn.last_insert_rowid(),
        })
    }

    /// The user writes a note: it counts at once.
    pub fn add_coach_note(&self, text: &str) -> Result<i64> {
        let text = clean_note(text)?;
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO coach_notes(text, state, source, created_at, updated_at, decided_at)
             VALUES(?1, 'accepted', 'user', ?2, ?2, ?2)",
            params![text, now],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Confirm (remember) or reject a proposal. A decided note cannot be changed back: returns `false` (the caller reports the error by code).
    pub fn decide_coach_note(&self, id: i64, accept: bool) -> Result<bool> {
        let now = Utc::now().to_rfc3339();
        let changed = self.conn.execute(
            "UPDATE coach_notes SET state = ?2, updated_at = ?3, decided_at = ?3
              WHERE id = ?1 AND state = 'proposed'",
            params![id, if accept { "accepted" } else { "rejected" }, now],
        )?;
        Ok(changed > 0)
    }

    /// Change the text of a note (remembered or still pending). The source stays the same: an AI proposal stays marked as from the AI after an edit.
    /// Returns `false` when not found.
    pub fn edit_coach_note(&self, id: i64, text: &str) -> Result<bool> {
        let text = clean_note(text)?;
        let changed = self.conn.execute(
            "UPDATE coach_notes SET text = ?2, updated_at = ?3
              WHERE id = ?1 AND state IN ('proposed', 'accepted')",
            params![id, text, Utc::now().to_rfc3339()],
        )?;
        Ok(changed > 0)
    }

    /// Delete a note (it is the user's data; once deleted it is gone).
    pub fn delete_coach_note(&self, id: i64) -> Result<()> {
        self.conn
            .execute("DELETE FROM coach_notes WHERE id = ?1", [id])?;
        Ok(())
    }

    /// Remembered notes (the ones given to the AI), oldest first: it reads like a profile written slowly over time.
    pub fn accepted_coach_notes(&self) -> Result<Vec<CoachNote>> {
        self.coach_notes_in("accepted", "id")
    }

    /// Notes waiting for user confirmation, newest first.
    pub fn proposed_coach_notes(&self) -> Result<Vec<CoachNote>> {
        self.coach_notes_in("proposed", "id DESC")
    }

    fn coach_notes_in(&self, state: &str, order: &str) -> Result<Vec<CoachNote>> {
        let mut statement = self
            .conn
            .prepare(&format!("{COLUMNS} WHERE state = ?1 ORDER BY {order}"))?;
        let rows = statement.query_map([state], note_row)?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    /// The local AI remembers a sentence directly (since 3.1.2, MCP `propose_coach_note` no longer goes to the inbox).
    /// A sentence the user rejected before is still refused. A note still in the inbox turns into remembered.
    /// Only notes written to the database are recorded in `mcp_writes` (the caller does this), so the desktop can undo them one by one.
    pub fn remember_coach_note(&self, text: &str, source_name: Option<&str>) -> Result<Remembered> {
        let text = clean_note(text)?;
        let existing: Option<(i64, String)> = self
            .conn
            .query_row(
                "SELECT id, state FROM coach_notes WHERE text = ?1 ORDER BY id DESC LIMIT 1",
                [&text],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        match existing {
            Some((id, state)) if state == "accepted" => Ok(Remembered::AlreadyKnown { id }),
            Some((id, state)) if state == "proposed" => {
                self.decide_coach_note(id, true)?;
                Ok(Remembered::Remembered {
                    id,
                    was_proposed: true,
                })
            }
            Some(_) => Ok(Remembered::RejectedBefore),
            None => {
                let now = Utc::now().to_rfc3339();
                self.conn.execute(
                    "INSERT INTO coach_notes(text, state, source, source_name, created_at, updated_at, decided_at)
                     VALUES(?1, 'accepted', 'mcp', ?2, ?3, ?3, ?3)",
                    params![text, clean_source_name(source_name), now],
                )?;
                Ok(Remembered::Remembered {
                    id: self.conn.last_insert_rowid(),
                    was_proposed: false,
                })
            }
        }
    }

    /// The status and text of a note. `None` when not found. Undo uses it to check whether the user changed the note.
    pub fn coach_note_state(&self, id: i64) -> Result<Option<(String, String)>> {
        Ok(self
            .conn
            .query_row(
                "SELECT state, text FROM coach_notes WHERE id = ?1",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?)
    }

    /// Undo a note the AI remembered: a newly written note becomes "rejected" (so the AI cannot write the same sentence again),
    /// and a note that was in the inbox goes back to the inbox.
    pub(crate) fn set_coach_note_state(&self, id: i64, state: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE coach_notes SET state = ?2, updated_at = ?3,
                    decided_at = CASE WHEN ?2 = 'proposed' THEN NULL ELSE ?3 END
              WHERE id = ?1",
            params![id, state, now],
        )?;
        Ok(())
    }
}

/// The result of the local AI remembering directly. Only `Remembered` wrote to the database.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "outcome")]
pub enum Remembered {
    Remembered { id: i64, was_proposed: bool },
    AlreadyKnown { id: i64 },
    RejectedBefore,
}
