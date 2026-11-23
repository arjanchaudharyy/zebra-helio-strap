//! Plan history: every plan pushed to the watch is kept, so you can browse all of them.
//!
//! The ledger is append-only. This is only another way to read it: list by batch, and each batch carries the workouts it put in
//! (`activated`, which is the plan itself), how many it took out, which draft it came from (written by you / a pasted AI reply /
//! MCP), and the scores given after those workouts. A rolling push (`roll`) only re-sends the same plan forward. It is not
//! a new plan, so it is not listed.

use super::{ids_from, summarize, Database, DraftOrigin, PublishRecord};
use crate::models::error::Result;
use crate::training_plan::Workout;
use rusqlite::OptionalExtension;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct HistoryWorkout {
    pub plan_workout_id: i64,
    #[serde(flatten)]
    pub workout: Workout,
    /// Score given after this workout (1-5). Empty when not scored.
    pub rating: Option<u8>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlanHistoryEntry {
    /// Summary of the whole batch (id, type, status, time).
    pub record: PublishRecord,
    /// Where the draft came from. Empty for undo, clear and revoke, which have no draft.
    pub origin: Option<DraftOrigin>,
    /// The one-line summary in the draft (original text by the user or AI).
    pub summary: Option<String>,
    /// Workouts this batch put in, sorted by date.
    pub workouts: Vec<HistoryWorkout>,
    /// How many workouts this batch took out.
    pub replaced: usize,
}

impl Database {
    /// Every pushed plan, newest first.
    pub fn plan_history(&self, limit: usize) -> Result<Vec<PlanHistoryEntry>> {
        let heads: Vec<(i64, Option<String>, String, String)> = {
            let mut statement = self.conn.prepare(
                "SELECT id, draft_id, activated, deactivated FROM training_plan_publishes
                  WHERE (batch_id IS NULL OR batch_id = id) AND kind != 'roll'
                  ORDER BY id DESC LIMIT ?1",
            )?;
            let rows = statement.query_map([limit.clamp(1, 200) as i64], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })?;
            rows.collect::<std::result::Result<_, _>>()?
        };
        let mut out = Vec::with_capacity(heads.len());
        for (batch_id, draft_id, activated, deactivated) in heads {
            let Some(record) = summarize(self.plan_batch(batch_id)?) else {
                continue;
            };
            let (origin, summary) = match draft_id {
                Some(id) => self
                    .conn
                    .query_row(
                        "SELECT origin, document FROM training_plan_drafts WHERE id = ?1",
                        [id],
                        |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                    )
                    .optional()?
                    .map(|(origin, document)| {
                        let summary = serde_json::from_str::<serde_json::Value>(&document)
                            .ok()
                            .and_then(|doc| doc.get("summary")?.as_str().map(str::to_owned))
                            .filter(|text| !text.trim().is_empty());
                        (Some(DraftOrigin::parse(&origin)), summary)
                    })
                    .unwrap_or((None, None)),
                None => (None, None),
            };
            let mut workouts = Vec::new();
            for id in ids_from(&activated) {
                let Some(text) = self
                    .conn
                    .query_row(
                        "SELECT workout FROM training_plan_workouts WHERE id = ?1",
                        [id],
                        |row| row.get::<_, String>(0),
                    )
                    .optional()?
                else {
                    continue;
                };
                let rating: Option<i64> = self
                    .conn
                    .query_row(
                        "SELECT MAX(rating) FROM workout_feedback
                          WHERE plan_workout_id = ?1 AND state = 'rated'",
                        [id],
                        |row| row.get(0),
                    )
                    .optional()?
                    .flatten();
                workouts.push(HistoryWorkout {
                    plan_workout_id: id,
                    workout: serde_json::from_str(&text)?,
                    rating: rating.map(|v| v as u8),
                });
            }
            workouts.sort_by(|a, b| {
                (a.workout.date, a.plan_workout_id).cmp(&(b.workout.date, b.plan_workout_id))
            });
            out.push(PlanHistoryEntry {
                record,
                origin,
                summary,
                workouts,
                replaced: ids_from(&deactivated).len(),
            });
        }
        Ok(out)
    }
}
