//! Coach: health questions answered by the AI provider the person picks in Settings.
//!
//! Providers: Sarvam, OpenAI, Anthropic, OpenRouter, Google Gemini, Groq, Ollama, LM Studio,
//! or any OpenAI-compatible server. The choice lives in `<data dir>/ai.json`; API keys live in
//! `<data dir>/ai-keys.json` (mode 0600). Keys never go to the webview: the page sends messages
//! here, and only the reply text goes back. Nothing is sent unless the person asks.

use crate::ipc_error::AppError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

const LEGACY_KEY_FILE: &str = "sarvam.key";
const CONFIG_FILE: &str = "ai.json";
const KEYS_FILE: &str = "ai-keys.json";

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

/// One provider: where to send, how to authenticate, and a default model.
struct Provider {
    id: &'static str,
    url: &'static str,
    model: &'static str,
    needs_key: bool,
}

const PROVIDERS: &[Provider] = &[
    Provider { id: "sarvam", url: "https://api.sarvam.ai/v2/chat/completions", model: "deepseekv4.1-flash-1017-interactive", needs_key: true },
    Provider { id: "openai", url: "https://api.openai.com/v1/chat/completions", model: "gpt-4o-mini", needs_key: true },
    Provider { id: "anthropic", url: "https://api.anthropic.com/v1/messages", model: "claude-haiku-5-5", needs_key: true },
    Provider { id: "openrouter", url: "https://openrouter.ai/api/v1/chat/completions", model: "openrouter/auto", needs_key: true },
    Provider { id: "gemini", url: "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions", model: "gemini-2.5-flash", needs_key: true },
    Provider { id: "groq", url: "https://api.groq.com/openai/v1/chat/completions", model: "llama-3.3-70b-versatile", needs_key: true },
    Provider { id: "ollama", url: "http://localhost:11434/v1/chat/completions", model: "llama3.2", needs_key: false },
    Provider { id: "lmstudio", url: "http://localhost:1234/v1/chat/completions", model: "local-model", needs_key: false },
    Provider { id: "custom", url: "", model: "", needs_key: false },
];

fn provider(id: &str) -> &'static Provider {
    PROVIDERS.iter().find(|p| p.id == id).unwrap_or(&PROVIDERS[0])
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiConfig {
    #[serde(default)]
    pub provider: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub base_url: String,
}

fn read_config(dir: &Path) -> AiConfig {
    let mut c: AiConfig = std::fs::read_to_string(dir.join(CONFIG_FILE))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    if c.provider.is_empty() {
        c.provider = "sarvam".into();
    }
    c
}

fn read_keys(dir: &Path) -> BTreeMap<String, String> {
    let mut keys: BTreeMap<String, String> = std::fs::read_to_string(dir.join(KEYS_FILE))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    if !keys.contains_key("sarvam") {
        let legacy = std::env::var("SARVAM_API_KEY").ok().filter(|v| !v.trim().is_empty())
            .or_else(|| std::fs::read_to_string(dir.join(LEGACY_KEY_FILE)).ok());
        if let Some(k) = legacy.map(|k| k.trim().to_string()).filter(|k| !k.is_empty()) {
            keys.insert("sarvam".into(), k);
        }
    }
    keys
}

fn write_private(path: &PathBuf, text: &str) -> Result<(), AppError> {
    std::fs::write(path, text).map_err(|e| AppError::new("err.coach.write", format!("Could not save: {e}")))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiStatus {
    pub provider: String,
    pub model: String,
    pub base_url: String,
    /// Providers that have a saved key (never the key itself).
    pub keys: Vec<String>,
    pub ready: bool,
}

fn status_of(dir: &Path) -> AiStatus {
    let c = read_config(dir);
    let keys = read_keys(dir);
    let p = provider(&c.provider);
    let ready = (!p.needs_key || keys.contains_key(p.id)) && (p.id != "custom" || !c.base_url.is_empty());
    AiStatus { provider: p.id.into(), model: c.model, base_url: c.base_url, keys: keys.keys().cloned().collect(), ready }
}

#[tauri::command]
pub fn ai_coach_status(state: tauri::State<'_, crate::app_state::AppState>) -> AiStatus {
    status_of(&state.data_dir)
}

/// Save the provider choice, and optionally a key for it (empty string removes the key).
#[tauri::command]
pub fn ai_coach_configure(
    state: tauri::State<'_, crate::app_state::AppState>,
    provider_id: String,
    model: String,
    base_url: String,
    key: Option<String>,
) -> Result<AiStatus, AppError> {
    let p = provider(&provider_id);
    let config = AiConfig { provider: p.id.into(), model: model.trim().into(), base_url: base_url.trim().into() };
    write_private(&state.data_dir.join(CONFIG_FILE), &serde_json::to_string_pretty(&config).unwrap_or_default())?;
    if let Some(key) = key {
        let mut keys = read_keys(&state.data_dir);
        let key = key.trim().to_string();
        if key.is_empty() { keys.remove(p.id); } else { keys.insert(p.id.into(), key); }
        write_private(&state.data_dir.join(KEYS_FILE), &serde_json::to_string(&keys).unwrap_or_default())?;
    }
    Ok(status_of(&state.data_dir))
}

#[tauri::command]
pub fn ai_coach_key_status(state: tauri::State<'_, crate::app_state::AppState>) -> bool {
    status_of(&state.data_dir).ready
}

#[tauri::command]
pub fn ai_coach_set_key(
    state: tauri::State<'_, crate::app_state::AppState>,
    key: String,
) -> Result<bool, AppError> {
    let c = read_config(&state.data_dir);
    let st = ai_coach_configure(state, c.provider, c.model, c.base_url, Some(key))?;
    Ok(st.ready)
}

#[tauri::command]
pub async fn ai_coach_ask(
    state: tauri::State<'_, crate::app_state::AppState>,
    messages: Vec<CoachMessage>,
    effort: Option<String>,
) -> Result<CoachReply, AppError> {
    let dir = &state.data_dir;
    let c = read_config(dir);
    let p = provider(&c.provider);
    let keys = read_keys(dir);
    let key = keys.get(p.id).cloned();
    if p.needs_key && key.is_none() {
        return Err(AppError::new("err.coach.no_key", "Add an API key in Settings, AI and MCP."));
    }
    let model = if c.model.is_empty() { p.model.to_string() } else { c.model.clone() };
    let url = if p.id == "custom" || !c.base_url.is_empty() {
        let base = c.base_url.trim_end_matches('/');
        if base.is_empty() {
            return Err(AppError::new("err.coach.no_url", "Add the server address in Settings, AI and MCP."));
        }
        if base.ends_with("/chat/completions") || base.ends_with("/messages") { base.to_string() } else { format!("{base}/chat/completions") }
    } else {
        p.url.to_string()
    };
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(300))
        .build()
        .map_err(|e| AppError::new("err.coach.client", e.to_string()))?;
    let anthropic = p.id == "anthropic";
    let body = if anthropic {
        let system: Vec<&str> = messages.iter().filter(|m| m.role == "system").map(|m| m.content.as_str()).collect();
        let rest: Vec<&CoachMessage> = messages.iter().filter(|m| m.role != "system").collect();
        serde_json::json!({ "model": model, "system": system.join("\n\n"), "messages": rest, "max_tokens": 1200 })
    } else if p.id == "sarvam" {
        // Reasoning off by default: short coaching answers do not need it and it costs seconds.
        let effort = match effort.as_deref() { Some("low") => "low", Some("high") => "high", _ => "none" };
        serde_json::json!({ "model": model, "messages": messages, "reasoning_effort": effort, "max_tokens": 1200 })
    } else {
        serde_json::json!({ "model": model, "messages": messages, "max_tokens": 1200 })
    };
    let mut req = client.post(&url).json(&body);
    if let Some(k) = key.as_deref() {
        req = match p.id {
            "sarvam" => req.header("api-subscription-key", k),
            "anthropic" => req.header("x-api-key", k).header("anthropic-version", "2023-06-01"),
            _ => req.bearer_auth(k),
        };
    }
    if p.id == "openrouter" {
        req = req.header("X-Title", "Zebra");
    }
    let response = req
        .send()
        .await
        .map_err(|e| AppError::new("err.coach.network", format!("Could not reach the AI provider: {e}")))?;
    let status = response.status();
    let value: serde_json::Value = response
        .json()
        .await
        .map_err(|e| AppError::new("err.coach.parse", format!("Unreadable reply from the AI provider: {e}")))?;
    if !status.is_success() {
        let detail = value
            .pointer("/error/message")
            .and_then(|v| v.as_str())
            .unwrap_or("request failed");
        return Err(AppError::new("err.coach.http", format!("The AI provider returned {}: {detail}", status.as_u16())));
    }
    let text_ptr = if anthropic { "/content/0/text" } else { "/choices/0/message/content" };
    let content = value
        .pointer(text_ptr)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .replace(" \u{2014} ", ", ")
        .replace('\u{2014}', ", ")
        .replace(" \u{2013} ", ", ");
    if content.is_empty() {
        return Err(AppError::new(
            "err.coach.empty",
            "The AI returned no answer. Try again, or ask a shorter question.",
        ));
    }
    Ok(CoachReply {
        content,
        model,
    })
}
