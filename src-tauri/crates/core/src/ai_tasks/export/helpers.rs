//! Merges time windows, cleans text and file names, checks attachments, builds task warnings (split out of ai_tasks/export.rs, logic unchanged).

use super::*;

/// Merges the anchor window rows of one category into day buckets. One day can be covered by several anchor
/// windows. `(category, date)` appears only once, and `linked` records all anchors that cover it.
/// The same entry on the same day (same metric / sleep_id / workout_id) is deduplicated.
pub(super) fn merge_window_gathers(
    category: AiTaskCategory,
    gathers: Vec<WindowGather>,
) -> BTreeMap<String, DayAcc> {
    let mut days: BTreeMap<String, DayAcc> = BTreeMap::new();
    let mut seen: BTreeSet<(String, String)> = BTreeSet::new();
    for gather in gathers {
        for (day, key, row) in gather.rows {
            let acc = days.entry(day.clone()).or_default();
            if let Some(workout_id) = &gather.workout_id {
                acc.linked.insert(workout_id.clone());
            }
            if !seen.insert((day, key)) {
                continue;
            }
            match category {
                AiTaskCategory::Workout => acc.workouts.push(row),
                AiTaskCategory::Sleep => acc.sleeps.push(row),
                _ => acc.metrics.push(row),
            }
        }
    }
    days
}

/// Window data bucket for one day. `linked` records all anchor workout ids that cover this day.
#[derive(Default)]
pub(crate) struct DayAcc {
    pub linked: BTreeSet<String>,
    pub metrics: Vec<Value>,
    pub sleeps: Vec<Value>,
    pub workouts: Vec<Value>,
}

/// The effective id of a task in IPC/directories: a draft (empty id) is called `draft`.
pub(super) fn effective_task_id(task: &AiTask) -> String {
    let id = task.id.trim();
    if id.is_empty() {
        DRAFT_TASK_ID.to_string()
    } else {
        id.to_string()
    }
}

/// Local path cleaning for outgoing user text: uses the same code in `crate::redact` as the AI export.
pub(super) fn sanitize_export_text(text: &str) -> String {
    crate::redact::sanitize_local_paths(text)
}

/// File/directory name segment: keeps readable characters such as Chinese, and folds only the Windows-forbidden
/// `<>:"/\|?*` and control characters into `_`. Trims spaces and `.` at both ends (Windows
/// drops a trailing dot), prefixes reserved device names (CON, COM1...) with `_`, cuts to 60 characters,
/// and falls back to `fallback` when empty.
pub(super) fn sanitize_file_name(name: &str, fallback: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') {
                '_'
            } else {
                c
            }
        })
        .collect();
    let trimmed: String = cleaned
        .trim_matches(|c: char| c == '.' || c.is_whitespace())
        .chars()
        .take(60)
        .collect();
    let trimmed = trimmed.trim_end_matches(|c: char| c == '.' || c.is_whitespace());
    if trimmed.is_empty() {
        return fallback.to_string();
    }
    let stem = trimmed
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.len() == 4
            && stem.as_bytes()[3].is_ascii_digit());
    if reserved {
        format!("_{trimmed}")
    } else {
        trimmed.to_string()
    }
}

/// Export folder name: `<task name>_<yyyyMMdd-HHmm>`. Each export makes a new folder,
/// so it does not overwrite the previous one given to the AI. When the title is empty, use the task id (a draft is `draft`).
pub(super) fn export_folder_name(task: &AiTask, task_id: &str, now: DateTime<Local>) -> String {
    let base = if task.title.trim().is_empty() {
        task_id
    } else {
        task.title.trim()
    };
    format!(
        "{}_{}",
        sanitize_file_name(base, "task"),
        now.format("%Y%m%d-%H%M")
    )
}

/// Attachment check shared by preview/prepare: `missing` (not found), `changed`
/// (measured size differs from the `byte_len` baseline recorded when added).
///
/// A `byte_len=None` baseline means "size not recorded when added". It can only tell if the file exists,
/// not if it changed, so existing means `ok`.
pub(crate) fn stat_task_attachments(
    attachments: &[AiTaskAttachmentRef],
) -> Vec<AiTaskAttachmentStatus> {
    attachments
        .iter()
        .map(|attachment| {
            let meta = std::fs::metadata(&attachment.path).ok();
            let byte_len = meta.as_ref().map(|m| m.len() as i64);
            let status = match (&meta, attachment.byte_len) {
                (None, _) => AiTaskAttachmentState::Missing,
                (Some(_), Some(baseline)) if baseline != byte_len.unwrap_or_default() => {
                    AiTaskAttachmentState::Changed
                }
                (Some(_), _) => AiTaskAttachmentState::Ok,
            };
            AiTaskAttachmentStatus {
                id: attachment.id.clone(),
                status,
                byte_len,
            }
        })
        .collect()
}

/// Preview warnings: a status, not an error. They use the `ui.ai_task.*` / finalized
/// `err.ai_task.attachment_missing` codes. The code table follows the registry in S4 `copy.ts`.
/// Add a new code there first.
///
/// Coverage warnings fold by category: all windows of a category have no data -> one
/// `warn.category_missing`. Data exists but does not cover everything -> `warn.partial_coverage`.
/// The merged row (`workout_id=None`) wins over per-workout rows, so one category does not show twice.
pub(super) fn task_warnings(
    task: &AiTask,
    coverage: &[AiTaskCoverage],
    attachments: &[AiTaskAttachmentStatus],
) -> Vec<AiTaskIssue> {
    let mut warnings = Vec::new();
    for (attachment, status) in task.attachments.iter().zip(attachments.iter()) {
        let name = attachment.display_name.clone();
        let params = || serde_json::json!({ "display_name": name });
        match status.status {
            AiTaskAttachmentState::Missing => warnings.push(
                AiTaskIssue::new(
                    "err.ai_task.attachment_missing",
                    format!("Attachment file is no longer at its original location: {name}"),
                )
                .with_params(params()),
            ),
            AiTaskAttachmentState::Changed => warnings.push(
                AiTaskIssue::new(
                    "ui.ai_task.warn.attachment_changed",
                    format!("Attachment content differs from when it was added: {name}"),
                )
                .with_params(params()),
            ),
            AiTaskAttachmentState::Ok => {}
        }
    }
    // Coverage warnings fold by category: the merged row (workout_id=None) is the deduplicated truth,
    // so use it first. Aggregate only when there are only per-workout rows (both are equal when windows do not overlap).
    let mut by_category: BTreeMap<AiTaskCategory, Vec<&AiTaskCoverage>> = BTreeMap::new();
    for row in coverage {
        by_category.entry(row.category).or_default().push(row);
    }
    for (category, rows) in by_category {
        let (days_with_data, days_in_range, missing) =
            match rows.iter().find(|row| row.workout_id.is_none()) {
                Some(row) => (row.days_with_data, row.days_in_range, row.missing),
                None => (
                    rows.iter().map(|row| row.days_with_data).sum(),
                    rows.iter().map(|row| row.days_in_range).sum(),
                    rows.iter().all(|row| row.missing),
                ),
            };
        if missing {
            warnings.push(
                AiTaskIssue::new(
                    "ui.ai_task.warn.category_missing",
                    "This category has no data in the selected time window. The export marks it as missing",
                )
                .with_params(serde_json::json!({ "category": category })),
            );
        } else if days_with_data < days_in_range {
            warnings.push(
                AiTaskIssue::new(
                    "ui.ai_task.warn.partial_coverage",
                    "Only some dates in the time window have data",
                )
                .with_params(serde_json::json!({
                    "category": category,
                    "days_with_data": days_with_data,
                    "days_in_range": days_in_range,
                })),
            );
        }
    }
    warnings
}
