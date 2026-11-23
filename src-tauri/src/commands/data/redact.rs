//! Redaction before data goes to AI. The rules live in core at `zeppbridge_core::redact` (shared with ai_tasks).
//! This file only encodes the redacted JSON as handoff text and maps errors to codes the UI knows.

use super::*;

pub(crate) use zeppbridge_core::redact::sanitize_local_paths as sanitize_clipboard_text;

/// Redacts an export in place (as a JSON tree, with no serialize and re-parse). Returns the encoded text and the redaction categories.
pub(crate) fn redact_ai_export(
    mut value: Value,
    include_precise_route: bool,
) -> std::result::Result<(String, Vec<String>), AppError> {
    let redactions = zeppbridge_core::redact::redact_ai_export(&mut value, include_precise_route);
    let encoded = serde_json::to_string_pretty(&value).map_err(|error| {
        AppError::new(
            "err.handoff.encode_failed",
            format!("Could not encode the redacted AI export: {error}"),
        )
    })?;
    Ok((encoded, redactions))
}
