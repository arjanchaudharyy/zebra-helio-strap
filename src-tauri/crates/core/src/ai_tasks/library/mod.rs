//! Prompt library (batch 7): task prompt version history, filter by category, `.zbprompt` export / import.
//!
//! - Versions: when a save changes the prompt, the **old prompt** goes into `ai_prompt_versions` (v45).
//!   Deleting a task keeps its history: undo stores the task back as it was, so the history must stay.
//!   Each task keeps only the latest [`MAX_VERSIONS_PER_TASK`] entries.
//! - `.zbprompt`: a shareable question. It has the title, prompt, categories with their lookback days, and detail level.
//!   It carries **nothing** that belongs to this computer or this person: linked workouts, picked days, attachments,
//!   personal notes, the precise GPS switch, MCP grants. Imports go to "Saved tasks". The user picks workouts again.

use super::model::*;
use super::AiTaskError;
use crate::models::error::{Result, ZeppBridgeError};
use crate::storage::Database;
use chrono::Utc;
use rusqlite::params;
use serde::{Deserialize, Serialize};

#[cfg(test)]
mod tests;

pub const MAX_VERSIONS_PER_TASK: i64 = 30;
pub const ZBPROMPT_FORMAT: &str = "zbprompt";
pub const ZBPROMPT_VERSION: i64 = 1;
/// Maximum size of a `.zbprompt` file in bytes. The prompt limit is 16 000 characters, so this leaves ample room.
pub const ZBPROMPT_MAX_BYTES: u64 = 256 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AiPromptVersion {
    pub id: i64,
    pub prompt: String,
    /// When this prompt was replaced. That is also the last time it was in use.
    pub replaced_at: String,
}

/// Shape of the `.zbprompt` file. The field names are the public contract. Renaming one requires a new `version`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ZbPrompt {
    pub format: String,
    pub version: i64,
    pub title: String,
    pub prompt: String,
    /// Only built-in template ids go out (other libraries do not have your user templates).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template_id: Option<String>,
    #[serde(default)]
    pub detail_level: AiTaskDetailLevel,
    #[serde(default)]
    pub categories: Vec<AiTaskCategoryRange>,
}

fn bad_file(message: &str) -> ZeppBridgeError {
    ZeppBridgeError::AiTask(AiTaskError::Invalid {
        code: "err.ai_task.bad_prompt_file",
        message: message.into(),
    })
}

/// The part of this task that can be shared.
pub fn to_zbprompt(task: &AiTask, builtin_template: bool) -> ZbPrompt {
    ZbPrompt {
        format: ZBPROMPT_FORMAT.into(),
        version: ZBPROMPT_VERSION,
        title: task.title.clone(),
        prompt: task.prompt.clone(),
        template_id: task.template_id.clone().filter(|_| builtin_template),
        detail_level: task.detail_level,
        categories: portable_categories(task.categories.clone()),
    }
}

/// Personal notes and attachments belong to this person, and so do picked days. None of them go out.
fn portable_categories(categories: Vec<AiTaskCategoryRange>) -> Vec<AiTaskCategoryRange> {
    categories
        .into_iter()
        .filter(|range| {
            !matches!(
                range.category,
                AiTaskCategory::PersonalNote | AiTaskCategory::Attachment
            )
        })
        .map(|range| AiTaskCategoryRange {
            picked_days: Vec::new(),
            ..range
        })
        .collect()
}

/// Read a `.zbprompt`. Reject it unless the format is right, the version is known and the prompt is not empty.
pub fn parse_zbprompt(text: &str) -> Result<ZbPrompt> {
    let prompt: ZbPrompt = serde_json::from_str(text.trim_start_matches('\u{feff}'))
        .map_err(|_| bad_file("Not a .zbprompt file"))?;
    if prompt.format != ZBPROMPT_FORMAT {
        return Err(bad_file("Not a .zbprompt file"));
    }
    if prompt.version > ZBPROMPT_VERSION {
        return Err(bad_file("This .zbprompt comes from a newer version"));
    }
    if prompt.prompt.trim().is_empty() {
        return Err(bad_file("This .zbprompt has no prompt"));
    }
    Ok(prompt)
}

impl Database {
    /// Call before a task save: if the prompt changed, record the old one and keep only the latest few.
    pub(crate) fn remember_prompt_version(
        &self,
        task_id: &str,
        before: &str,
        after: &str,
    ) -> Result<()> {
        if before.trim().is_empty() || before.trim() == after.trim() {
            return Ok(());
        }
        self.conn.execute(
            "INSERT INTO ai_prompt_versions(task_id, prompt, replaced_at) VALUES(?1, ?2, ?3)",
            params![task_id, before, Utc::now().to_rfc3339()],
        )?;
        self.conn.execute(
            "DELETE FROM ai_prompt_versions WHERE task_id = ?1 AND id NOT IN (
                SELECT id FROM ai_prompt_versions WHERE task_id = ?1 ORDER BY id DESC LIMIT ?2)",
            params![task_id, MAX_VERSIONS_PER_TASK],
        )?;
        Ok(())
    }

    /// Prompts this task used before, newest first.
    pub fn ai_prompt_versions(&self, task_id: &str) -> Result<Vec<AiPromptVersion>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, prompt, replaced_at FROM ai_prompt_versions
             WHERE task_id = ?1 ORDER BY id DESC",
        )?;
        let rows = stmt.query_map([task_id], |row| {
            Ok(AiPromptVersion {
                id: row.get(0)?,
                prompt: row.get(1)?,
                replaced_at: row.get(2)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// Export a task as `.zbprompt` JSON text.
    pub fn export_zbprompt(&self, task_id: &str) -> Result<String> {
        let task = self.require_ai_task(task_id)?;
        let builtin = match task.template_id.as_deref() {
            Some(id) => self
                .get_ai_task_template(id)?
                .is_some_and(|template| template.builtin),
            None => false,
        };
        Ok(serde_json::to_string_pretty(&to_zbprompt(&task, builtin))?)
    }

    /// Import a `.zbprompt`: save it as a new task with no linked workouts (update the existing task if the same question exists).
    pub fn import_zbprompt(&self, text: &str) -> Result<AiTask> {
        let prompt = parse_zbprompt(text)?;
        let template_id = match prompt.template_id {
            Some(id) => self
                .get_ai_task_template(&id)?
                .filter(|template| template.builtin)
                .map(|template| template.id),
            None => None,
        };
        let title = match prompt.title.trim() {
            "" => prompt.prompt.trim().chars().take(40).collect(),
            title => title.to_string(),
        };
        self.save_ai_task(&AiTask {
            schema_version: AI_TASK_SCHEMA_VERSION,
            id: String::new(),
            title,
            template_id,
            workout_ids: Vec::new(),
            categories: portable_categories(prompt.categories),
            detail_level: prompt.detail_level,
            prompt: prompt.prompt,
            personal_note: String::new(),
            attachments: Vec::new(),
            include_precise_gps: false,
            mcp_shared: false,
            created_at: String::new(),
            updated_at: String::new(),
        })
    }
}
