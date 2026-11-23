//! Errors sent to the UI.
//!
//! The previous version returned `Result<_, String>` from every command. The string was hard-coded text, and the frontend
//! `toUserMessage` passed it through unchanged. So every backend error showed in one language, whatever the UI language was.
//! This blocked real users.
//!
//! This file separates "what the error is" from "how to say it": the backend gives only a stable `code`
//! and optional params, and the UI picks the text in its own language by code. The message is still included as
//! a fallback when the UI has no translation yet, and for the CLI and logs.
//!
//! Codes are named `module.reason` (`auth.not_configured`, `export.empty_range`).
//! They are a public contract: renaming one breaks the existing translations. A new code needs its UI text in every locale.
//! `npm run i18n:check` catches a missing one.

use serde::Serialize;
use serde_json::Value;
use zeppbridge_core::models::error::ZeppBridgeError;

#[derive(Debug, Clone, Serialize)]
pub struct AppError {
    /// Stable error code. The UI uses it to pick the localized text.
    pub code: String,
    /// Fallback message. The UI shows it when it does not know the code. The CLI and logs always use it.
    pub message: String,
    /// Values to fill into the text (counts, months, HTTP status codes and so on).
    ///
    /// Only values that are safe to show: no tokens, cookies, full URLs, absolute local paths or health data.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl AppError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_string(),
            message: message.into(),
            params: None,
        }
    }

    pub fn with_params(mut self, params: Value) -> Self {
        self.params = Some(params);
        self
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

/// Lets `?` lift a core error into a UI error, so commands do not need `map_err` everywhere.
impl From<ZeppBridgeError> for AppError {
    fn from(error: ZeppBridgeError) -> Self {
        let mut app = AppError::new(error.code(), error.user_message());
        // The status code is a value to fill into the text. It is not part of the error identity.
        match &error {
            ZeppBridgeError::RetryExhausted { status, .. }
            | ZeppBridgeError::HttpStatus { status, .. } => {
                app.params = Some(serde_json::json!({ "status": status }));
            }
            // ai_task params (a missing workout id and so on) are judged safe to show by AiTaskError itself,
            // so they pass through unchanged.
            ZeppBridgeError::AiTask(inner) => {
                app.params = inner.params();
            }
            _ => {}
        }
        app
    }
}

/// The two write-lock failures mean different things to the user: "someone is writing" needs only a short wait, "the lock cannot be created"
/// needs a check of folder permissions. One shared message would leave the user guessing.
impl From<zeppbridge_core::storage::write_lock::WriteLockError> for AppError {
    fn from(error: zeppbridge_core::storage::write_lock::WriteLockError) -> Self {
        use zeppbridge_core::storage::write_lock::WriteLockError;
        match &error {
            WriteLockError::Busy { .. } => {
                AppError::new("err.storage.write_busy", error.to_string())
            }
            WriteLockError::Unavailable(_) => {
                AppError::new("err.storage.write_lock_unavailable", error.to_string())
            }
        }
    }
}

impl From<std::io::Error> for AppError {
    fn from(error: std::io::Error) -> Self {
        AppError::new("err.core.io", format!("Could not read or write a local file: {error}"))
    }
}
