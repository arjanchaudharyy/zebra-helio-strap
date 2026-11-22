//! Coach: health questions answered by Sarvam's chat API.
//!
//! The API key lives in `<data dir>/sarvam.key` (mode 0600), or in the
//! `SARVAM_API_KEY` environment variable. It never goes to the webview: the
//! page sends messages here, and only the reply text goes back.
//! Nothing is sent unless the person asks a question or asks for a briefing.

use crate::ipc_error::AppError;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;

const KEY_FILE: &str = "sarvam.key";
const ENDPOINT: &str = "https://api.sarvam.ai/v2/chat/completions";
/// DeepSeek V4.1 Flash on Sarvam's interactive deployment: answers in about a second.
const MODEL: &str = "deepseekv4.1-flash-1017-interactive";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoachMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CoachReply {
    pub content: String,
    pub model: String,
}

fn key_path(data_dir: &Path) -> PathBuf {
    data_dir.join(KEY_FILE)
}

fn read_key(data_dir: &Path) -> Option<String> {
    if let Ok(value) = std::env::var("SARVAM_API_KEY") {
        let value = value.trim().to_string();
        if !value.is_empty() {
            return Some(value);
        }
    }
    std::fs::read_to_string(key_path(data_dir))
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[tauri::command]
pub fn ai_coach_key_status(state: tauri::State<'_, crate::app_state::AppState>) -> bool {
    read_key(&state.data_dir).is_some()
}

#[tauri::command]
pub fn ai_coach_set_key(
    state: tauri::State<'_, crate::app_state::AppState>,
    key: String,
) -> Result<bool, AppError> {
    let key = key.trim();
    let path = key_path(&state.data_dir);
    if key.is_empty() {
        let _ = std::fs::remove_file(&path);
        return Ok(false);
    }
    std::fs::write(&path, key)
        .map_err(|e| AppError::new("err.coach.key_write", format!("Could not save the key: {e}")))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(true)
}

#[tauri::command]
pub async fn ai_coach_ask(
    state: tauri::State<'_, crate::app_state::AppState>,
    messages: Vec<CoachMessage>,
    effort: Option<String>,
) -> Result<CoachReply, AppError> {
    let key = read_key(&state.data_dir).ok_or_else(|| {
        AppError::new("err.coach.no_key", "Add your Sarvam API key in Settings › AI and MCP.")
    })?;
    // Reasoning off by default: short coaching answers do not need it and it costs seconds.
    let effort = match effort.as_deref() {
        Some("low") => "low",
        Some("high") => "high",
        _ => "none",
    };
    let body = serde_json::json!({
        "model": MODEL,
        "messages": messages,
        "reasoning_effort": effort,
        "max_tokens": 1200,
    });
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(300))
        .build()
        .map_err(|e| AppError::new("err.coach.client", e.to_string()))?;
    let response = client
        .post(ENDPOINT)
        .header("api-subscription-key", key)
        .json(&body)
        .send()
        .await
        .map_err(|e| AppError::new("err.coach.network", format!("Could not reach Sarvam: {e}")))?;
    let status = response.status();
    let value: serde_json::Value = response
        .json()
        .await
        .map_err(|e| AppError::new("err.coach.parse", format!("Unreadable reply from Sarvam: {e}")))?;
    if !status.is_success() {
        let detail = value
            .pointer("/error/message")
            .and_then(|v| v.as_str())
            .unwrap_or("request failed");
        return Err(AppError::new(
            "err.coach.http",
            format!("Sarvam returned {}: {detail}", status.as_u16()),
        ));
    }
    let content = value
        .pointer("/choices/0/message/content")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .replace(" \u{2014} ", ", ")
        .replace('\u{2014}', ", ")
        .replace(" \u{2013} ", ", ");
    if content.is_empty() {
        return Err(AppError::new(
            "err.coach.empty",
            "Sarvam returned no answer. Try again, or ask a shorter question.",
        ));
    }
    Ok(CoachReply {
        content,
        model: MODEL.to_string(),
    })
}
