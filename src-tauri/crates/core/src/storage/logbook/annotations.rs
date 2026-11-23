//! How a workout felt: the user's own RPE, where it is sore, which shoes were worn.
//!
//! The RPE the watch records is in `workouts.rpe`. It is a cloud fact and is not overwritten. The user's own RPE is stored separately; the UI and the AI handoff
//! get both, labelled with who recorded each. A workout with no field filled in keeps no row.
use super::shoes::{assign, ShoeAssignment};
use super::{invalid, valid_body_part};
use crate::models::error::Result;
use crate::storage::Database;
use chrono::Utc;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AnnotationInput {
    pub workout_id: String,
    pub rpe: Option<u8>,
    #[serde(default)]
    pub soreness: Vec<String>,
    /// `auto` / `none` / `chosen`.
    pub shoe_mode: String,
    #[serde(default)]
    pub shoe_id: Option<i64>,
}

/// A pair of shoes the user can pick (same use and not retired come first).
#[derive(Debug, Clone, Serialize)]
pub struct ShoeChoice {
    pub id: i64,
    pub name: String,
    pub sport: String,
    pub retired: bool,
}

/// Everything the "How it felt" card in workout details needs.
#[derive(Debug, Clone, Serialize)]
pub struct WorkoutFeel {
    pub workout_id: String,
    /// The user's own RPE.
    pub rpe: Option<u8>,
    /// The RPE the watch recorded (from the cloud).
    pub watch_rpe: Option<i32>,
    pub soreness: Vec<String>,
    pub shoe_mode: String,
    pub shoe_id: Option<i64>,
    /// Which kind of shoe this workout calls for. `None` is a workout without shoes, so the UI does not ask about shoes.
    pub footwear: Option<&'static str>,
    /// Which pair the workout is counted on now, and why.
    pub assigned: Option<ShoeAssignment>,
    pub choices: Vec<ShoeChoice>,
}

fn parse_soreness(raw: &str) -> Vec<String> {
    serde_json::from_str::<Vec<String>>(raw)
        .unwrap_or_default()
        .into_iter()
        .filter(|part| valid_body_part(part))
        .collect()
}

/// Feelings for the AI handoff: the user's own RPE and sore body parts.
pub type FeelForAi = (Option<u8>, Vec<String>);

impl Database {
    pub fn workout_feel(&self, workout_id: &str) -> Result<Option<WorkoutFeel>> {
        let watch_rpe: Option<Option<i32>> = self
            .conn
            .query_row(
                "SELECT rpe FROM workouts_visible WHERE workout_id = ?1",
                [workout_id],
                |row| row.get(0),
            )
            .optional()?;
        let Some(watch_rpe) = watch_rpe else {
            return Ok(None);
        };
        let stored: Option<(Option<u8>, String, String, Option<i64>)> = self
            .conn
            .query_row(
                "SELECT rpe, soreness, shoe_mode, shoe_id FROM workout_annotations WHERE workout_id = ?1",
                [workout_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()?;
        let (rpe, soreness, shoe_mode, shoe_id) =
            stored.unwrap_or((None, "[]".into(), "auto".into(), None));
        let footwear_workout = self.footwear_workouts(Some(workout_id))?.into_iter().next();
        let shoes = self.list_shoe_rows()?;
        let footwear = footwear_workout.as_ref().map(|w| w.family);
        let assigned = footwear_workout.as_ref().map(|w| assign(w, &shoes));
        let mut choices: Vec<ShoeChoice> = shoes
            .iter()
            .filter(|_| footwear.is_some())
            .map(|shoe| ShoeChoice {
                id: shoe.input.id.unwrap_or_default(),
                name: shoe.input.name.clone(),
                sport: shoe.input.sport.clone(),
                retired: shoe.input.retired_on.is_some(),
            })
            .collect();
        // Same use first, retired last. Trail runs also put road running shoes first.
        let rank = |choice: &ShoeChoice| {
            let fits = footwear
                .is_some_and(|f| choice.sport == f || (f == "trail" && choice.sport == "running"));
            (choice.retired, !fits)
        };
        choices.sort_by_key(rank);
        Ok(Some(WorkoutFeel {
            workout_id: workout_id.to_string(),
            rpe,
            watch_rpe,
            soreness: parse_soreness(&soreness),
            shoe_mode,
            shoe_id,
            footwear,
            assigned,
            choices,
        }))
    }

    /// Save how a workout felt. If all fields are empty (no RPE, no soreness, shoes auto-matched), delete the row.
    pub fn save_workout_feel(&self, input: &AnnotationInput) -> Result<()> {
        let mut soreness: Vec<&str> = input.soreness.iter().map(String::as_str).collect();
        soreness.sort_unstable();
        soreness.dedup();
        let exists: bool = self
            .conn
            .query_row(
                "SELECT 1 FROM workouts_visible WHERE workout_id = ?1",
                [&input.workout_id],
                |_| Ok(true),
            )
            .optional()?
            .is_some();
        let shoe_ok = match input.shoe_mode.as_str() {
            "auto" | "none" => input.shoe_id.is_none(),
            "chosen" => match input.shoe_id {
                Some(id) => self
                    .conn
                    .query_row("SELECT 1 FROM shoes WHERE id = ?1", [id], |_| Ok(true))
                    .optional()?
                    .is_some(),
                None => false,
            },
            _ => false,
        };
        if !exists
            || !shoe_ok
            || input.rpe.is_some_and(|rpe| !(1..=10).contains(&rpe))
            || soreness.len() > 20
            || soreness.iter().any(|part| !valid_body_part(part))
        {
            return Err(invalid("The workout feeling data is not valid"));
        }
        if input.rpe.is_none() && soreness.is_empty() && input.shoe_mode == "auto" {
            self.conn.execute(
                "DELETE FROM workout_annotations WHERE workout_id = ?1",
                [&input.workout_id],
            )?;
            return Ok(());
        }
        let soreness = serde_json::to_string(&soreness).unwrap_or_else(|_| "[]".into());
        self.conn.execute(
            "INSERT INTO workout_annotations(workout_id, rpe, soreness, shoe_mode, shoe_id, updated_at)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(workout_id) DO UPDATE SET rpe = excluded.rpe, soreness = excluded.soreness,
                 shoe_mode = excluded.shoe_mode, shoe_id = excluded.shoe_id, updated_at = excluded.updated_at",
            params![input.workout_id, input.rpe, soreness, input.shoe_mode, input.shoe_id, Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    /// For the AI handoff: the user's own RPE and soreness over a period (workout ID -> (RPE, parts)).
    pub fn workout_feel_map(&self) -> Result<std::collections::HashMap<String, FeelForAi>> {
        let mut statement = self
            .conn
            .prepare("SELECT workout_id, rpe, soreness FROM workout_annotations")?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<u8>>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        let mut out = std::collections::HashMap::new();
        for row in rows {
            let (id, rpe, soreness) = row?;
            out.insert(id, (rpe, parse_soreness(&soreness)));
        }
        Ok(out)
    }
}
