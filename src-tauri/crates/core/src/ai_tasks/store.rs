//! Read, write and validate `ai_tasks` / `ai_task_templates`.
//!
//! Tables are "JSON payload + index columns": lists and grant filters use the columns; the full object uses `payload`.
//! Writes happen only after the command layer takes the `WritePurpose::Metadata` write lock. This file assumes
//! the caller already holds the write lock (same rule as `life_events`).

use super::model::*;
use super::AiTaskError;
use crate::models::error::{Result, ZeppBridgeError};
use crate::storage::Database;
use chrono::{NaiveDate, Utc};
use rusqlite::{params, OptionalExtension};
use std::collections::BTreeSet;
use std::path::Path;

// Input limits. These are not product rules. They are the floor that stops one bad input from bloating the database or freezing the UI.
// A window of 64 workouts x 365 days already covers any real task.
const MAX_TITLE_CHARS: usize = 200;
const MAX_NAME_CHARS: usize = 200;
const MAX_PROMPT_CHARS: usize = 16_000;
const MAX_NOTE_CHARS: usize = 4_000;
const MAX_WORKOUT_IDS: usize = 64;
const MAX_ID_CHARS: usize = 200;
const MAX_ATTACHMENTS: usize = 16;
const MAX_PATH_CHARS: usize = 1_024;
const MAX_CATEGORIES: usize = 16;
/// Lookback limit: "All" = ten years (3.0.4, same as the backfill limit `MAX_HISTORY_SYNC_DAYS`).
const MAX_DAYS_BEFORE: i64 = 3650;
const MAX_EXCLUDED_METRICS: usize = 64;
/// Maximum picked days per category: enough to pick every day of the ten calendar years of "All" (3.0.4).
const MAX_PICKED_DAYS: usize = 3700;

fn random_hex(bytes: usize) -> String {
    let mut buffer = vec![0u8; bytes];
    if getrandom::getrandom(&mut buffer).is_err() {
        // If the entropy source fails, fall back to the timestamp. The random prefix prevents collisions, not guessing.
        return format!("{:x}", Utc::now().timestamp_nanos_opt().unwrap_or_default());
    }
    hex::encode(buffer)
}

/// Validate and normalize a task before save. Invalid -> `err.ai_task.invalid`.
///
/// Normalization does three things: pins schema_version to 1, dedupes workout_ids/categories
/// (keeping first-seen order), and turns an empty `template_id` into `None`.
/// `id`/`created_at`/`updated_at` are not touched here. `save_ai_task` owns them.
pub(crate) fn normalize_task(task: &AiTask) -> Result<AiTask> {
    normalize_task_impl(task, true)
}

/// Draft validation for preview/prepare: the same rules, but an empty title and empty id are allowed.
/// An unsaved draft must also be previewable.
pub(crate) fn normalize_task_draft(task: &AiTask) -> Result<AiTask> {
    normalize_task_impl(task, false)
}

fn normalize_task_impl(task: &AiTask, require_title: bool) -> Result<AiTask> {
    let invalid = |message: &str| AiTaskError::invalid_task(message);
    let mut task = task.clone();
    // Drafts from old UIs / old MCP clients have only `heart_rate`: add `resting_hr` first so the rules match.
    upgrade_heart_rate_split(&mut task.categories);

    if require_title && task.title.trim().is_empty() {
        return Err(invalid("The task title must not be empty"));
    }
    if task.id.chars().count() > MAX_ID_CHARS {
        return Err(invalid("The task id is too long"));
    }
    if task.title.chars().count() > MAX_TITLE_CHARS {
        return Err(invalid("The task title is too long"));
    }
    if task.prompt.chars().count() > MAX_PROMPT_CHARS {
        return Err(invalid("The prompt is too long"));
    }
    if task.personal_note.chars().count() > MAX_NOTE_CHARS {
        return Err(invalid("The personal note is too long"));
    }
    if task.workout_ids.len() > MAX_WORKOUT_IDS {
        return Err(invalid("A task can link at most 64 workouts"));
    }
    let mut seen_ids = BTreeSet::new();
    task.workout_ids.retain(|id| {
        let id = id.trim();
        !id.is_empty() && id.chars().count() <= MAX_ID_CHARS && seen_ids.insert(id.to_string())
    });
    // retain gets a borrow of the original string, and the dedup check uses a trimmed copy.
    // Here each id itself is also normalized to its trimmed form.
    for id in &mut task.workout_ids {
        *id = id.trim().to_string();
    }

    normalize_categories(&mut task.categories).map_err(|m| invalid(&m))?;

    if task.attachments.len() > MAX_ATTACHMENTS {
        return Err(invalid("At most 16 attachments are allowed"));
    }
    for attachment in &task.attachments {
        if attachment.id.trim().is_empty()
            || attachment.path.trim().is_empty()
            || attachment.display_name.trim().is_empty()
        {
            return Err(invalid("An attachment is missing its id, path or file name"));
        }
        if attachment.path.chars().count() > MAX_PATH_CHARS
            || attachment.display_name.chars().count() > MAX_NAME_CHARS
            || attachment.id.chars().count() > MAX_ID_CHARS
        {
            return Err(invalid("An attachment field is too long"));
        }
        if attachment.byte_len.is_some_and(|len| len < 0) {
            return Err(invalid("The attachment size must not be negative"));
        }
    }

    task.template_id = task
        .template_id
        .take()
        .and_then(|id| (!id.trim().is_empty()).then_some(id));
    task.schema_version = AI_TASK_SCHEMA_VERSION;
    Ok(task)
}

/// Validate and normalize a template before save. Invalid -> `err.ai_template.invalid`.
pub(crate) fn normalize_template(template: &AiTaskTemplate) -> Result<AiTaskTemplate> {
    let invalid = |message: &str| AiTaskError::invalid_template(message);
    let mut template = template.clone();

    if template.name.trim().is_empty() {
        return Err(invalid("The template name must not be empty"));
    }
    if template.name.chars().count() > MAX_NAME_CHARS {
        return Err(invalid("The template name is too long"));
    }
    if template.prompt_template.chars().count() > MAX_PROMPT_CHARS {
        return Err(invalid("The template prompt is too long"));
    }
    normalize_categories(&mut template.categories).map_err(|m| invalid(&m))?;

    template.schema_version = AI_TASK_SCHEMA_VERSION;
    Ok(template)
}

/// Shared checks for categories: dedupe (keep the first of each category) and bound days_before.
fn normalize_categories(
    categories: &mut Vec<AiTaskCategoryRange>,
) -> std::result::Result<(), String> {
    if categories.len() > MAX_CATEGORIES {
        return Err("Too many category settings".to_string());
    }
    let mut seen = BTreeSet::new();
    let mut deduped = Vec::with_capacity(categories.len());
    for mut range in categories.drain(..) {
        if range.excluded_metrics.len() > MAX_EXCLUDED_METRICS {
            return Err("Too many excluded metrics".to_string());
        }
        let mut seen_metrics = BTreeSet::new();
        range.excluded_metrics = range
            .excluded_metrics
            .iter()
            .map(|metric| metric.trim().to_string())
            .filter(|metric| {
                !metric.is_empty()
                    && metric.chars().count() <= MAX_ID_CHARS
                    && seen_metrics.insert(metric.clone())
            })
            .collect();
        if range.picked_days.len() > MAX_PICKED_DAYS {
            return Err("Too many picked dates".to_string());
        }
        let mut picked = BTreeSet::new();
        for day in &range.picked_days {
            let parsed = NaiveDate::parse_from_str(day.trim(), "%Y-%m-%d")
                .map_err(|_| format!("Invalid picked date: {day}"))?;
            picked.insert(parsed.to_string());
        }
        range.picked_days = picked.into_iter().collect();
        if !(0..=MAX_DAYS_BEFORE).contains(&range.days_before) {
            return Err(format!(
                "The lookback days must be between 0 and {MAX_DAYS_BEFORE} ({:?} = {})",
                range.category, range.days_before
            ));
        }
        if seen.insert(range.category) {
            deduped.push(range);
        }
    }
    *categories = deduped;
    Ok(())
}

impl Database {
    /// P3 `ai_task_list`: uses the index columns. SQLite JSON functions read the enabled categories from payload,
    /// so Rust does not deserialize the whole object.
    pub fn list_ai_tasks(&self) -> Result<Vec<AiTaskSummary>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, title, template_id, workout_count, mcp_shared, updated_at, pinned,
                    (SELECT group_concat(json_extract(value, '$.category'))
                     FROM json_each(ai_tasks.payload, '$.categories')
                     WHERE json_extract(value, '$.enabled'))
             FROM ai_tasks ORDER BY pinned DESC, updated_at DESC, id",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(AiTaskSummary {
                id: row.get(0)?,
                title: row.get(1)?,
                template_id: row.get(2)?,
                workout_count: row.get(3)?,
                mcp_shared: row.get::<_, i64>(4)? != 0,
                updated_at: row.get(5)?,
                pinned: row.get::<_, i64>(6)? != 0,
                categories: row
                    .get::<_, Option<String>>(7)?
                    .map(|list| list.split(',').map(str::to_string).collect())
                    .unwrap_or_default(),
            })
        })?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    /// Full version of the `ai_task_get` command: not found -> `err.ai_task.not_found`.
    /// It lives in core because `AiTaskError::task_not_found` is `pub(crate)`,
    /// and the command layer cannot reach it. The code and params have their single definition here.
    pub fn require_ai_task(&self, id: &str) -> Result<AiTask> {
        self.get_ai_task(id)?
            .ok_or_else(|| AiTaskError::task_not_found(id))
    }

    pub fn get_ai_task(&self, id: &str) -> Result<Option<AiTask>> {
        let payload: Option<String> = self
            .conn
            .query_row("SELECT payload FROM ai_tasks WHERE id = ?1", [id], |row| {
                row.get(0)
            })
            .optional()?;
        payload
            .map(|payload| {
                serde_json::from_str::<AiTask>(&payload)
                    .map(|mut task| {
                        upgrade_heart_rate_split(&mut task.categories);
                        task
                    })
                    .map_err(|error| {
                        ZeppBridgeError::ParseError(format!(
                            "ai_tasks[{id}] payload is corrupt: {error}"
                        ))
                    })
            })
            .transpose()
    }

    /// P3 `ai_task_save`: upsert. The backend fills id and timestamps. The `mcp_shared` switch
    /// is also in this one command (the final P3 spec has no separate set_mcp_shared).
    pub fn save_ai_task(&self, task: &AiTask) -> Result<AiTask> {
        let mut task = normalize_task(task)?;
        let now = Utc::now().to_rfc3339();
        // Auto dedup (batch 5.3): if a new task matches an existing one (same template, same time range, same question), update that one.
        // Saved tasks never hold two identical entries.
        if task.id.is_empty() {
            if let Some(existing) = self.same_ai_task(&task)? {
                task.id = existing;
            }
        }
        if task.id.is_empty() {
            task.id = self.new_ai_task_id()?;
            task.created_at = now.clone();
        } else {
            let existing = self.get_ai_task(&task.id)?;
            if let Some(existing) = &existing {
                self.remember_prompt_version(&task.id, &existing.prompt, &task.prompt)?;
            }
            task.created_at = existing
                .map(|existing| existing.created_at)
                .filter(|created| !created.is_empty())
                .unwrap_or_else(|| now.clone());
        }
        task.updated_at = now;

        let payload = serde_json::to_string(&task)
            .map_err(|error| ZeppBridgeError::ParseError(format!("ai_task serialization failed: {error}")))?;
        self.conn.execute(
            "INSERT INTO ai_tasks
                (id, title, template_id, payload, workout_count, mcp_shared, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(id) DO UPDATE SET
                title = excluded.title,
                template_id = excluded.template_id,
                payload = excluded.payload,
                workout_count = excluded.workout_count,
                mcp_shared = excluded.mcp_shared,
                updated_at = excluded.updated_at",
            params![
                task.id,
                task.title,
                task.template_id,
                payload,
                task.workout_ids.len() as i64,
                task.mcp_shared as i64,
                task.created_at,
                task.updated_at,
            ],
        )?;
        Ok(task)
    }

    /// P3 `ai_task_delete`: deletes only the task row. Attachment originals stay.
    /// "The same task": same template, same enabled categories with their lookback days, same question (trimmed).
    fn same_ai_task(&self, task: &AiTask) -> Result<Option<String>> {
        let key = |task: &AiTask| {
            // Picked days (the inbox) count too: the same question with different picked days is a different task.
            let mut ranges: Vec<(String, i64, Vec<String>)> = task
                .categories
                .iter()
                .filter(|range| range.enabled)
                .map(|range| {
                    (
                        format!("{:?}", range.category),
                        range.days_before,
                        range.picked_days.clone(),
                    )
                })
                .collect();
            ranges.sort();
            (
                task.template_id.clone(),
                ranges,
                task.prompt.trim().to_string(),
            )
        };
        let wanted = key(task);
        let mut stmt = self
            .conn
            .prepare("SELECT id, payload FROM ai_tasks ORDER BY updated_at DESC")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        for row in rows {
            let (id, payload) = row?;
            if let Ok(existing) = serde_json::from_str::<AiTask>(&payload) {
                if key(&existing) == wanted {
                    return Ok(Some(id));
                }
            }
        }
        Ok(None)
    }

    /// Pin / unpin. Does not touch `updated_at`: pinning is not "use".
    pub fn set_ai_task_pinned(&self, id: &str, pinned: bool) -> Result<()> {
        let changed = self.conn.execute(
            "UPDATE ai_tasks SET pinned = ?2 WHERE id = ?1",
            params![id, pinned as i64],
        )?;
        if changed == 0 {
            return Err(AiTaskError::task_not_found(id));
        }
        Ok(())
    }

    pub fn delete_ai_task(&self, id: &str) -> Result<()> {
        let removed = self
            .conn
            .execute("DELETE FROM ai_tasks WHERE id = ?1", [id])?;
        if removed == 0 {
            return Err(AiTaskError::task_not_found(id));
        }
        Ok(())
    }

    /// Batch delete (round 4 1D D7 "long-press to pick, then delete N"): deletes all in one transaction and returns the deleted originals and their pin state.
    /// The UI "undo" stores them back as they were. Ids already gone are skipped and do not fail the batch. Each id counts once.
    pub fn delete_ai_tasks(&self, ids: &[String]) -> Result<Vec<DeletedAiTask>> {
        let transaction = self.conn.unchecked_transaction()?;
        let mut seen = BTreeSet::new();
        let mut deleted = Vec::new();
        for id in ids {
            if !seen.insert(id.as_str()) {
                continue;
            }
            let row: Option<(String, i64)> = transaction
                .query_row(
                    "SELECT payload, pinned FROM ai_tasks WHERE id = ?1",
                    [id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()?;
            let Some((payload, pinned)) = row else {
                continue;
            };
            let mut task = serde_json::from_str::<AiTask>(&payload).map_err(|error| {
                ZeppBridgeError::ParseError(format!("ai_tasks[{id}] payload is corrupt: {error}"))
            })?;
            upgrade_heart_rate_split(&mut task.categories);
            transaction.execute("DELETE FROM ai_tasks WHERE id = ?1", [id])?;
            deleted.push(DeletedAiTask {
                task,
                pinned: pinned != 0,
            });
        }
        transaction.commit()?;
        Ok(deleted)
    }

    /// P3 `ai_template_list`: built-in templates first.
    pub fn list_ai_task_templates(&self) -> Result<Vec<AiTaskTemplate>> {
        let mut stmt = self.conn.prepare(
            "SELECT payload FROM ai_task_templates
             ORDER BY builtin DESC, updated_at DESC, id",
        )?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        let mut templates = Vec::new();
        for row in rows {
            let payload = row?;
            let mut template =
                serde_json::from_str::<AiTaskTemplate>(&payload).map_err(|error| {
                    ZeppBridgeError::ParseError(format!(
                        "ai_task_templates payload is corrupt: {error}"
                    ))
                })?;
            upgrade_heart_rate_split(&mut template.categories);
            templates.push(template);
        }
        Ok(templates)
    }

    pub fn get_ai_task_template(&self, id: &str) -> Result<Option<AiTaskTemplate>> {
        let payload: Option<String> = self
            .conn
            .query_row(
                "SELECT payload FROM ai_task_templates WHERE id = ?1",
                [id],
                |row| row.get(0),
            )
            .optional()?;
        payload
            .map(|payload| {
                serde_json::from_str::<AiTaskTemplate>(&payload)
                    .map(|mut template| {
                        upgrade_heart_rate_split(&mut template.categories);
                        template
                    })
                    .map_err(|error| {
                        ZeppBridgeError::ParseError(format!(
                            "ai_task_templates[{id}] payload is corrupt: {error}"
                        ))
                    })
            })
            .transpose()
    }

    /// P3 `ai_template_save`: built-in templates are read-only. A new id with `builtin=true` is always refused.
    /// The built-in flag comes only from migration seeds. Users can only "save as" a copy with builtin=false.
    pub fn save_ai_task_template(&self, template: &AiTaskTemplate) -> Result<AiTaskTemplate> {
        if let Some(existing) = self.get_ai_task_template(&template.id)? {
            if existing.builtin {
                return Err(AiTaskError::builtin_readonly());
            }
        }
        let mut template = normalize_template(template)?;
        if template.builtin {
            return Err(AiTaskError::invalid_template(
                "The builtin flag belongs only to built-in templates. Save the template as a normal user template.",
            ));
        }
        // name_code / prompt_code are localization handles of built-in seeds. User templates have no matching entries,
        // so storing them would point to codes that do not exist. Strip them so the UI renders the raw text.
        template.name_code = None;
        template.prompt_code = None;

        let now = Utc::now().to_rfc3339();
        if template.id.is_empty() {
            template.id = self.new_ai_template_id()?;
            template.created_at = now.clone();
        } else {
            template.created_at = self
                .get_ai_task_template(&template.id)?
                .map(|existing| existing.created_at)
                .filter(|created| !created.is_empty())
                .unwrap_or_else(|| now.clone());
        }
        template.updated_at = now;

        let payload = serde_json::to_string(&template).map_err(|error| {
            ZeppBridgeError::ParseError(format!("ai_task_template serialization failed: {error}"))
        })?;
        self.conn.execute(
            "INSERT INTO ai_task_templates (id, builtin, name, payload, created_at, updated_at)
             VALUES (?1, 0, ?2, ?3, ?4, ?5)
             ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                payload = excluded.payload,
                updated_at = excluded.updated_at",
            params![
                template.id,
                template.name,
                payload,
                template.created_at,
                template.updated_at,
            ],
        )?;
        Ok(template)
    }

    /// P3 `ai_template_delete`: built-in template -> `err.ai_template.builtin_readonly`;
    /// not found -> `err.ai_template.not_found`.
    pub fn delete_ai_task_template(&self, id: &str) -> Result<()> {
        match self.get_ai_task_template(id)? {
            Some(template) if template.builtin => {
                return Err(AiTaskError::builtin_readonly());
            }
            Some(_) => {}
            None => return Err(AiTaskError::template_not_found(id)),
        }
        self.conn
            .execute("DELETE FROM ai_task_templates WHERE id = ?1", [id])?;
        Ok(())
    }

    /// Allocate a `task-*` id that does not collide.
    fn new_ai_task_id(&self) -> Result<String> {
        for _ in 0..8 {
            let id = format!("task-{}", random_hex(8));
            if self.get_ai_task(&id)?.is_none() {
                return Ok(id);
            }
        }
        Err(ZeppBridgeError::Busy("Could not allocate a unique task id".into()))
    }

    fn new_ai_template_id(&self) -> Result<String> {
        for _ in 0..8 {
            let id = format!("tmpl-{}", random_hex(8));
            if self.get_ai_task_template(&id)?.is_none() {
                return Ok(id);
            }
        }
        Err(ZeppBridgeError::Busy("Could not allocate a unique template id".into()))
    }
}

/// P3 `ai_task_attachment_stat`: reads only file metadata, not content.
///
/// This is a **local** check. Returning `path` is valid (the UI checks the file the user gave).
/// It is not the same as the export's `AiTaskAttachmentPublicRef`.
pub fn stat_attachment_paths(paths: &[String]) -> Vec<AiTaskAttachmentStat> {
    paths
        .iter()
        .map(|path| match std::fs::metadata(Path::new(path)) {
            Ok(meta) => AiTaskAttachmentStat {
                path: path.clone(),
                exists: true,
                byte_len: Some(meta.len() as i64),
                mtime: meta
                    .modified()
                    .ok()
                    .map(|time| chrono::DateTime::<Utc>::from(time).to_rfc3339()),
            },
            Err(_) => AiTaskAttachmentStat {
                path: path.clone(),
                exists: false,
                byte_len: None,
                mtime: None,
            },
        })
        .collect()
}
