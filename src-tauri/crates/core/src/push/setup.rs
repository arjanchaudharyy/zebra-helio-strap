//! Push target settings (for the desktop setting "Push to your own server").
//!
//! Reads and writes `push/targets.json`, stores and reads keys, parses the connection code the user pastes, reads the last round result,
//! and takes over the site and key from a locally installed ZeppBridge Cloud component (reads the component's data only, deletes nothing).
//! The rules are the same as in `mod.rs`: no keys in the config file; keys go only into the system credential store; errors carry only codes.

use super::{
    load_targets, pa, push_dir, push_error, secret_account, store_pa_secrets, PushError,
    TargetConfig, TargetKind, TARGETS_FILE,
};
use crate::auth::CredentialBackend;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;

/// The UI manages only one PA target, with a fixed id.
pub const PA_TARGET_ID: &str = "pa";

/// Writes the target config back (it runs the same checks as a read first; on failure it writes no byte).
pub fn save_targets(data_dir: &Path, targets: &[TargetConfig]) -> Result<(), PushError> {
    let dir = push_dir(data_dir);
    std::fs::create_dir_all(&dir)
        .map_err(|error| push_error("err.push.state_unwritable", error.to_string()))?;
    let body = serde_json::json!({ "version": 1, "targets": targets });
    if let Some(field) = super::find_secret_field(&body, "") {
        return Err(push_error("err.push.config_has_secret", field));
    }
    let text = serde_json::to_vec_pretty(&body)
        .map_err(|error| push_error("err.push.config_invalid", error.to_string()))?;
    let path = dir.join(TARGETS_FILE);
    crate::paths::write_file_atomically(&path, &text)
        .map_err(|error| push_error("err.push.state_unwritable", error.to_string()))?;
    // Read it again after the write: if it cannot be read back, the write is broken.
    load_targets(data_dir).map(|_| ())
}

/// Creates or updates the PA target: the URL is stored as the user entered it (normalized at push time); other options keep their values.
pub fn upsert_pa_target(data_dir: &Path, site: &str, enabled: bool) -> Result<(), PushError> {
    super::client::normalize_pa_site(site, false)
        .map_err(|code| push_error("err.push.site_invalid", code))?;
    let mut targets = load_targets(data_dir)?;
    match targets.iter_mut().find(|target| target.id == PA_TARGET_ID) {
        Some(target) => {
            target.site = site.trim().to_string();
            target.enabled = enabled;
        }
        None => targets.push(TargetConfig {
            id: PA_TARGET_ID.into(),
            kind: TargetKind::Pa,
            enabled,
            site: site.trim().to_string(),
            retention_days: 90,
            summary_days: 30,
            full_resolution_workouts: false,
            dev_loopback: false,
        }),
    }
    save_targets(data_dir, &targets)
}

/// Changes only the on/off switch.
pub fn set_enabled(data_dir: &Path, id: &str, enabled: bool) -> Result<(), PushError> {
    let mut targets = load_targets(data_dir)?;
    let target = targets
        .iter_mut()
        .find(|target| target.id == id)
        .ok_or_else(|| push_error("err.push.not_configured", ""))?;
    target.enabled = enabled;
    save_targets(data_dir, &targets)
}

/// Removes the target: config, key, manifest and last round result are deleted together. Nothing on the site changes.
pub fn remove_target(
    data_dir: &Path,
    credentials: &dyn CredentialBackend,
    id: &str,
) -> Result<(), PushError> {
    let targets: Vec<TargetConfig> = load_targets(data_dir)?
        .into_iter()
        .filter(|target| target.id != id)
        .collect();
    save_targets(data_dir, &targets)?;
    if credentials.supports_named_entries() {
        let _ = credentials.delete(&secret_account(id));
    }
    let dir = push_dir(data_dir);
    for suffix in ["manifest.json", "status.json"] {
        let _ = std::fs::remove_file(dir.join(format!("{id}.{suffix}")));
    }
    Ok(())
}

/// Tells whether the credential store has a usable key for this target (it only checks that it decodes; it returns no key).
pub fn has_secrets(credentials: &dyn CredentialBackend, id: &str) -> bool {
    credentials.supports_named_entries()
        && credentials
            .get(&secret_account(id))
            .ok()
            .flatten()
            .and_then(|text| serde_json::from_str::<pa::PaSecrets>(&text).ok())
            .is_some_and(|secrets| secrets.validate().is_ok())
}

/// Parses the connection code the user pastes: a JSON object with at least `dk` and `writeKey` (`tenant` is optional;
/// the full output line of `node keys.mjs init` also works; an extra `readKey` is ignored). Other text before or after is allowed
/// (a whole block copied from an AI answer).
pub fn parse_connection_code(text: &str) -> Result<pa::PaSecrets, PushError> {
    let invalid = || push_error("err.push.connection_code_invalid", "");
    let start = text.find('{').ok_or_else(invalid)?;
    let end = text.rfind('}').ok_or_else(invalid)?;
    if end < start {
        return Err(invalid());
    }
    let value: Value = serde_json::from_str(&text[start..=end]).map_err(|_| invalid())?;
    let field = |name: &str| value.get(name).and_then(Value::as_str).map(str::trim);
    let secrets = pa::PaSecrets {
        tenant: field("tenant")
            .filter(|tenant| !tenant.is_empty())
            .map(str::to_string),
        dk: field("dk").ok_or_else(invalid)?.to_string(),
        write_key: field("writeKey").ok_or_else(invalid)?.to_string(),
    };
    secrets
        .validate()
        .map_err(|code| push_error(code, "connection code"))?;
    Ok(secrets)
}

/// Stores the connection code.
pub fn save_connection_code(
    credentials: &dyn CredentialBackend,
    id: &str,
    text: &str,
) -> Result<(), PushError> {
    let secrets = parse_connection_code(text)?;
    store_pa_secrets(credentials, id, &secrets)
}

/// The last round result (`push/<id>.status.json`). None if it never ran.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LastPush {
    pub result: String,
    #[serde(default)]
    pub stats: LastPushStats,
    pub error_code: Option<String>,
    pub at: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LastPushStats {
    #[serde(default)]
    pub objects: usize,
    #[serde(default)]
    pub deleted: usize,
    #[serde(default)]
    pub unchanged: usize,
    #[serde(default)]
    pub bytes_sent: usize,
}

pub fn last_push(data_dir: &Path, id: &str) -> Option<LastPush> {
    let bytes = std::fs::read(push_dir(data_dir).join(format!("{id}.status.json"))).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// The upload key that the local ZeppBridge Cloud component stores in the credential manager (`ZeppBridgeCloud:<host>:upload`,
/// content `{tenant, dk, writeKey}`). This component exists only on Windows.
#[cfg(windows)]
pub fn companion_upload_secret(host: &str) -> Option<String> {
    let target = format!("ZeppBridgeCloud:{host}:upload");
    let entry = keyring::Entry::new_with_target(&target, "ZeppBridgeCloud", "").ok()?;
    let bytes = entry.get_secret().ok()?;
    String::from_utf8(bytes).ok()
}

#[cfg(not(windows))]
pub fn companion_upload_secret(_host: &str) -> Option<String> {
    None
}
