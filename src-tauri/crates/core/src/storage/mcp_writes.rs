//! Change ledger for the local AI (v49, 3.1.2).
//!
//! Each write by an MCP write tool (injury, life event, race, running shoe, workout feel, type change, coach profile) records one row:
//! a snapshot before and after the change. The "Local AI" card on desktop undoes them one by one:
//!
//! - Undo of a create = delete (a coach profile note becomes "rejected", so the AI does not write the same sentence back);
//! - Undo of an update = write back the before snapshot through the original save function;
//! - Undo of a type change uses `undo_workout_edit` (it has its own ledger);
//! - When the current value does not match the "after" in the ledger (the user or another AI changed it), or the same record has a newer change not yet undone,
//!   never undo. Return [`UndoOutcome::Stale`].
//!
//! There is no delete tool: AI can only create and update. "End / retire" uses fields (injury `end_date`, shoe `retired_on`).
//! Sync and export do not enter this table: they are actions and do not change records. As usual, the caller takes the cross-process write lock for writes.

use super::coach_notes::Remembered;
use super::life_events::LifeEventInput;
use super::logbook::annotations::AnnotationInput;
use super::logbook::injuries::InjuryInput;
use super::logbook::races::RaceInput;
use super::logbook::shoes::ShoeInput;
use super::Database;
use crate::models::{error::Result, ZeppBridgeError};
use chrono::Utc;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// What one write changes.
#[derive(Debug, Clone)]
pub enum McpChange {
    Injury(InjuryInput),
    LifeEvent(LifeEventInput),
    Race(RaceInput),
    Shoe(ShoeInput),
    /// RPE and sore areas. The shoe choice keeps the current one and the AI does not change it.
    WorkoutFeel {
        workout_id: String,
        rpe: Option<u8>,
        soreness: Vec<String>,
    },
    /// `None` removes the correction and goes back to the type the watch recorded.
    WorkoutType {
        workout_id: String,
        workout_type: Option<String>,
    },
    CoachNote(String),
}

/// The result of a write. An empty `write_id` means nothing was written (the coach profile already has it / it was rejected before).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpApplied {
    pub write_id: Option<i64>,
    pub kind: &'static str,
    pub target_id: String,
    /// `created` / `updated` / `unchanged`.
    pub action: &'static str,
    /// The outcome for the coach profile (empty for all others).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<Remembered>,
}

/// One row as the desktop sees it.
#[derive(Debug, Clone, Serialize)]
pub struct McpWrite {
    pub id: i64,
    pub tool: String,
    pub kind: String,
    pub target_id: String,
    pub label: String,
    /// `created` / `updated`.
    pub action: &'static str,
    pub source_name: Option<String>,
    pub created_at: String,
    pub undone_at: Option<String>,
}

/// The outcome of an undo. The command layer maps it to a stable code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UndoOutcome {
    Undone,
    /// It was changed again later, so it cannot be undone from the ledger.
    Stale,
    /// The ledger does not have this row, or it was already undone.
    Missing,
}

fn encode<T: Serialize>(value: &T) -> Result<Value> {
    serde_json::to_value(value).map_err(|error| ZeppBridgeError::ParseError(error.to_string()))
}

fn decode<T: serde::de::DeserializeOwned>(value: Value) -> Result<T> {
    serde_json::from_value(value).map_err(|error| ZeppBridgeError::ParseError(error.to_string()))
}

type ShoeDefaults = BTreeMap<String, Vec<i64>>;

#[derive(Serialize, Deserialize)]
struct ShoeDefaultChange {
    before: ShoeDefaults,
    after: ShoeDefaults,
}

fn clip(text: &str) -> String {
    let mut out: String = text.chars().take(80).collect();
    if text.chars().count() > 80 {
        out.push('…');
    }
    out
}

impl Database {
    /// The current state of a record (the copy used for ledger comparison and write-back). `None` when it does not exist.
    fn mcp_snapshot(&self, kind: &str, target: &str) -> Result<Option<Value>> {
        let id = || target.parse::<i64>().unwrap_or(-1);
        Ok(match kind {
            "injury" => self
                .list_injuries()?
                .into_iter()
                .find(|row| row.input.id == Some(id()))
                .map(|row| encode(&row.input))
                .transpose()?,
            "life_event" => self
                .list_life_events(None, None)?
                .into_iter()
                .find(|row| row.input.id == Some(id()))
                .map(|row| encode(&row.input))
                .transpose()?,
            "race" => self
                .list_races()?
                .into_iter()
                .find(|row| row.input.id == Some(id()))
                .map(|row| encode(&row.input))
                .transpose()?,
            "shoe" => self
                .list_shoe_rows()?
                .into_iter()
                .find(|row| row.input.id == Some(id()))
                .map(|row| encode(&row.input))
                .transpose()?,
            "workout_feel" => self
                .conn
                .query_row(
                    "SELECT rpe, soreness, shoe_mode, shoe_id FROM workout_annotations WHERE workout_id = ?1",
                    [target],
                    |row| {
                        Ok(AnnotationInput {
                            workout_id: target.to_string(),
                            rpe: row.get(0)?,
                            soreness: serde_json::from_str(&row.get::<_, String>(1)?)
                                .unwrap_or_default(),
                            shoe_mode: row.get(2)?,
                            shoe_id: row.get(3)?,
                        })
                    },
                )
                .optional()?
                .map(|input| encode(&input))
                .transpose()?,
            "coach_note" => self
                .coach_note_state(id())?
                .map(|(state, text)| json!({ "state": state, "text": text })),
            _ => None,
        })
    }

    /// Write back from a snapshot. `None` means delete (undo of a create).
    fn mcp_restore(&self, kind: &str, target: &str, before: Option<Value>) -> Result<()> {
        let id = target.parse::<i64>().unwrap_or(-1);
        match (kind, before) {
            ("injury", Some(value)) => self.save_injury(&decode(value)?).map(drop),
            ("injury", None) => self.delete_injury(id),
            ("life_event", Some(value)) => self.save_life_event(&decode(value)?).map(drop),
            ("life_event", None) => self.delete_life_event(id),
            ("race", Some(value)) => self.save_race(&decode(value)?).map(drop),
            ("race", None) => self.delete_race(id),
            ("shoe", Some(value)) => self.save_shoe(&decode(value)?).map(drop),
            ("shoe", None) => self.delete_shoe(id),
            ("workout_feel", before) => {
                let input = match before {
                    Some(value) => decode(value)?,
                    // There was no row before: store an all-empty one, and save_workout_feel deletes the row.
                    None => AnnotationInput {
                        workout_id: target.to_string(),
                        rpe: None,
                        soreness: Vec::new(),
                        shoe_mode: "auto".into(),
                        shoe_id: None,
                    },
                };
                self.save_workout_feel(&input)
            }
            ("coach_note", Some(value)) => self.set_coach_note_state(
                id,
                value
                    .get("state")
                    .and_then(Value::as_str)
                    .unwrap_or("proposed"),
            ),
            ("coach_note", None) => self.set_coach_note_state(id, "rejected"),
            _ => Err(ZeppBridgeError::ConfigError("Unknown change kind".into())),
        }
    }

    #[allow(clippy::too_many_arguments)] // seven columns of one ledger row. A struct would only move the same seven fields to another place
    fn record_mcp_write(
        &self,
        tool: &str,
        kind: &str,
        target: &str,
        label: &str,
        before: Option<&Value>,
        after: &Value,
        source_name: Option<&str>,
    ) -> Result<i64> {
        let source: Option<String> = source_name
            .map(|name| name.chars().filter(|c| !c.is_control()).take(60).collect())
            .filter(|name: &String| !name.trim().is_empty());
        self.conn.execute(
            "INSERT INTO mcp_writes(tool, kind, target_id, label, before_json, after_json, source_name, created_at)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                tool,
                kind,
                target,
                clip(label),
                before.map(Value::to_string),
                after.to_string(),
                source,
                Utc::now().to_rfc3339()
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// One local AI write, recorded in the ledger. All validation is done by the original save functions, with no second copy here.
    pub fn apply_mcp_change(
        &self,
        tool: &str,
        change: McpChange,
        source_name: Option<&str>,
    ) -> Result<McpApplied> {
        let tx = self.conn.unchecked_transaction()?;
        let (applied, changed_workout) = match &change {
            McpChange::WorkoutType {
                workout_id,
                workout_type,
            } => self.apply_mcp_type(tool, workout_id, workout_type.as_deref(), source_name)?,
            _ => (
                self.apply_mcp_change_in_transaction(tool, change, source_name)?,
                false,
            ),
        };
        tx.commit()?;
        if changed_workout {
            super::note_content_change();
        }
        Ok(applied)
    }

    fn apply_mcp_change_in_transaction(
        &self,
        tool: &str,
        change: McpChange,
        source_name: Option<&str>,
    ) -> Result<McpApplied> {
        let (kind, target, label, before) = match &change {
            McpChange::Injury(input) => ("injury", input.id, input.title.clone(), None),
            McpChange::LifeEvent(input) => ("life_event", input.id, input.title.clone(), None),
            McpChange::Race(input) => ("race", input.id, input.name.clone(), None),
            McpChange::Shoe(input) => return self.apply_mcp_shoe(tool, input, source_name),
            McpChange::WorkoutFeel { workout_id, .. } => (
                "workout_feel",
                None,
                workout_id.clone(),
                Some(workout_id.clone()),
            ),
            McpChange::WorkoutType { .. } => unreachable!("handled by the transaction entry point"),
            McpChange::CoachNote(text) => {
                return self.apply_mcp_note(tool, text, source_name);
            }
        };
        // Updating an existing record: first confirm it exists, and get its state before the change.
        let target_before = match (target, &before) {
            (Some(id), _) => Some(id.to_string()),
            (None, Some(workout)) => Some(workout.clone()),
            (None, None) => None,
        };
        let before_value = match &target_before {
            Some(target) => self.mcp_snapshot(kind, target)?,
            None => None,
        };
        if target.is_some() && before_value.is_none() {
            return Err(ZeppBridgeError::DataUnavailable(
                "Record to change not found".into(),
            ));
        }
        let target_id = match change {
            McpChange::Injury(input) => self.save_injury(&input)?.to_string(),
            McpChange::LifeEvent(input) => self.save_life_event(&input)?.to_string(),
            McpChange::Race(input) => self.save_race(&input)?.to_string(),
            McpChange::WorkoutFeel {
                workout_id,
                rpe,
                soreness,
            } => {
                // The shoe keeps the current choice: the AI only changes RPE and soreness.
                let current: Option<AnnotationInput> =
                    before_value.clone().map(decode).transpose()?;
                let (shoe_mode, shoe_id) = current
                    .map(|feel| (feel.shoe_mode, feel.shoe_id))
                    .unwrap_or_else(|| ("auto".into(), None));
                self.save_workout_feel(&AnnotationInput {
                    workout_id: workout_id.clone(),
                    rpe,
                    soreness,
                    shoe_mode,
                    shoe_id,
                })?;
                workout_id
            }
            McpChange::WorkoutType { .. } | McpChange::CoachNote(_) | McpChange::Shoe(_) => {
                unreachable!()
            }
        };
        let after = self.mcp_snapshot(kind, &target_id)?.unwrap_or(Value::Null);
        let action = if before_value.is_none() {
            "created"
        } else {
            "updated"
        };
        if before_value.as_ref() == Some(&after) {
            return Ok(McpApplied {
                write_id: None,
                kind,
                target_id,
                action: "unchanged",
                note: None,
            });
        }
        let write_id = self.record_mcp_write(
            tool,
            kind,
            &target_id,
            &label,
            before_value.as_ref(),
            &after,
            source_name,
        )?;
        Ok(McpApplied {
            write_id: Some(write_id),
            kind,
            target_id,
            action,
            note: None,
        })
    }

    fn shoe_defaults<'a>(
        &self,
        sports: impl IntoIterator<Item = &'a String>,
    ) -> Result<ShoeDefaults> {
        let mut defaults: ShoeDefaults = sports
            .into_iter()
            .map(|sport| (sport.clone(), Vec::new()))
            .collect();
        for shoe in self.list_shoe_rows()? {
            if let (Some(ids), Some(id), true) = (
                defaults.get_mut(&shoe.input.sport),
                shoe.input.id,
                shoe.input.is_default,
            ) {
                ids.push(id);
            }
        }
        for ids in defaults.values_mut() {
            ids.sort_unstable();
        }
        Ok(defaults)
    }

    fn apply_mcp_shoe(
        &self,
        tool: &str,
        input: &ShoeInput,
        source_name: Option<&str>,
    ) -> Result<McpApplied> {
        let existing: Option<ShoeInput> = match input.id {
            Some(id) => Some(decode(
                self.mcp_snapshot("shoe", &id.to_string())?.ok_or_else(|| {
                    ZeppBridgeError::DataUnavailable("Record to change not found".into())
                })?,
            )?),
            None => None,
        };
        let defaults = self.shoe_defaults(
            std::iter::once(&input.sport).chain(existing.iter().map(|shoe| &shoe.sport)),
        )?;
        let target_id = self.save_shoe(input)?.to_string();
        let mut after = self
            .mcp_snapshot("shoe", &target_id)?
            .ok_or_else(|| ZeppBridgeError::DataUnavailable("Running shoe just saved not found".into()))?;
        let before = existing.as_ref().map(encode).transpose()?;
        let new_defaults = self.shoe_defaults(defaults.keys())?;
        let action = match (
            before.as_ref() == Some(&after),
            defaults == new_defaults,
            before.is_none(),
        ) {
            (true, true, _) => "unchanged",
            (_, _, true) => "created",
            _ => "updated",
        };
        let write_id = if action == "unchanged" {
            None
        } else {
            after["default_change"] = encode(&ShoeDefaultChange {
                before: defaults,
                after: new_defaults,
            })?;
            Some(self.record_mcp_write(
                tool,
                "shoe",
                &target_id,
                &input.name,
                before.as_ref(),
                &after,
                source_name,
            )?)
        };
        Ok(McpApplied {
            write_id,
            kind: "shoe",
            target_id,
            action,
            note: None,
        })
    }

    fn apply_mcp_type(
        &self,
        tool: &str,
        workout_id: &str,
        workout_type: Option<&str>,
        source_name: Option<&str>,
    ) -> Result<(McpApplied, bool)> {
        let (edit, changed) =
            self.edit_workouts_in_transaction(&[workout_id.to_string()], Some(workout_type), None)?;
        let after = json!({ "editId": edit.id, "type": workout_type });
        let write_id = self.record_mcp_write(
            tool,
            "workout_type",
            workout_id,
            workout_id,
            None,
            &after,
            source_name,
        )?;
        Ok((
            McpApplied {
                write_id: Some(write_id),
                kind: "workout_type",
                target_id: workout_id.to_string(),
                action: "updated",
                note: None,
            },
            changed,
        ))
    }

    fn apply_mcp_note(
        &self,
        tool: &str,
        text: &str,
        source_name: Option<&str>,
    ) -> Result<McpApplied> {
        let outcome = self.remember_coach_note(text, source_name)?;
        let (write_id, target_id, action) = match &outcome {
            Remembered::Remembered { id, was_proposed } => {
                let target = id.to_string();
                let before =
                    was_proposed.then(|| json!({ "state": "proposed", "text": text.trim() }));
                let after = self
                    .mcp_snapshot("coach_note", &target)?
                    .unwrap_or(Value::Null);
                let label = after
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or(text)
                    .to_string();
                let write = self.record_mcp_write(
                    tool,
                    "coach_note",
                    &target,
                    &label,
                    before.as_ref(),
                    &after,
                    source_name,
                )?;
                (
                    Some(write),
                    target,
                    if *was_proposed { "updated" } else { "created" },
                )
            }
            Remembered::AlreadyKnown { id } => (None, id.to_string(), "unchanged"),
            Remembered::RejectedBefore => (None, String::new(), "unchanged"),
        };
        Ok(McpApplied {
            write_id,
            kind: "coach_note",
            target_id,
            action,
            note: Some(outcome),
        })
    }

    /// Recent changes, newest first (includes undone ones, which the UI greys out).
    pub fn mcp_writes(&self, limit: usize) -> Result<Vec<McpWrite>> {
        let mut statement = self.conn.prepare(
            "SELECT id, tool, kind, target_id, label, before_json IS NULL, source_name, created_at, undone_at
               FROM mcp_writes ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = statement.query_map([limit.clamp(1, 500) as i64], |row| {
            let kind: String = row.get(2)?;
            let created: bool = row.get(5)?;
            Ok(McpWrite {
                id: row.get(0)?,
                tool: row.get(1)?,
                action: if created && kind != "workout_type" {
                    "created"
                } else {
                    "updated"
                },
                kind,
                target_id: row.get(3)?,
                label: row.get(4)?,
                source_name: row.get(6)?,
                created_at: row.get(7)?,
                undone_at: row.get(8)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    /// Undo one. See the module notes.
    pub fn undo_mcp_write(&self, id: i64) -> Result<UndoOutcome> {
        let tx = self.conn.unchecked_transaction()?;
        let (outcome, changed_workout) = self.undo_mcp_write_in_transaction(id)?;
        tx.commit()?;
        if changed_workout {
            super::note_content_change();
        }
        Ok(outcome)
    }

    fn undo_mcp_write_in_transaction(&self, id: i64) -> Result<(UndoOutcome, bool)> {
        let row: Option<(String, String, Option<String>, String)> = self
            .conn
            .query_row(
                "SELECT kind, target_id, before_json, after_json FROM mcp_writes
                  WHERE id = ?1 AND undone_at IS NULL",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()?;
        let Some((kind, target, before, after)) = row else {
            return Ok((UndoOutcome::Missing, false));
        };
        let newer: bool = self
            .conn
            .query_row(
                "SELECT 1 FROM mcp_writes WHERE kind = ?1 AND target_id = ?2 AND id > ?3
                    AND undone_at IS NULL LIMIT 1",
                params![kind, target, id],
                |_| Ok(true),
            )
            .optional()?
            .is_some();
        if newer {
            return Ok((UndoOutcome::Stale, false));
        }
        let parse = |text: &str| {
            serde_json::from_str::<Value>(text)
                .map_err(|error| ZeppBridgeError::ParseError(error.to_string()))
        };
        let after = parse(&after)?;
        if kind == "workout_type" {
            let edit_id = after
                .get("editId")
                .and_then(Value::as_str)
                .unwrap_or_default();
            match self.undo_workout_edit_in_transaction(edit_id) {
                Ok(_) => {}
                Err(ZeppBridgeError::ConfigError(_) | ZeppBridgeError::DataUnavailable(_)) => {
                    return Ok((UndoOutcome::Stale, false))
                }
                Err(error) => return Err(error),
            }
        } else {
            let before = before.as_deref().map(parse).transpose()?;
            if kind == "shoe" {
                if !self.undo_mcp_shoe(&target, before, after)? {
                    return Ok((UndoOutcome::Stale, false));
                }
            } else {
                if self.mcp_snapshot(&kind, &target)?.as_ref() != Some(&after) {
                    return Ok((UndoOutcome::Stale, false));
                }
                self.mcp_restore(&kind, &target, before)?;
            }
        }
        self.conn.execute(
            "UPDATE mcp_writes SET undone_at = ?2 WHERE id = ?1",
            params![id, Utc::now().to_rfc3339()],
        )?;
        Ok((UndoOutcome::Undone, kind == "workout_type"))
    }

    fn undo_mcp_shoe(&self, target: &str, before: Option<Value>, mut after: Value) -> Result<bool> {
        let default_change: Option<ShoeDefaultChange> = after
            .as_object_mut()
            .and_then(|entry| entry.remove("default_change"))
            .map(decode)
            .transpose()?;
        if self.mcp_snapshot("shoe", target)?.as_ref() != Some(&after) {
            return Ok(false);
        }
        if before.is_none()
            && self.conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM workout_annotations WHERE shoe_id = ?1)",
                [target],
                |row| row.get::<_, bool>(0),
            )?
        {
            return Ok(false);
        }
        let prior: Option<ShoeInput> = before.clone().map(decode).transpose()?;
        let current: ShoeInput = decode(after)?;

        if let Some(change) = &default_change {
            if self.shoe_defaults(change.after.keys())? != change.after {
                return Ok(false);
            }
            let shoes = self.list_shoe_rows()?;
            for (sport, ids) in &change.before {
                for id in ids {
                    if Some(*id) != current.id
                        && !shoes.iter().any(|shoe| {
                            shoe.input.id == Some(*id)
                                && shoe.input.sport == *sport
                                && shoe.input.retired_on.is_none()
                        })
                    {
                        return Ok(false);
                    }
                }
            }
        } else if current.is_default
            && !prior
                .as_ref()
                .is_some_and(|shoe| shoe.is_default && shoe.sport == current.sport)
        {
            // Old entries did not record which previous default this edit replaced.
            return Ok(false);
        } else if let Some(shoe) = &prior {
            if shoe.is_default
                && self.shoe_defaults([&shoe.sport])?[&shoe.sport]
                    .iter()
                    .any(|id| Some(*id) != current.id)
            {
                return Ok(false);
            }
        }

        self.mcp_restore("shoe", target, before)?;
        if let Some(change) = default_change {
            for (sport, ids) in change.before {
                for id in ids {
                    self.conn.execute(
                        "UPDATE shoes SET is_default = 1 WHERE id = ?1 AND sport = ?2",
                        params![id, sport],
                    )?;
                }
            }
        }
        Ok(true)
    }
}

/// The "shared tasks only" switch (Settings > Local AI). When on, the generated config includes `--scope task`.
const TASK_SCOPE_ONLY_KEY: &str = "mcp_task_scope_only";

impl Database {
    pub fn mcp_task_scope_only(&self) -> Result<bool> {
        Ok(self.get_app_meta(TASK_SCOPE_ONLY_KEY)?.as_deref() == Some("1"))
    }

    pub fn set_mcp_task_scope_only(&self, on: bool) -> Result<()> {
        self.set_app_meta(TASK_SCOPE_ONLY_KEY, if on { "1" } else { "0" })
    }
}
