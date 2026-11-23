//! Publish engine: works out per day "what we sent last time", computes which windows to push this time and how, and records the result.
//!
//! The server semantics are **per-day replacement**: pushing a window `[s, s+6]` replaces all 7 days under ZeppBridge with
//! the workouts in the payload. Other days stay the same. Windows are cut from "today" again each day, so the ledger works per day: for a given day,
//! what we sent last time is the latest row that covers that day (`sent` / `unknown` / `pending`).
//!
//! The order of one push (one batch): from the farthest window toward today. When a window must become empty:
//!
//! - The current window (from today): send `[]` (it clears only the 7 days from today, which is exactly this window).
//! - A future window: send a placeholder payload (`v2::clear_future_window`). The placeholder lands on the day before the window, so
//!   **push the previous window again right after** to overwrite the placeholder. If the previous window is also empty, apply the same rule
//!   and keep going back until the current window.
//!
//! The ledger also records the placeholder row as "one placeholder on the day before the window". If the repush of the previous window does not arrive, that day
//! does not match the active plan. The UI marks it "uncertain", and the rolling push sends it again to overwrite it.

use super::{
    ids_from, ids_json, plan_error, Database, PendingSend, Prepared, PublishRequest, SendOutcome,
    SendRole, UndoTarget, WeekState, WeekStatus,
};
use crate::models::error::Result;
use crate::training_plan::v2::{clear_future_window, window_body, NumberedWorkout, WatchLocale};
use crate::training_plan::window::Window;
use crate::training_plan::{check_plan, Workout, MAX_DAYS_AHEAD, WINDOW_DAYS};
use chrono::{Duration, NaiveDate, Utc};
use rusqlite::{params, OptionalExtension};
use std::collections::BTreeMap;

/// Placeholder workouts take their `workoutId` from here (plus the ledger ID of their row). Real workout IDs are
/// `training_plan_workouts` row IDs and never reach one billion. The value stays within a 32-bit integer.
pub const PLACEHOLDER_ID_BASE: i64 = 1_000_000_000;

/// The language for the subtype in the description sent to the watch: the UI language at publish time. Rolling pushes reuse the last one.
const WATCH_LOCALE_KEY: &str = "training_plan_watch_locale";

/// All windows cut into 7 days from today, up to the farthest day the plan can schedule.
pub(super) fn horizon(today: NaiveDate) -> Vec<Window> {
    let last = today + Duration::days(MAX_DAYS_AHEAD);
    let mut windows = Vec::new();
    let mut start = today;
    while start <= last {
        windows.push(Window::starting(start));
        start += Duration::days(WINDOW_DAYS);
    }
    windows
}

/// For each day in a window, the workout ID we sent last time (from the ledger).
struct SentDays {
    days: BTreeMap<NaiveDate, Vec<i64>>,
    /// Some of the rows that decide these days have an uncertain result.
    uncertain: bool,
}

impl SentDays {
    fn real_count(&self) -> usize {
        self.days
            .values()
            .flatten()
            .filter(|id| **id < PLACEHOLDER_ID_BASE)
            .count()
    }
}

/// The active plan's workout ID for each day in a window.
fn target_days(window: Window, active: &[NumberedWorkout]) -> BTreeMap<NaiveDate, Vec<i64>> {
    let mut days: BTreeMap<NaiveDate, Vec<i64>> =
        window.days().map(|day| (day, Vec::new())).collect();
    for item in active {
        if let Some(list) = days.get_mut(&item.workout.date) {
            list.push(item.id);
        }
    }
    for list in days.values_mut() {
        list.sort_unstable();
    }
    days
}

/// Whether this window needs a push this time, whether it will be empty after the push, and whether the ledger had a real workout there.
struct WindowPlan {
    window: Window,
    differs: bool,
    target_empty: bool,
    had_real: bool,
    /// The last send of this window in the ledger has no confirmed result (network loss / timeout / partial send).
    uncertain: bool,
}

impl Database {
    pub(super) fn watch_locale(&self) -> Result<WatchLocale> {
        Ok(self
            .get_app_meta(WATCH_LOCALE_KEY)?
            .map(|tag| WatchLocale::from_tag(&tag))
            .unwrap_or_default())
    }

    /// Record the UI language at publish time. The subtype in the first line of the description uses it.
    pub fn set_plan_watch_locale(&self, tag: &str) -> Result<()> {
        self.set_app_meta(WATCH_LOCALE_KEY, WatchLocale::from_tag(tag).tag())
    }

    /// The source text of the workouts these rows store (in ID order).
    fn workout_texts(&self, ids: &[i64]) -> Result<Vec<(i64, String)>> {
        let mut statement = self
            .conn
            .prepare("SELECT workout FROM training_plan_workouts WHERE id = ?1")?;
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(text) = statement
                .query_row([id], |row| row.get::<_, String>(0))
                .optional()?
            {
                out.push((*id, text));
            }
        }
        Ok(out)
    }

    fn workout_dates(&self, ids: &[i64]) -> Result<Vec<(i64, NaiveDate)>> {
        let mut statement = self
            .conn
            .prepare("SELECT workout_date FROM training_plan_workouts WHERE id = ?1")?;
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            let date: Option<String> = statement.query_row([id], |row| row.get(0)).optional()?;
            if let Some(date) = date.and_then(|d| NaiveDate::parse_from_str(&d, "%Y-%m-%d").ok()) {
                out.push((*id, date));
            }
        }
        Ok(out)
    }

    fn sent_days(&self, window: Window) -> Result<SentDays> {
        let earliest = window.start - Duration::days(WINDOW_DAYS - 1);
        // A placeholder lands on the day before its window, so placeholder rows starting at the next window's start also touch this window.
        let latest = window.end() + Duration::days(1);
        let mut statement = self.conn.prepare(
            "SELECT id, window_start, window_ids, state, COALESCE(role, 'window')
             FROM training_plan_publishes
             WHERE state IN ('sent', 'unknown', 'pending') AND body != 'null'
               AND window_start >= ?1 AND window_start <= ?2
               AND id > COALESCE((SELECT MAX(id) FROM training_plan_publishes
                                  WHERE kind = 'revoked'), 0)
             ORDER BY id DESC",
        )?;
        let rows = statement
            .query_map(params![earliest.to_string(), latest.to_string()], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let mut days: BTreeMap<NaiveDate, Vec<i64>> = BTreeMap::new();
        let mut uncertain = false;
        for (id, start, ids, state, role) in rows {
            let Ok(start) = NaiveDate::parse_from_str(&start, "%Y-%m-%d") else {
                continue;
            };
            let covered = Window::starting(start);
            let placeholder_day =
                (role == SendRole::Placeholder.as_str()).then(|| start - Duration::days(1));
            let open: Vec<NaiveDate> = window
                .days()
                .filter(|day| {
                    !days.contains_key(day)
                        && (covered.contains(*day) || Some(*day) == placeholder_day)
                })
                .collect();
            if open.is_empty() {
                continue;
            }
            if state != "sent" {
                uncertain = true;
            }
            let dated = self.workout_dates(&ids_from(&ids))?;
            for day in open {
                let mut list: Vec<i64> = dated
                    .iter()
                    .filter(|(_, date)| *date == day && covered.contains(day))
                    .map(|(id, _)| *id)
                    .collect();
                if Some(day) == placeholder_day {
                    // The placeholder is still on that day: no later push overwrote it.
                    list.push(PLACEHOLDER_ID_BASE + id);
                    uncertain = true;
                }
                list.sort_unstable();
                days.insert(day, list);
            }
        }
        for day in window.days() {
            days.entry(day).or_default();
        }
        Ok(SentDays { days, uncertain })
    }

    /// For a window, the workouts we sent last time per the ledger, and whether any push that decides them is uncertain.
    pub(super) fn sent_for_window(&self, window: Window) -> Result<(Vec<Workout>, bool)> {
        let sent = self.sent_days(window)?;
        let ids: Vec<i64> = sent
            .days
            .values()
            .flatten()
            .copied()
            .filter(|id| *id < PLACEHOLDER_ID_BASE)
            .collect();
        let mut workouts = self.workouts_by_ids(&ids)?;
        workouts.sort_by_key(|w| w.date);
        Ok((workouts, sent.uncertain))
    }

    pub(super) fn week_statuses(
        &self,
        today: NaiveDate,
        active: &[NumberedWorkout],
    ) -> Result<Vec<WeekStatus>> {
        let mut weeks = Vec::new();
        for window in horizon(today) {
            let target = target_days(window, active);
            let sent = self.sent_days(window)?;
            let planned: usize = target.values().map(Vec::len).sum();
            let sent_count = sent.real_count();
            if planned == 0 && sent_count == 0 && !sent.uncertain {
                continue;
            }
            let state = if target != sent.days {
                WeekState::NotSent
            } else if sent.uncertain {
                WeekState::Uncertain
            } else {
                WeekState::InSync
            };
            let error_code = if state == WeekState::InSync {
                None
            } else {
                self.conn
                    .query_row(
                        "SELECT error_code FROM training_plan_publishes
                         WHERE state IN ('rejected', 'unknown') AND error_code IS NOT NULL
                           AND window_start >= ?1 AND window_start <= ?2
                         ORDER BY id DESC LIMIT 1",
                        params![
                            (window.start - Duration::days(WINDOW_DAYS - 1)).to_string(),
                            window.end().to_string()
                        ],
                        |row| row.get(0),
                    )
                    .optional()?
            };
            weeks.push(WeekStatus {
                start: window.start,
                planned,
                sent: sent_count,
                state,
                error_code,
            });
        }
        Ok(weeks)
    }

    /// Only the latest batch of publish / clear / undo can be undone, and only when nothing came after it. A batch that was fully rejected does not count.
    /// After authorization is revoked there is nothing to undo: the official side already deleted everything.
    pub(super) fn last_undoable_publish(&self) -> Result<Option<UndoTarget>> {
        let row = self
            .conn
            .query_row(
                "SELECT h.id, h.activated, h.deactivated, h.undone, h.kind,
                        EXISTS(SELECT 1 FROM training_plan_publishes b
                               WHERE COALESCE(b.batch_id, b.id) = h.id
                                 AND b.state IN ('sent', 'unknown'))
                 FROM training_plan_publishes h
                 WHERE h.kind IN ('publish', 'clear', 'undo', 'revoked')
                   AND (h.batch_id IS NULL OR h.batch_id = h.id)
                 ORDER BY h.id DESC LIMIT 1",
                [],
                |row| {
                    let undoable = row.get::<_, i64>(3)? == 0
                        && row.get::<_, String>(4)? != "revoked"
                        && row.get::<_, bool>(5)?;
                    Ok(undoable.then(|| -> rusqlite::Result<UndoTarget> {
                        Ok(UndoTarget {
                            id: row.get(0)?,
                            activated: ids_from(&row.get::<_, String>(1)?),
                            deactivated: ids_from(&row.get::<_, String>(2)?),
                        })
                    }))
                },
            )
            .optional()?;
        Ok(row.flatten().transpose()?)
    }

    /// Update the active plan and record each window to send. See the file header.
    ///
    /// `confirm_clear`: the user already confirmed the clear when this push empties windows that had workouts.
    pub fn prepare_plan_publish(
        &self,
        request: &PublishRequest,
        today: NaiveDate,
        confirm_clear: bool,
    ) -> Result<Prepared> {
        let transaction = self.conn.unchecked_transaction()?;
        let prepared = self.prepare_in_transaction(request, today, confirm_clear)?;
        if matches!(prepared, Prepared::Send { .. } | Prepared::NotNeeded) {
            transaction.commit()?;
        }
        // Any other result keeps nothing: the transaction drops here and rolls back.
        Ok(prepared)
    }

    fn prepare_in_transaction(
        &self,
        request: &PublishRequest,
        today: NaiveDate,
        confirm_clear: bool,
    ) -> Result<Prepared> {
        let now = Utc::now().to_rfc3339();
        let current = Window::starting(today);
        let mut activated: Vec<i64> = Vec::new();
        let mut deactivated: Vec<i64> = Vec::new();
        let mut draft_id: Option<String> = None;

        match request {
            PublishRequest::Draft { id } => {
                let draft = self.plan_draft(id)?.ok_or_else(|| {
                    plan_error("err.training_plan.draft_not_found", "Plan draft not found")
                })?;
                if draft.status != "open" {
                    return Err(plan_error(
                        "err.training_plan.draft_closed",
                        "This plan draft was already sent or discarded",
                    ));
                }
                let check = check_plan(&draft.document, self.plan_context(today)?);
                let (Some(from), Some(to)) = (check.from, check.to) else {
                    return Ok(Prepared::Invalid { check });
                };
                if !check.publishable() {
                    return Ok(Prepared::Invalid { check });
                }
                let replaced = self.active_ids_between(from, to)?;
                // A workout identical to the old one (edited after sending, only some days changed) reuses the old row: the ID stays the same,
                // so the ledger for those days matches what we sent last time, and unchanged weeks need no repush.
                let mut keep: Vec<(i64, String)> = self.workout_texts(&replaced)?;
                let mut kept: Vec<i64> = Vec::new();
                for workout in &check.workouts {
                    let text = serde_json::to_string(workout)?;
                    if let Some(at) = keep.iter().position(|(_, old)| *old == text) {
                        kept.push(keep.remove(at).0);
                        continue;
                    }
                    self.conn.execute(
                        "INSERT INTO training_plan_workouts(draft_id, workout_date, workout, active, created_at)
                         VALUES(?1, ?2, ?3, 1, ?4)",
                        params![id, workout.date.to_string(), text, now],
                    )?;
                    activated.push(self.conn.last_insert_rowid());
                }
                deactivated = replaced
                    .into_iter()
                    .filter(|old| !kept.contains(old))
                    .collect();
                self.set_active(&deactivated, false)?;
                self.conn.execute(
                    "UPDATE training_plan_drafts SET status = 'published', updated_at = ?2 WHERE id = ?1",
                    params![id, now],
                )?;
                draft_id = Some(id.clone());
            }
            PublishRequest::UndoLast => {
                let Some(UndoTarget {
                    id: publish_id,
                    activated: was_activated,
                    deactivated: was_deactivated,
                }) = self.last_undoable_publish()?
                else {
                    return Ok(Prepared::NothingToUndo);
                };
                self.set_active(&was_activated, false)?;
                self.set_active(&was_deactivated, true)?;
                self.conn.execute(
                    "UPDATE training_plan_publishes SET undone = 1 WHERE id = ?1",
                    [publish_id],
                )?;
                // This undo can itself be undone: record the flip in the opposite direction.
                activated = was_deactivated;
                deactivated = was_activated;
            }
            PublishRequest::Clear => {
                if !confirm_clear {
                    return Ok(Prepared::NeedsClearConfirmation);
                }
                // Clear the workouts ZeppBridge sent in **all** windows from today, not only this week (user feedback:
                // when we cleared only this week, next week's workouts stayed on the watch, and the phone had no place to delete them).
                // Future windows are cleared by the placeholder below + a repush of the previous window (v2::clear_future_window).
                let last = horizon(today).last().map_or(current.end(), |w| w.end());
                deactivated = self.active_ids_between(current.start, last)?;
                self.set_active(&deactivated, false)?;
            }
            PublishRequest::Roll => {}
        }

        let active = self.active_plan_workouts(today)?;
        let mut plans = Vec::new();
        for window in horizon(today) {
            let target = target_days(window, &active);
            let sent = self.sent_days(window)?;
            plans.push(WindowPlan {
                window,
                differs: target != sent.days,
                target_empty: target.values().all(Vec::is_empty),
                had_real: sent.real_count() > 0,
                uncertain: sent.uncertain,
            });
        }
        let roll = matches!(request, PublishRequest::Roll);
        let clears_real = plans
            .iter()
            .any(|plan| plan.differs && plan.target_empty && plan.had_real);
        if clears_real && !roll && !confirm_clear {
            return Ok(Prepared::NeedsClearConfirmation);
        }

        // From the farthest window toward today. The automatic rolling push never clears a window.
        //
        // Windows with no confirmed delivery are sent again too (issue #143): the ledger counts `unknown` as "sent",
        // so sending an identical draft again only gave `not_needed`, and the user had to change one character to force a resend.
        // Full-window replacement is idempotent, so a resend does not add extra workouts on the watch.
        let mut must: Vec<bool> = plans
            .iter()
            .map(|plan| (plan.differs || plan.uncertain) && !(roll && plan.target_empty))
            .collect();
        if matches!(request, PublishRequest::Clear) {
            // A clear the user confirmed: send it even when the ledger says empty. When the server and the ledger disagree, this aligns them.
            must[0] = true;
        }
        let mut order: Vec<(Window, SendRole)> = Vec::new();
        for k in (0..plans.len()).rev() {
            if !must[k] {
                continue;
            }
            let plan = &plans[k];
            let role = if !plan.target_empty {
                SendRole::Window
            } else if k == 0 {
                SendRole::Clear
            } else {
                // The placeholder lands in the previous window, so the previous window must be pushed again right after.
                must[k - 1] = true;
                SendRole::Placeholder
            };
            order.push((plan.window, role));
        }
        if order.is_empty() {
            return self.record_not_needed(request, draft_id, current, &activated, &deactivated);
        }

        let locale = self.watch_locale()?;
        let mut sends = Vec::with_capacity(order.len());
        let mut batch_id: Option<i64> = None;
        for (window, role) in order {
            let window_ids: Vec<i64> = match role {
                SendRole::Window => active
                    .iter()
                    .filter(|item| window.contains(item.workout.date))
                    .map(|item| item.id)
                    .collect(),
                SendRole::Clear | SendRole::Placeholder => Vec::new(),
            };
            // Which rows flipped and which draft they came from is recorded only on the first row of this batch.
            let head = batch_id.is_none();
            self.conn.execute(
                "INSERT INTO training_plan_publishes
                    (kind, draft_id, window_start, window_ids, body, activated, deactivated,
                     state, created_at, batch_id, role)
                 VALUES(?1, ?2, ?3, ?4, '{}', ?5, ?6, 'pending', ?7, ?8, ?9)",
                params![
                    request.kind(),
                    draft_id,
                    window.start.to_string(),
                    ids_json(&window_ids),
                    ids_json(if head { &activated } else { &[] }),
                    ids_json(if head { &deactivated } else { &[] }),
                    now,
                    batch_id,
                    role.as_str(),
                ],
            )?;
            let publish_id = self.conn.last_insert_rowid();
            let batch = *batch_id.get_or_insert(publish_id);
            let body = match role {
                SendRole::Window => window_body(window, &active, locale),
                SendRole::Clear => window_body(window, &[], locale),
                SendRole::Placeholder => {
                    clear_future_window(window, PLACEHOLDER_ID_BASE + publish_id)
                }
            };
            self.conn.execute(
                "UPDATE training_plan_publishes SET body = ?2, batch_id = ?3 WHERE id = ?1",
                params![publish_id, serde_json::to_string(&body)?, batch],
            )?;
            sends.push(PendingSend {
                publish_id,
                window_start: window.start,
                role,
                body,
            });
        }
        Ok(Prepared::Send {
            batch_id: batch_id.unwrap_or_default(),
            sends,
        })
    }

    /// The active plan changed, but no window needs a resend (each window's content matches what we sent last time).
    /// Publish and undo still record a row (status is `sent` at once, payload is `null`), so undo can find it.
    /// Rows like this are ignored when we work out "what we sent".
    fn record_not_needed(
        &self,
        request: &PublishRequest,
        draft_id: Option<String>,
        window: Window,
        activated: &[i64],
        deactivated: &[i64],
    ) -> Result<Prepared> {
        if activated.is_empty() && deactivated.is_empty() {
            return Ok(Prepared::NotNeeded);
        }
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO training_plan_publishes
                (kind, draft_id, window_start, window_ids, body, activated, deactivated, state,
                 created_at, finished_at, role)
             VALUES(?1, ?2, ?3, '[]', 'null', ?4, ?5, 'sent', ?6, ?6, 'window')",
            params![
                request.kind(),
                draft_id,
                window.start.to_string(),
                ids_json(activated),
                ids_json(deactivated),
                now
            ],
        )?;
        self.conn.execute(
            "UPDATE training_plan_publishes SET batch_id = id WHERE id = ?1",
            [self.conn.last_insert_rowid()],
        )?;
        Ok(Prepared::NotNeeded)
    }

    /// Record the send result of one window. When the whole batch has results and no window took effect (all rejected),
    /// flip this batch's changes back as they were, and the draft returns to "not sent". If some windows arrived, do not flip: the active plan
    /// stays new, the UI marks the weeks that did not arrive, and the rolling push fills them.
    pub fn finish_plan_publish(&self, publish_id: i64, outcome: &SendOutcome) -> Result<()> {
        let transaction = self.conn.unchecked_transaction()?;
        let now = Utc::now().to_rfc3339();
        let row: Option<(String, i64)> = self
            .conn
            .query_row(
                "SELECT state, COALESCE(batch_id, id) FROM training_plan_publishes WHERE id = ?1",
                [publish_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((state, batch_id)) = row else {
            return Err(plan_error(
                "err.training_plan.publish_not_found",
                "Push record not found",
            ));
        };
        if state != "pending" {
            return Ok(());
        }
        match outcome {
            SendOutcome::Delivered => {
                self.conn.execute(
                    "UPDATE training_plan_publishes SET state = 'sent', finished_at = ?2 WHERE id = ?1",
                    params![publish_id, now],
                )?;
            }
            SendOutcome::Unknown { error_code } => {
                self.conn.execute(
                    "UPDATE training_plan_publishes SET state = 'unknown', error_code = ?2,
                        finished_at = ?3 WHERE id = ?1",
                    params![publish_id, error_code, now],
                )?;
            }
            SendOutcome::Rejected {
                http_status,
                error_code,
            } => {
                self.conn.execute(
                    "UPDATE training_plan_publishes SET state = 'rejected', http_status = ?2,
                        error_code = ?3, finished_at = ?4 WHERE id = ?1",
                    params![publish_id, http_status, error_code, now],
                )?;
            }
        }
        let (pending, effective): (i64, i64) = self.conn.query_row(
            "SELECT COALESCE(SUM(state = 'pending'), 0), COALESCE(SUM(state IN ('sent', 'unknown')), 0)
             FROM training_plan_publishes WHERE COALESCE(batch_id, id) = ?1",
            [batch_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        if pending == 0 && effective == 0 {
            self.roll_back_batch(batch_id, &now)?;
        }
        if pending == 0 {
            self.exchange_publish(batch_id)?;
        }
        transaction.commit()?;
        Ok(())
    }

    fn roll_back_batch(&self, batch_id: i64, now: &str) -> Result<()> {
        let (kind, draft_id, activated, deactivated): (String, Option<String>, String, String) =
            self.conn.query_row(
                "SELECT kind, draft_id, activated, deactivated FROM training_plan_publishes
                 WHERE id = ?1",
                [batch_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )?;
        self.set_active(&ids_from(&activated), false)?;
        self.set_active(&ids_from(&deactivated), true)?;
        if kind == "undo" {
            // Undo rejected: the undone batch goes back to "not undone".
            self.conn.execute(
                "UPDATE training_plan_publishes SET undone = 0
                 WHERE id = (SELECT MAX(id) FROM training_plan_publishes
                             WHERE id < ?1 AND undone = 1)",
                [batch_id],
            )?;
        }
        if let Some(draft_id) = draft_id {
            self.conn.execute(
                "UPDATE training_plan_drafts SET status = 'open', updated_at = ?2 WHERE id = ?1",
                params![draft_id, now],
            )?;
        }
        Ok(())
    }
}
