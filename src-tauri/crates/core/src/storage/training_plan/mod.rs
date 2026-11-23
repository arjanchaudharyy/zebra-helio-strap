//! Draft and publish ledger for training plans.
//!
//! The official API has no read endpoint, so this is the only source of "what is on the watch now". The rules:
//!
//! - **Record first, then send**: [`Database::prepare_plan_publish`] updates the active
//!   plan and writes every full-window body to send (status `pending`) in one transaction. Only then does the caller send. After each send,
//!   [`Database::finish_plan_publish`] records the result. If the process is killed in between, the remaining `pending` rows
//!   count as "unknown whether it arrived", not as success.
//! - **Training rows are append-only**: switching plans only flips `active`. Each push records which rows it flipped. Undo
//!   flips them back exactly and pushes again.
//! - **Push every 7-day window the plan covers** (windows are cut in 7-day steps from today. R4 test on 2026-10-06:
//!   future windows can be pushed and do not affect this week). One push is one "batch", with one row per window and a result per row.
//!   If one week did not arrive, mark that week as it is. The rolling push fills it in next time. See [`publish`].
//! - **Send an empty array only after the user confirms**. It always clears only "7 days from today" (R5 test, independent of
//!   `startDate`). To make a future window empty, use "placeholder + push the previous window again". The automatic rolling
//!   push never clears any window.

mod history;
mod publish;

pub use history::{HistoryWorkout, PlanHistoryEntry};
pub use publish::PLACEHOLDER_ID_BASE;

use super::Database;
use crate::models::{error::Result, ZeppBridgeError};
use crate::training_plan::v2::NumberedWorkout;
use crate::training_plan::window::{preview, DayPreview, Window};
use crate::training_plan::{check_plan, PlanCheck, PlanContext, PlanDocument, Workout};
use chrono::{NaiveDate, Utc};
use rusqlite::{params, OptionalExtension};
use serde::Serialize;
use serde_json::Value;

/// The "let AI publish directly" switch, off by default: AI can only draft, and it is sent only after the user confirms in ZeppBridge.
const AI_PUBLISH_KEY: &str = "training_plan_ai_publish";

/// Where the draft came from. The UI uses it to say "drafted by AI" or "written by you".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DraftOrigin {
    User,
    AiPaste,
    Mcp,
}

impl DraftOrigin {
    fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::AiPaste => "ai_paste",
            Self::Mcp => "mcp",
        }
    }

    fn parse(text: &str) -> Self {
        match text {
            "ai_paste" => Self::AiPaste,
            "mcp" => Self::Mcp,
            _ => Self::User,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PlanDraft {
    pub id: String,
    pub origin: DraftOrigin,
    pub status: String,
    pub document: PlanDocument,
    pub created_at: String,
    pub updated_at: String,
}

/// What to send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PublishRequest {
    /// Turn a draft into the active plan and push every window whose content changed.
    Draft { id: String },
    /// The daily rolling push: the active plan stays the same. Only resend the windows where the ledger and the active plan differ (the window moves
    /// forward one day each day, and a week that did not arrive earlier is filled in here too). Never clears a window.
    Roll,
    /// Undo the last publish: flip back the rows it changed exactly, then push again.
    UndoLast,
    /// Clear the current window (empty array). Only the user can start this, after confirmation.
    Clear,
}

impl PublishRequest {
    fn kind(&self) -> &'static str {
        match self {
            Self::Draft { .. } => "publish",
            Self::Roll => "roll",
            Self::UndoLast => "undo",
            Self::Clear => "clear",
        }
    }
}

/// How to send one window this time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SendRole {
    /// Normal full window: the training sessions in the window.
    Window,
    /// The current window (from today) must become empty: send `[]`.
    Clear,
    /// A future window must become empty: clear with a placeholder (see `training_plan::v2::clear_future_window`),
    /// then in the same batch push the previous window again right after, to overwrite the placeholder.
    Placeholder,
}

impl SendRole {
    fn as_str(self) -> &'static str {
        match self {
            Self::Window => "window",
            Self::Clear => "clear",
            Self::Placeholder => "placeholder",
        }
    }
}

/// One window in a batch: send `body`, then bring back `publish_id` to record the result. **Send in order**:
/// later windows first, and each placeholder directly followed by the window before it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PendingSend {
    pub publish_id: i64,
    pub window_start: NaiveDate,
    pub role: SendRole,
    pub body: Value,
}

/// The result of preparing. Only `Send` leaves `pending` rows in the ledger.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "outcome")]
pub enum Prepared {
    /// Go and send: send `sends` in order, and record the result after each one.
    Send {
        batch_id: i64,
        sends: Vec<PendingSend>,
    },
    /// Nothing to send: every window has the same content as the last send (the active plan may already be updated).
    NotNeeded,
    /// This would make a window empty, and the last send to that window had training in it: clearing needs user confirmation.
    /// Nothing changed.
    NeedsClearConfirmation,
    /// The draft failed validation. Nothing changed.
    Invalid { check: PlanCheck },
    /// No publish to undo.
    NothingToUndo,
}

/// The result after a push is sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SendOutcome {
    /// The official API returned 2xx. (It returns success for any body, so this only means "it arrived".)
    Delivered,
    /// The official API refused it explicitly (4xx): this window did not take effect. When the whole batch is refused, the ledger changes must be flipped back.
    Rejected {
        http_status: u16,
        error_code: String,
    },
    /// Unknown whether it arrived: offline, timeout, 5xx. The ledger stays "changed", and the UI offers a resend.
    Unknown { error_code: String },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PublishRecord {
    pub id: i64,
    /// All pushes in one batch share one number (the `id` of the first row in the batch).
    pub batch_id: i64,
    pub kind: String,
    /// `window` / `clear` / `placeholder`.
    pub role: String,
    pub window_start: String,
    pub workout_count: usize,
    /// `pending` / `sent` / `rejected` / `unknown`. The batch summary can also be `partial`
    /// (some windows arrived, some were refused).
    pub state: String,
    pub http_status: Option<u16>,
    pub error_code: Option<String>,
    pub undone: bool,
    pub created_at: String,
    pub finished_at: Option<String>,
}

/// One 7-day window as the ledger sees it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WeekStatus {
    pub start: NaiveDate,
    /// How many training sessions the active plan has in these 7 days.
    pub planned: usize,
    /// How many were sent according to the ledger.
    pub sent: usize,
    pub state: WeekState,
    /// The error code (`err.*`) when the last push of these days failed.
    pub error_code: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WeekState {
    /// What the ledger sent matches the active plan.
    InSync,
    /// It matches, but the last result is unknown (offline, timeout, process stopped, or a placeholder may remain).
    Uncertain,
    /// It does not match: refused, not sent, or still sending. The rolling push resends the weeks that have training.
    NotSent,
}

/// The "last sent to the watch" state, derived from the ledger, and the active plan.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PlanOverview {
    pub window: Window,
    /// The training sessions the ledger last sent in the current window.
    pub sent: Vec<Workout>,
    /// All training sessions in the active plan from today onward.
    pub planned: Vec<Workout>,
    /// The last push result is unknown (offline, timeout, process stopped), so `sent` may differ from the watch.
    pub uncertain: bool,
    /// Summary of the last push batch (the status is the worst one in the batch).
    pub last_publish: Option<PublishRecord>,
    /// Per-window results of the last push batch. The UI uses them to say "which week did not arrive".
    pub last_batch: Vec<PublishRecord>,
    /// Every 7-day window (from today) that has training or had training sent.
    pub weeks: Vec<WeekStatus>,
    pub can_undo: bool,
}

/// Preview of a draft: validation result + a day-by-day "before send / after send".
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DraftPreview {
    pub check: PlanCheck,
    pub window: Window,
    /// Day-by-day comparison from today to `max(window end, draft end)`. "Before" is what the ledger sent.
    pub days: Vec<DayPreview>,
}

/// The push batch that can be undone, and the rows it flipped.
struct UndoTarget {
    id: i64,
    activated: Vec<i64>,
    deactivated: Vec<i64>,
}

fn plan_error(code: &'static str, message: &str) -> ZeppBridgeError {
    ZeppBridgeError::TrainingPlan {
        code,
        message: message.into(),
    }
}

fn random_hex(bytes: usize) -> String {
    let mut buffer = vec![0u8; bytes];
    if getrandom::getrandom(&mut buffer).is_err() {
        return format!("{:x}", Utc::now().timestamp_nanos_opt().unwrap_or_default());
    }
    hex::encode(buffer)
}

fn ids_json(ids: &[i64]) -> String {
    serde_json::to_string(ids).unwrap_or_else(|_| "[]".into())
}

fn ids_from(text: &str) -> Vec<i64> {
    serde_json::from_str(text).unwrap_or_default()
}

impl Database {
    /// For the heart rate limit check: the larger of the locally measured max heart rate and the max heart rate the watch reports. `None` when neither exists
    /// (validation falls back to 220 and only blocks numbers that are clearly wrong).
    pub fn training_plan_max_hr(&self) -> Result<Option<u16>> {
        let value: Option<f64> = self.conn.query_row(
            "SELECT MAX(v) FROM (
                SELECT MAX(max_hr) AS v FROM workouts_visible WHERE max_hr > 0
                UNION ALL
                SELECT MAX(value) FROM daily_metrics
                 WHERE metric = 'device_max_hr' AND value > 0)",
            [],
            |row| row.get(0),
        )?;
        Ok(value
            .filter(|v| v.is_finite() && *v > 0.0)
            .map(|v| v.round().min(f64::from(u16::MAX)) as u16))
    }

    pub fn ai_may_publish_plans(&self) -> Result<bool> {
        Ok(self.get_app_meta(AI_PUBLISH_KEY)?.as_deref() == Some("1"))
    }

    pub fn set_ai_may_publish_plans(&self, allowed: bool) -> Result<()> {
        self.set_app_meta(AI_PUBLISH_KEY, if allowed { "1" } else { "0" })
    }

    pub fn save_plan_draft(&self, origin: DraftOrigin, document: &PlanDocument) -> Result<String> {
        let id = format!("plan-{}", random_hex(8));
        let now = Utc::now().to_rfc3339();
        let transaction = self.conn.unchecked_transaction()?;
        transaction.execute(
            "INSERT INTO training_plan_drafts(id, origin, document, status, created_at, updated_at)
             VALUES(?1, ?2, ?3, 'open', ?4, ?4)",
            params![id, origin.as_str(), serde_json::to_string(document)?, now],
        )?;
        if origin == DraftOrigin::AiPaste {
            self.link_ai_exchange(&id)?;
        }
        if origin == DraftOrigin::Mcp {
            self.record_mcp_exchange(&id, document)?;
        }
        transaction.commit()?;
        Ok(id)
    }

    pub fn plan_draft(&self, id: &str) -> Result<Option<PlanDraft>> {
        self.conn
            .query_row(
                "SELECT id, origin, status, document, created_at, updated_at
                 FROM training_plan_drafts WHERE id = ?1",
                [id],
                draft_row,
            )
            .optional()?
            .transpose()
    }

    /// Drafts not yet sent and not discarded, newest first.
    pub fn open_plan_drafts(&self) -> Result<Vec<PlanDraft>> {
        let mut statement = self.conn.prepare(
            "SELECT id, origin, status, document, created_at, updated_at
             FROM training_plan_drafts WHERE status = 'open'
             ORDER BY created_at DESC, id DESC LIMIT 20",
        )?;
        let rows = statement.query_map([], draft_row)?;
        rows.map(|row| row?).collect()
    }

    pub fn discard_plan_draft(&self, id: &str) -> Result<bool> {
        let changed = self.conn.execute(
            "UPDATE training_plan_drafts SET status = 'discarded', updated_at = ?2
             WHERE id = ?1 AND status = 'open'",
            params![id, Utc::now().to_rfc3339()],
        )?;
        Ok(changed > 0)
    }

    fn plan_context(&self, today: NaiveDate) -> Result<PlanContext> {
        Ok(PlanContext {
            today,
            max_hr: self.training_plan_max_hr()?,
        })
    }

    /// Training sessions in the active plan with date >= `from`, with their ids.
    fn active_plan_workouts(&self, from: NaiveDate) -> Result<Vec<NumberedWorkout>> {
        let mut statement = self.conn.prepare(
            "SELECT id, workout FROM training_plan_workouts
             WHERE active = 1 AND workout_date >= ?1
             ORDER BY workout_date, id",
        )?;
        let rows = statement.query_map([from.to_string()], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?;
        rows.map(|row| {
            let (id, text) = row?;
            Ok(NumberedWorkout {
                id,
                workout: serde_json::from_str(&text)?,
            })
        })
        .collect()
    }

    fn workouts_by_ids(&self, ids: &[i64]) -> Result<Vec<Workout>> {
        let mut statement = self
            .conn
            .prepare("SELECT workout FROM training_plan_workouts WHERE id = ?1")?;
        let mut workouts = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(text) = statement
                .query_row([id], |row| row.get::<_, String>(0))
                .optional()?
            {
                workouts.push(serde_json::from_str(&text)?);
            }
        }
        Ok(workouts)
    }

    pub fn plan_publishes(&self, limit: usize) -> Result<Vec<PublishRecord>> {
        let mut statement = self
            .conn
            .prepare(&format!("{PUBLISH_COLUMNS} ORDER BY id DESC LIMIT ?1"))?;
        let rows = statement.query_map([limit.clamp(1, 200) as i64], publish_row)?;
        rows.map(|row| row.map_err(Into::into)).collect()
    }

    /// Per-window records of one push batch, in send order.
    pub fn plan_batch(&self, batch_id: i64) -> Result<Vec<PublishRecord>> {
        let mut statement = self.conn.prepare(&format!(
            "{PUBLISH_COLUMNS} WHERE COALESCE(batch_id, id) = ?1 ORDER BY id"
        ))?;
        let rows = statement.query_map([batch_id], publish_row)?;
        rows.map(|row| row.map_err(Into::into)).collect()
    }

    /// Summary of one batch: the status is the worst one, and the counts are added up.
    pub fn plan_batch_summary(&self, batch_id: i64) -> Result<Option<PublishRecord>> {
        Ok(summarize(self.plan_batch(batch_id)?))
    }

    fn last_batch_id(&self) -> Result<Option<i64>> {
        self.conn
            .query_row(
                "SELECT id FROM training_plan_publishes
                 WHERE kind != 'revoked' AND (batch_id IS NULL OR batch_id = id)
                 ORDER BY id DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn plan_overview(&self, today: NaiveDate) -> Result<PlanOverview> {
        let window = Window::starting(today);
        let (sent, mut uncertain) = self.sent_for_window(window)?;
        let active = self.active_plan_workouts(today)?;
        let weeks = self.week_statuses(today, &active)?;
        uncertain |= weeks.iter().any(|week| week.state == WeekState::Uncertain);
        let last_batch = match self.last_batch_id()? {
            Some(id) => self.plan_batch(id)?,
            None => Vec::new(),
        };
        Ok(PlanOverview {
            window,
            sent,
            planned: active.into_iter().map(|item| item.workout).collect(),
            uncertain,
            last_publish: summarize(last_batch.clone()),
            last_batch,
            weeks,
            can_undo: self.last_undoable_publish()?.is_some(),
        })
    }

    /// What the draft becomes after it is sent. Does not write to the database.
    pub fn preview_plan_draft(&self, id: &str, today: NaiveDate) -> Result<DraftPreview> {
        let draft = self
            .plan_draft(id)?
            .ok_or_else(|| plan_error("err.training_plan.draft_not_found", "Plan draft not found"))?;
        let check = check_plan(&draft.document, self.plan_context(today)?);
        let window = Window::starting(today);
        let active: Vec<Workout> = self
            .active_plan_workouts(today)?
            .into_iter()
            .map(|item| item.workout)
            .collect();
        // "Before" follows what the ledger sent (every window is pushed, so every window checks the ledger).
        let mut before: Vec<Workout> = Vec::new();
        for chunk in publish::horizon(today) {
            before.extend(self.sent_for_window(chunk)?.0);
        }
        let after: Vec<Workout> = match (check.from, check.to) {
            (Some(from), Some(to)) if check.publishable() => active
                .iter()
                .filter(|w| w.date < from || w.date > to)
                .cloned()
                .chain(check.workouts.iter().cloned())
                .collect(),
            _ => active.clone(),
        };
        let last = check.to.map_or(window.end(), |to| to.max(window.end()));
        let mut days = Vec::new();
        let mut start = today;
        while start <= last {
            let chunk = Window::starting(start);
            days.extend(
                preview(chunk, &before, &after)
                    .into_iter()
                    .filter(|d| d.date <= last),
            );
            start = chunk.end() + chrono::Duration::days(1);
        }
        for day in &mut days {
            day.rest = check.rest.iter().find(|r| r.date == day.date).cloned();
        }
        Ok(DraftPreview {
            check,
            window,
            days,
        })
    }

    fn active_ids_between(&self, from: NaiveDate, to: NaiveDate) -> Result<Vec<i64>> {
        let mut statement = self.conn.prepare(
            "SELECT id FROM training_plan_workouts
             WHERE active = 1 AND workout_date >= ?1 AND workout_date <= ?2 ORDER BY id",
        )?;
        let rows = statement.query_map([from.to_string(), to.to_string()], |row| row.get(0))?;
        rows.map(|row| row.map_err(Into::into)).collect()
    }

    fn set_active(&self, ids: &[i64], active: bool) -> Result<()> {
        let mut statement = self
            .conn
            .prepare("UPDATE training_plan_workouts SET active = ?2 WHERE id = ?1")?;
        for id in ids {
            statement.execute(params![id, i64::from(active)])?;
        }
        Ok(())
    }

    /// The user disconnected the official authorization: the official side deletes all synced plans (an empty array can no longer be sent).
    /// Deactivate all active plans locally and record one `revoked` row. Earlier pushes no longer count as "on the watch".
    pub fn forget_plans_after_revoke(&self, today: NaiveDate) -> Result<()> {
        let transaction = self.conn.unchecked_transaction()?;
        let now = Utc::now().to_rfc3339();
        let active: Vec<i64> = {
            let mut statement = self
                .conn
                .prepare("SELECT id FROM training_plan_workouts WHERE active = 1")?;
            let rows = statement.query_map([], |row| row.get(0))?;
            rows.collect::<std::result::Result<_, _>>()?
        };
        self.set_active(&active, false)?;
        self.conn.execute(
            "INSERT INTO training_plan_publishes
                (kind, window_start, body, deactivated, state, created_at, finished_at)
             VALUES('revoked', ?1, 'null', ?2, 'sent', ?3, ?3)",
            params![today.to_string(), ids_json(&active), now],
        )?;
        transaction.commit()?;
        Ok(())
    }
}

const PUBLISH_COLUMNS: &str = "SELECT id, kind, window_start, window_ids, state, http_status,
        error_code, undone, created_at, finished_at, COALESCE(batch_id, id),
        COALESCE(role, 'window') FROM training_plan_publishes";

fn publish_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<PublishRecord> {
    Ok(PublishRecord {
        id: row.get(0)?,
        batch_id: row.get(10)?,
        kind: row.get(1)?,
        role: row.get(11)?,
        window_start: row.get(2)?,
        workout_count: ids_from(&row.get::<_, String>(3)?).len(),
        state: row.get(4)?,
        http_status: row
            .get::<_, Option<i64>>(5)?
            .and_then(|v| u16::try_from(v).ok()),
        error_code: row.get(6)?,
        undone: row.get::<_, i64>(7)? != 0,
        created_at: row.get(8)?,
        finished_at: row.get(9)?,
    })
}

/// The summary row of one batch: id, kind and undo flag come from the first row in the batch, the status is the worst one, and the counts are added up.
fn summarize(rows: Vec<PublishRecord>) -> Option<PublishRecord> {
    let mut head = rows.first()?.clone();
    let has = |state: &str| rows.iter().any(|row| row.state == state);
    head.state = if has("pending") {
        "pending"
    } else if has("unknown") {
        "unknown"
    } else if rows.iter().all(|row| row.state == "rejected") {
        "rejected"
    } else if has("rejected") {
        "partial"
    } else {
        "sent"
    }
    .into();
    head.workout_count = rows.iter().map(|row| row.workout_count).sum();
    head.window_start = rows
        .iter()
        .map(|row| row.window_start.clone())
        .min()
        .unwrap_or_default();
    let failed = rows
        .iter()
        .find(|row| matches!(row.state.as_str(), "rejected" | "unknown"));
    head.error_code = failed.and_then(|row| row.error_code.clone());
    head.http_status = failed.and_then(|row| row.http_status);
    head.finished_at = if has("pending") {
        None
    } else {
        rows.iter().filter_map(|row| row.finished_at.clone()).max()
    };
    Some(head)
}

fn draft_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Result<PlanDraft>> {
    let document: String = row.get(3)?;
    Ok((|| {
        Ok(PlanDraft {
            id: row.get(0)?,
            origin: DraftOrigin::parse(&row.get::<_, String>(1)?),
            status: row.get(2)?,
            document: serde_json::from_str(&document)?,
            created_at: row.get(4)?,
            updated_at: row.get(5)?,
        })
    })())
}
