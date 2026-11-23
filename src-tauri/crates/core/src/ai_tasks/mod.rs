//! AI analysis tasks: storage model, window coverage, export bundle building and MCP grant windows.
//!
//! Rust side of the BETA1 A7 protocol (P1-P4, P6). Boundaries:
//!
//! - This module produces **facts and shapes** only: task/template CRUD, windows expanded per P4,
//!   clean export JSON. The frontend supplies prompt copy by code (the `coverage_note` parameter).
//!   The backend produces no UI copy.
//! - [`model::AiTaskAttachmentRef`] (with a local `path`) stays on this machine.
//!   Attachments that leave this machine always use [`model::AiTaskAttachmentPublicRef`].
//! - [`shared_task_grants`] computes an `access::TaskGrant` for each `mcp_shared=1` task
//!   on every request. Expansion and checks live in `access.rs` (the S5 file).
//!   This module only keeps the `ai_tasks` table shape in line with its `SharedTaskPayload` parsing.

pub mod coverage;
pub mod export;
pub mod library;
pub mod model;
pub mod store;

#[cfg(test)]
mod tests;

pub use crate::access::shared_task_grants;
pub use model::*;
pub use store::stat_attachment_paths;

use crate::models::error::ZeppBridgeError;

/// Business failures of the task/template layer.
///
/// Why a separate enum and not `ZeppBridgeError::ConfigError`:
/// the command layer returns IPC errors by code (`err.ai_task.invalid` and `err.ai_template.invalid`
/// are two codes, and both can come from "field validation failed"). The code is fixed here, and
/// `AppError::from` passes it through, so the command layer does not guess the entity again.
#[derive(Debug, thiserror::Error)]
pub enum AiTaskError {
    /// Input validation failed. `code` is `err.ai_task.invalid` or `err.ai_template.invalid`,
    /// set by the entity that produced it.
    #[error("{message}")]
    Invalid { code: &'static str, message: String },
    /// Record not found: `err.ai_task.not_found` / `err.ai_template.not_found` /
    /// `err.ai_task.workout_not_found`. `params` holds only public, safe ids.
    #[error("{message}")]
    NotFound {
        code: &'static str,
        message: String,
        params: Option<serde_json::Value>,
    },
    /// Built-in templates cannot be edited or deleted. "Save as user template" is the only valid path.
    #[error("Built-in templates are read-only. Save it as a user template.")]
    BuiltinReadonly,
    /// Writing the handoff file to disk failed.
    #[error("{message}")]
    WriteFailed { message: String },
    // Note: do not put `Storage(#[from] ZeppBridgeError)` here.
    // `ZeppBridgeError::AiTask(AiTaskError)` and it would nest into an infinitely sized type.
    // Lower-level errors use `ZeppBridgeError`'s own #[from] (rusqlite/serde_json/io).
    // In functions that return `ZeppBridgeError`, pass them through with `?`.
}

impl AiTaskError {
    /// Stable code. Both `ZeppBridgeError::AiTask` and `AppError::from` use it.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Invalid { code, .. } | Self::NotFound { code, .. } => code,
            Self::BuiltinReadonly => "err.ai_template.builtin_readonly",
            Self::WriteFailed { .. } => "err.ai_task.write_failed",
        }
    }

    /// Fallback message. The UI gets localized text by `code()`.
    pub fn user_message(&self) -> String {
        match self {
            Self::Invalid { message, .. }
            | Self::NotFound { message, .. }
            | Self::WriteFailed { message } => crate::models::error::sanitize_user_text(message),
            Self::BuiltinReadonly => self.to_string(),
        }
    }

    /// Values to fill into the message (missing id lists and so on). Only public, safe values.
    pub fn params(&self) -> Option<serde_json::Value> {
        match self {
            Self::NotFound { params, .. } => params.clone(),
            _ => None,
        }
    }

    pub(crate) fn invalid_task(message: impl Into<String>) -> ZeppBridgeError {
        ZeppBridgeError::AiTask(AiTaskError::Invalid {
            code: "err.ai_task.invalid",
            message: message.into(),
        })
    }

    pub(crate) fn invalid_template(message: impl Into<String>) -> ZeppBridgeError {
        ZeppBridgeError::AiTask(AiTaskError::Invalid {
            code: "err.ai_template.invalid",
            message: message.into(),
        })
    }

    pub(crate) fn task_not_found(id: &str) -> ZeppBridgeError {
        ZeppBridgeError::AiTask(AiTaskError::NotFound {
            code: "err.ai_task.not_found",
            message: "The analysis task does not exist or was deleted".to_string(),
            params: Some(serde_json::json!({ "id": id })),
        })
    }

    pub(crate) fn template_not_found(id: &str) -> ZeppBridgeError {
        ZeppBridgeError::AiTask(AiTaskError::NotFound {
            code: "err.ai_template.not_found",
            message: "The template does not exist or was deleted".to_string(),
            params: Some(serde_json::json!({ "id": id })),
        })
    }

    pub(crate) fn workouts_not_found(ids: Vec<String>) -> ZeppBridgeError {
        ZeppBridgeError::AiTask(AiTaskError::NotFound {
            code: "err.ai_task.workout_not_found",
            message: format!("The selected workouts do not exist on this machine: {}", ids.join(", ")),
            params: Some(serde_json::json!({ "workout_ids": ids })),
        })
    }

    pub(crate) fn builtin_readonly() -> ZeppBridgeError {
        ZeppBridgeError::AiTask(AiTaskError::BuiltinReadonly)
    }

    pub(crate) fn write_failed(message: impl Into<String>) -> ZeppBridgeError {
        ZeppBridgeError::AiTask(AiTaskError::WriteFailed {
            message: message.into(),
        })
    }
}
