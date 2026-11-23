use super::coverage::{
    category_metric_specs, category_units, category_windows, parse_rfc3339_utc,
    window_coverage_rows, AnchorWorkout,
};

use super::model::*;

use super::store::normalize_task_draft;

use super::AiTaskError;

use crate::models::error::Result;

use crate::models::{SleepStageSlice, Workout};

use crate::paths::write_file_atomically;

use crate::storage::{loaded_stage_minutes, Database, MetricSource};

use chrono::{DateTime, Local, NaiveDate, Utc};

use rusqlite::{params, params_from_iter};

use serde_json::{json, Map, Value};

use std::collections::{BTreeMap, BTreeSet};

use std::path::{Path, PathBuf};

mod bundle;
mod gather;
mod helpers;
mod journal;
pub mod markdown;
mod route_gpx;
mod workout_extras;

pub use route_gpx::route_file_name;

pub(crate) use helpers::*;

/// Build result shared by preview and prepare. The numbers on both sides must come from the same build.
pub(crate) struct AiTaskBundle {
    pub document: Value,
    pub briefs: Vec<AiTaskWorkoutBrief>,
    pub coverage: Vec<AiTaskCoverage>,
    pub attachments: Vec<AiTaskAttachmentStatus>,
    pub warnings: Vec<AiTaskIssue>,
}

/// One loading pass for one anchor window: the days/sources for coverage and the day rows for the document
/// come from the same query (H4: before, coverage and `gather_category_days`
/// each queried the same window).
///
/// Workout/sleep categories accumulate days/sources from the rows directly. Metric category output rows
/// have no `source_scope` (`daily_metric_points`/`sample_metric_points` do not
/// return it), so their days/sources still use the dedicated `category_window_days` query.
/// Metric categories therefore keep the same query count as the old version, no worse.
pub(crate) struct WindowGather {
    /// `None` = the task has no linked workout, and the window is anchored on today.
    pub workout_id: Option<String>,
    /// Picked days (inbox): keep only these days. `None` = continuous window.
    pub picked: Option<BTreeSet<String>>,
    pub start: NaiveDate,
    pub end: NaiveDate,
    /// Set of "local days with data" in the window and the set of matched `source_scope`.
    /// Deduplication granularity is the P4 `(category, date)`.
    pub covered_days: BTreeSet<String>,
    pub sources: BTreeSet<String>,
    /// Local days with data per metric (per field for workout/sleep), including excluded metrics.
    pub metric_days: BTreeMap<String, BTreeSet<String>>,
    /// Document rows: `(local day, dedup key, output object)`. The dedup key is
    /// `w:<workout_id>` / `s:<sleep_id>` / metric name, used when merging across windows.
    pub rows: Vec<(String, String, Value)>,
}

/// Prompt and file name parts that the frontend prepares in the UI language. The backend does not produce copy. It only joins and cleans.
/// All are optional: old callers such as the CLI pass nothing, and behavior stays as before.
#[derive(Debug, Default, Clone, Copy)]
pub struct AiTaskPromptParts<'a> {
    /// "Task instructions" part: what the data is, how to read the files, what to do by default with no question, start the analysis directly. Goes first.
    pub brief: Option<&'a str>,
    /// "Analysis direction" part (template). When not passed, falls back to the template's own prompt_template.
    pub direction: Option<&'a str>,
    /// Full text the user edited in the final prompt: replaces the instructions + direction + question parts. Coverage notes are still appended after it.
    pub override_text: Option<&'a str>,
    /// Data file base name (no extension). Empty = old name `health-context`.
    pub data_stem: Option<&'a str>,
    /// Prompt file base name (no extension). Empty = old name `prompt`.
    pub prompt_stem: Option<&'a str>,
    /// When given, deliver one `.md` (prompt + reading notes + data) instead of two files (JSON + txt).
    pub markdown: Option<AiTaskMarkdownParts<'a>>,
}

/// Parameters for single `.md` delivery: the frontend gives the reading notes in the UI language. The budget follows the chosen AI and "I am subscribed".
#[derive(Debug, Clone, Copy)]
pub struct AiTaskMarkdownParts<'a> {
    pub guide: Option<&'a str>,
    pub token_budget: usize,
}

fn markdown_estimate(
    render: &markdown::MarkdownRender,
    token_budget: usize,
) -> AiTaskMarkdownEstimate {
    AiTaskMarkdownEstimate {
        approx_tokens: render.approx_tokens as i64,
        byte_len: render.text.len() as i64,
        curve_average_seconds: render.curve_average_seconds,
        summarized_workouts: render.summarized_workouts.clone(),
        over_budget: render.over_budget,
        token_budget: token_budget as i64,
    }
}

/// Output of `ai_task_prepare_plan`: all reads are done, only writing the files is left.
/// The command layer first runs the build on a read-only connection, then runs `finish()` on a blocking thread.
/// Writing the files does not hold a database connection and needs no cross-process write lock.
pub struct AiTaskPreparePlan {
    task_id: String,
    output_dir: PathBuf,
    /// File names of the data file and the prompt file (cleaned, with extension).
    json_name: String,
    prompt_name: String,
    prompt_text: String,
    attachments: Vec<AiTaskAttachmentStatus>,
    blocked: Vec<AiTaskIssue>,
    /// Always present when ready. Empty when blocked.
    json_text: Option<String>,
    /// Originals to copy into `attachments/`: `(local source path, display name)`.
    attachment_sources: Vec<(PathBuf, String)>,
    /// Single `.md` delivery: `(file name, render result, budget)`. When present, no JSON + txt is written.
    markdown: Option<(String, markdown::MarkdownRender, usize)>,
    /// Full track next to the `.md` when "Exact route" is on: `(file name, GPX)` (see route_gpx.rs).
    route_gpx: Option<(String, String)>,
}

impl AiTaskPreparePlan {
    /// Write side: pure file IO, no database. When blocked, nothing is written (not even the directory).
    pub fn finish(self) -> Result<AiTaskPrepareResult> {
        let output_dir_text = self.output_dir.to_string_lossy().into_owned();
        if !self.blocked.is_empty() {
            return Ok(AiTaskPrepareResult {
                status: AiTaskPrepareStatus::Blocked,
                task_id: self.task_id,
                output_dir: output_dir_text,
                json_path: None,
                prompt_path: None,
                prompt_text: self.prompt_text,
                byte_len: 0,
                copied_attachments: 0,
                md_path: None,
                markdown: None,
                attachments: self.attachments,
                blocked: self.blocked,
            });
        }
        if let Some((md_name, render, budget)) = self.markdown {
            std::fs::create_dir_all(&self.output_dir).map_err(|error| {
                AiTaskError::write_failed(format!(
                    "failed to create handoff directory {}: {error}",
                    self.output_dir.display()
                ))
            })?;
            let md_path = self.output_dir.join(&md_name);
            write_file_atomically(&md_path, render.text.as_bytes())
                .map_err(|error| AiTaskError::write_failed(format!("failed to write handoff file: {error}")))?;
            if let Some((gpx_name, gpx)) = &self.route_gpx {
                write_file_atomically(&self.output_dir.join(gpx_name), gpx.as_bytes()).map_err(
                    |error| AiTaskError::write_failed(format!("failed to write full track: {error}")),
                )?;
            }
            let copied_attachments = copy_attachments(&self.output_dir, &self.attachment_sources)?;
            return Ok(AiTaskPrepareResult {
                status: AiTaskPrepareStatus::Ready,
                task_id: self.task_id,
                output_dir: output_dir_text,
                json_path: None,
                prompt_path: None,
                prompt_text: self.prompt_text,
                byte_len: render.text.len() as i64,
                copied_attachments,
                md_path: Some(md_path.to_string_lossy().into_owned()),
                markdown: Some(markdown_estimate(&render, budget)),
                attachments: self.attachments,
                blocked: Vec::new(),
            });
        }
        // Invariant: blocked is empty => the plan always has the serialized document.
        let json_text = self
            .json_text
            .ok_or_else(|| AiTaskError::write_failed("missing handoff JSON content"))?;
        let byte_len = json_text.len() as i64;

        std::fs::create_dir_all(&self.output_dir).map_err(|error| {
            AiTaskError::write_failed(format!(
                "failed to create handoff directory {}: {error}",
                self.output_dir.display()
            ))
        })?;
        let json_path = self.output_dir.join(&self.json_name);
        let prompt_path = self.output_dir.join(&self.prompt_name);
        write_file_atomically(&json_path, json_text.as_bytes())
            .map_err(|error| AiTaskError::write_failed(format!("failed to write handoff JSON: {error}")))?;
        write_file_atomically(&prompt_path, self.prompt_text.as_bytes())
            .map_err(|error| AiTaskError::write_failed(format!("failed to write prompt file: {error}")))?;
        let copied_attachments = copy_attachments(&self.output_dir, &self.attachment_sources)?;

        Ok(AiTaskPrepareResult {
            status: AiTaskPrepareStatus::Ready,
            task_id: self.task_id,
            output_dir: output_dir_text,
            json_path: Some(json_path.to_string_lossy().into_owned()),
            prompt_path: Some(prompt_path.to_string_lossy().into_owned()),
            prompt_text: self.prompt_text,
            byte_len,
            copied_attachments,
            md_path: None,
            markdown: None,
            attachments: self.attachments,
            blocked: Vec::new(),
        })
    }
}

/// Copies the original attachments into `<output_dir>/attachments/`, so the user can drag them all at once.
/// The file name is the display name (Windows-forbidden characters cleaned). Duplicates get ` (2)` appended.
fn copy_attachments(output_dir: &Path, sources: &[(PathBuf, String)]) -> Result<i64> {
    if sources.is_empty() {
        return Ok(0);
    }
    let dir = output_dir.join("attachments");
    std::fs::create_dir_all(&dir)
        .map_err(|error| AiTaskError::write_failed(format!("failed to create attachment directory: {error}")))?;
    let mut used = BTreeSet::new();
    for (source, display_name) in sources {
        let name = unique_file_name(&sanitize_file_name(display_name, "attachment"), &mut used);
        std::fs::copy(source, dir.join(&name)).map_err(|error| {
            AiTaskError::write_failed(format!("failed to copy attachment {display_name}: {error}"))
        })?;
    }
    Ok(sources.len() as i64)
}

/// Final file names of the data file and the prompt file: the frontend base name is cleaned by Windows rules.
/// When not given, use the old name. When the two cleaned base names collide, the prompt one gets a suffix, so neither overwrites the other.
fn export_file_names(parts: &AiTaskPromptParts<'_>) -> (String, String) {
    let stem = |value: Option<&str>, fallback: &str| {
        value
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(|text| sanitize_file_name(text, fallback))
            .unwrap_or_else(|| fallback.to_string())
    };
    let data = stem(parts.data_stem, "health-context");
    let mut prompt = stem(parts.prompt_stem, "prompt");
    if prompt.to_lowercase() == data.to_lowercase() {
        prompt = format!("{data}_prompt");
    }
    (format!("{data}.json"), format!("{prompt}.txt"))
}

/// File name of the single `.md`: uses the data file base name (given by the frontend by naming rules, already cleaned).
fn markdown_file_name(json_name: &str) -> String {
    format!(
        "{}.md",
        json_name.strip_suffix(".json").unwrap_or(json_name)
    )
}

fn unique_file_name(name: &str, used: &mut BTreeSet<String>) -> String {
    let lower = |value: &str| value.to_lowercase();
    if used.insert(lower(name)) {
        return name.to_string();
    }
    let (stem, ext) = match name.rfind('.') {
        Some(index) if index > 0 => (&name[..index], &name[index..]),
        _ => (name, ""),
    };
    (2..)
        .map(|n| format!("{stem} ({n}){ext}"))
        .find(|candidate| used.insert(lower(candidate)))
        .expect("a unique file name always exists")
}

/// Display id and directory name when an unsaved draft goes through preview/prepare.
const DRAFT_TASK_ID: &str = "draft";

impl Database {
    /// P3 `ai_task_preview`: drafts can be previewed. Validation is loose (empty title/id is valid),
    /// but a missing workout in `workout_ids` is still a hard error `err.ai_task.workout_not_found`.
    pub fn ai_task_preview(&self, task: &AiTask) -> Result<AiTaskPreview> {
        self.ai_task_preview_with_budget(task, None)
    }

    /// Same as [`Self::ai_task_preview`], and also estimates the size and degradation of a single `.md` by `token_budget`
    /// (prompt not included, so the UI says "about").
    pub fn ai_task_preview_with_budget(
        &self,
        task: &AiTask,
        token_budget: Option<usize>,
    ) -> Result<AiTaskPreview> {
        let task = normalize_task_draft(task)?;
        let anchors = self.ai_task_anchors(&task.workout_ids)?;
        let attachments = stat_task_attachments(&task.attachments);
        let bundle = self.build_ai_task_bundle(&task, &anchors, attachments)?;
        // The estimate must use the same serialization (pretty) as the real write in `ai_task_prepare`.
        // Otherwise the byte count in the UI preview does not match the written file.
        let estimated_bytes = serde_json::to_string_pretty(&bundle.document)?.len() as i64;
        let markdown = token_budget.map(|budget| {
            markdown_estimate(
                &markdown::render_task_markdown(&bundle.document, "", budget),
                budget,
            )
        });
        Ok(AiTaskPreview {
            markdown,
            task_id: effective_task_id(&task),
            workouts: bundle.briefs,
            coverage: bundle.coverage,
            attachments: bundle.attachments,
            estimated_bytes,
            warnings: bundle.warnings,
        })
    }

    /// P3 `ai_task_prepare`: writes the handoff files to
    /// `<output_root>/<task name>_<yyyyMMdd-HHmm>/{health-context.json, prompt.txt, attachments/}`.
    ///
    /// The command layer decides `output_root` (desktop `ZeppBridge AI`, or when not available
    /// `data_dir/exports/ai-tasks`). When blocked, no file is written.
    ///
    /// Split into [`Self::ai_task_prepare_plan`] (read side) + [`AiTaskPreparePlan::finish`]
    /// (write side): the command layer holds a database connection only on the read side, and file writes hold no lock.
    /// Callers that need one call for everything (tests) keep using this thin wrapper.
    pub fn ai_task_prepare(
        &self,
        task: &AiTask,
        coverage_note: &str,
        output_root: &Path,
    ) -> Result<AiTaskPrepareResult> {
        self.ai_task_prepare_plan(
            task,
            coverage_note,
            &AiTaskPromptParts::default(),
            output_root,
        )?
        .finish()
    }

    /// Read side of `ai_task_prepare`: validation, anchor resolution, attachment check, prompt assembly,
    /// blocked check and bundle build (including JSON serialization) all happen here.
    /// The returned [`AiTaskPreparePlan`] only writes files and does not touch the database.
    ///
    /// `parts`: task instructions / direction / edited full text and file names that the frontend prepares in the UI language,
    /// the same approach as `coverage_note`. The backend does not produce copy. It only joins and cleans.
    pub fn ai_task_prepare_plan(
        &self,
        task: &AiTask,
        coverage_note: &str,
        parts: &AiTaskPromptParts<'_>,
        output_root: &Path,
    ) -> Result<AiTaskPreparePlan> {
        let task = normalize_task_draft(task)?;
        let anchors = self.ai_task_anchors(&task.workout_ids)?;
        let attachments = stat_task_attachments(&task.attachments);
        let prompt_text = self.assemble_task_prompt(&task, coverage_note, parts)?;
        let (json_name, prompt_name) = export_file_names(parts);

        // No linked workout is no longer blocked: the window is anchored on today, so the analysis covers "the last N days".
        let mut blocked: Vec<AiTaskIssue> = Vec::new();
        for (attachment, status) in task.attachments.iter().zip(attachments.iter()) {
            if status.status == AiTaskAttachmentState::Missing {
                // Code named in the P2 final spec: a `missing` attachment is listed in blocked as
                // `err.ai_task.attachment_missing` (other blocked items use ui.*).
                blocked.push(
                    AiTaskIssue::new(
                        "err.ai_task.attachment_missing",
                        format!("Attachment file is no longer at its original location: {}", attachment.display_name),
                    )
                    .with_params(serde_json::json!({ "display_name": attachment.display_name })),
                );
            }
        }
        // When nothing can be selected (no window category, no attachment, no text), the delivery
        // is only an empty shell. This blocked case is not missing data. There is nothing to give.
        let nothing_to_export = !task.categories.iter().any(|range| range.enabled)
            && task.attachments.is_empty()
            && task.personal_note.trim().is_empty();
        if nothing_to_export {
            blocked.push(AiTaskIssue::new(
                "ui.ai_task.blocked.empty",
                "The current selection covers no data. Adjust the categories or workout range first",
            ));
        }

        let task_id = effective_task_id(&task);
        let output_dir = output_root.join(export_folder_name(&task, &task_id, Local::now()));

        if !blocked.is_empty() {
            return Ok(AiTaskPreparePlan {
                task_id,
                output_dir,
                json_name,
                prompt_name,
                prompt_text,
                attachments,
                blocked,
                json_text: None,
                attachment_sources: Vec::new(),
                markdown: None,
                route_gpx: None,
            });
        }

        let bundle = self.build_ai_task_bundle(&task, &anchors, attachments)?;
        let mut markdown = parts.markdown.map(|md| {
            let guide = md.guide.map(str::trim).unwrap_or_default();
            let header = [prompt_text.trim(), guide]
                .into_iter()
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
                .join("\n\n");
            (
                markdown_file_name(&json_name),
                markdown::render_task_markdown(&bundle.document, &header, md.token_budget),
                md.token_budget,
            )
        });
        // Exact route + detailed: the track in the `.md` is sampled, so a full GPX goes next to it.
        let mut route_file = None;
        if let Some((md_name, render, _)) = markdown.as_mut() {
            if task.include_precise_gps && task.detail_level == AiTaskDetailLevel::Detailed {
                if let Some((gpx, points)) = route_gpx::route_gpx(&bundle.document) {
                    let name = route_gpx::route_file_name(md_name);
                    render.text.push_str(&route_gpx::route_note(&name, points));
                    route_file = Some((name, gpx));
                }
            }
        }
        let json_text = serde_json::to_string_pretty(&bundle.document)
            .map_err(|error| AiTaskError::write_failed(format!("failed to serialize handoff data: {error}")))?;
        let attachment_sources = task
            .attachments
            .iter()
            .map(|reference| {
                (
                    PathBuf::from(&reference.path),
                    reference.display_name.clone(),
                )
            })
            .collect();

        Ok(AiTaskPreparePlan {
            task_id,
            output_dir,
            json_name,
            prompt_name,
            prompt_text,
            attachments: bundle.attachments,
            blocked: Vec::new(),
            json_text: Some(json_text),
            attachment_sources,
            markdown,
            route_gpx: route_file,
        })
    }
}

#[cfg(test)]
mod tests;
