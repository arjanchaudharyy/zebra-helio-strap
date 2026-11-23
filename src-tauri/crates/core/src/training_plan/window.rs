//! The 7-day window and a day-by-day preview of what this push changes.
//!
//! Official rule: `startDate` and `endDate` are both included, and `endDate` must be exactly `startDate + 6`.
//! The window cannot be split. A change to one day also sends all 7 days, and the 7 days sent replace, as a whole,
//! the 7 days under ZeppBridge.

use super::{Workout, WINDOW_DAYS};
use chrono::{Duration, NaiveDate};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Window {
    pub start: NaiveDate,
}

impl Window {
    pub fn starting(start: NaiveDate) -> Self {
        Self { start }
    }

    pub fn end(self) -> NaiveDate {
        self.start + Duration::days(WINDOW_DAYS - 1)
    }

    pub fn contains(self, date: NaiveDate) -> bool {
        date >= self.start && date <= self.end()
    }

    pub fn days(self) -> impl Iterator<Item = NaiveDate> {
        (0..WINDOW_DAYS).map(move |offset| self.start + Duration::days(offset))
    }
}

/// The difference on one day before and after the push.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DayChange {
    /// No workout before or after the push.
    Rest,
    Unchanged,
    Added,
    Replaced,
    Removed,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DayPreview {
    pub rest: Option<super::RestDay>,
    pub date: NaiveDate,
    pub change: DayChange,
    pub before: Vec<Workout>,
    pub after: Vec<Workout>,
}

/// Compare day by day "what the watch has now (by the ledger)" and "what this push sends".
///
/// `before` can only come from our own publish ledger. The official API has no read endpoint, and we do not know what the user deleted
/// elsewhere. So the UI must say "last sent", not "on the watch now".
pub fn preview(window: Window, before: &[Workout], after: &[Workout]) -> Vec<DayPreview> {
    let on = |list: &[Workout], date: NaiveDate| -> Vec<Workout> {
        list.iter().filter(|w| w.date == date).cloned().collect()
    };
    window
        .days()
        .map(|date| {
            let before = on(before, date);
            let after = on(after, date);
            let change = match (before.is_empty(), after.is_empty()) {
                (true, true) => DayChange::Rest,
                (true, false) => DayChange::Added,
                (false, true) => DayChange::Removed,
                (false, false) if before == after => DayChange::Unchanged,
                (false, false) => DayChange::Replaced,
            };
            DayPreview {
                rest: None,
                date,
                change,
                before,
                after,
            }
        })
        .collect()
}
