//! Redaction before data leaves this machine. An export for AI loses auth fields,
//! identity fields and precise routes. Local paths in free text become a placeholder.
//!
//! Before, two copies lived in the Tauri command layer (`commands/data`) and in
//! `ai_tasks`. The first parsed the export **string** again, which doubled peak memory
//! for large exports. The second was a hand copy of the first. Now both call this
//! module, which edits the JSON tree directly.

use serde_json::{Map, Value};
use std::collections::BTreeSet;

/// Replaces local paths in free text (prompts, notes, export strings) with a placeholder:
/// Windows drive paths, UNC paths and Unix absolute paths. A path only starts at a word
/// boundary, so URLs such as `https://...` are not changed.
pub fn sanitize_local_paths(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    // The last character written to output. Before, each step called
    // `output.chars().last()`, which scanned the whole output: quadratic on long text.
    let mut previous: Option<char> = None;
    while let Some(character) = chars.next() {
        let at_boundary = previous.is_none_or(|value| {
            value.is_whitespace() || matches!(value, '"' | '\'' | '(' | '[' | '{' | '=')
        });
        let is_windows_drive = at_boundary
            && character.is_ascii_alphabetic()
            && chars.peek() == Some(&':')
            && chars
                .clone()
                .nth(1)
                .is_some_and(|next| next == '\\' || next == '/');
        let is_unc = at_boundary && character == '\\' && chars.peek() == Some(&'\\');
        let is_unix =
            at_boundary && character == '/' && chars.peek().is_some_and(|next| *next != ' ');
        if is_windows_drive || is_unc || is_unix {
            output.push_str("[local path removed]");
            previous = Some(']');
            if is_windows_drive {
                let _ = chars.next();
            }
            while let Some(next) = chars.peek() {
                if next.is_whitespace() || *next == '"' || *next == '\'' || *next == ')' {
                    break;
                }
                let _ = chars.next();
            }
        } else {
            output.push(character);
            previous = Some(character);
        }
    }
    output
}

/// Redacts an export for AI in place and returns the redaction classes applied (policy
/// names with no user values, safe for the UI and metadata). Precise routes are removed
/// by default and kept only when `include_precise_route` is true.
pub fn redact_ai_export(value: &mut Value, include_precise_route: bool) -> Vec<String> {
    let mut redactions = BTreeSet::from(["authentication_fields", "identity_fields"]);
    if !include_precise_route {
        redactions.insert("precise_route");
    }
    redact_value(value, include_precise_route, &mut redactions);
    let redactions: Vec<String> = redactions.into_iter().map(str::to_owned).collect();

    if let Some(root) = value.as_object_mut() {
        root.insert(
            "redactions".to_string(),
            Value::Array(redactions.iter().cloned().map(Value::String).collect()),
        );
        root.insert(
            "metadata".to_string(),
            serde_json::json!({
                "ai_handoff": true,
                "precise_route_included": include_precise_route,
                "authentication_fields_removed": true,
                "identity_fields_removed": true,
            }),
        );
    }
    redactions
}

fn redact_value(
    value: &mut Value,
    include_precise_route: bool,
    redactions: &mut BTreeSet<&'static str>,
) {
    match value {
        Value::Object(object) => redact_object(object, include_precise_route, redactions),
        Value::Array(items) => {
            for item in items {
                redact_value(item, include_precise_route, redactions);
            }
        }
        Value::String(text) => {
            let sanitized = sanitize_local_paths(text);
            if sanitized != *text {
                *text = sanitized;
                redactions.insert("local_paths");
            }
        }
        _ => {}
    }
}

fn redact_object(
    object: &mut Map<String, Value>,
    include_precise_route: bool,
    redactions: &mut BTreeSet<&'static str>,
) {
    object.retain(|key, _| match removal_reason(key, include_precise_route) {
        Some(reason) => {
            redactions.insert(reason);
            false
        }
        None => true,
    });
    for child in object.values_mut() {
        redact_value(child, include_precise_route, redactions);
    }
}

/// Whether to remove this key, and if so, its class. Auth wins over identity, identity wins over route.
fn removal_reason(key: &str, include_precise_route: bool) -> Option<&'static str> {
    let normalized = normalize_json_key(key);
    if is_authentication_key(&normalized) {
        Some("authentication_fields")
    } else if is_identity_key(&normalized) {
        Some("identity_fields")
    } else if !include_precise_route && is_precise_route_key(&normalized) {
        Some("precise_route")
    } else if is_local_path_key(&normalized) {
        Some("local_paths")
    } else {
        None
    }
}

fn normalize_json_key(key: &str) -> String {
    key.chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(|character| character.to_lowercase())
        .collect()
}

fn is_authentication_key(key: &str) -> bool {
    key.contains("token")
        || key.contains("auth")
        || key.contains("credential")
        || key.contains("secret")
        || key.contains("password")
        || key == "cookie"
        || key == "cookies"
        || key == "apikey"
        || key == "authorization"
}

fn is_identity_key(key: &str) -> bool {
    matches!(
        key,
        "user"
            | "username"
            | "userid"
            | "useraccount"
            | "account"
            | "accountid"
            | "accountname"
            | "serial"
            | "serialnumber"
            | "device"
            | "deviceid"
            | "record"
            | "workoutid"
            | "workout"
            | "sleepid"
            | "sleep"
            | "sessionid"
            | "recordid"
            | "sampleid"
            | "metricid"
            | "dailyid"
            | "sourceid"
            | "rawid"
            | "id"
            | "useridentifier"
            | "accountidentifier"
            | "deviceidentifier"
            | "serialidentifier"
            | "recordidentifier"
            | "workoutidentifier"
            | "sleepidentifier"
            | "sessionidentifier"
            | "useremail"
            | "accountemail"
            | "userphone"
            | "accountphone"
            | "email"
            | "phone"
            | "phonenumber"
            | "uuid"
            | "guid"
            | "identifier"
    ) || (key.ends_with("id")
        && [
            "user", "account", "device", "record", "workout", "sleep", "session", "sample",
            "metric", "daily", "source", "raw",
        ]
        .iter()
        .any(|prefix| key.starts_with(prefix)))
        || ([
            "user", "account", "device", "record", "workout", "sleep", "session", "serial",
        ]
        .iter()
        .any(|prefix| key.starts_with(prefix))
            && ["identifier", "uuid", "guid", "key", "number"]
                .iter()
                .any(|suffix| key.ends_with(suffix)))
        || key.contains("serial")
}

fn is_precise_route_key(key: &str) -> bool {
    let coordinate_digits = |prefix: &str| {
        key.starts_with(prefix)
            && key
                .chars()
                .skip(prefix.len())
                .all(|character| character.is_ascii_digit() || character == 'e')
    };
    matches!(
        key,
        "route"
            | "routepoints"
            | "routepoint"
            | "gps"
            | "location"
            | "locations"
            | "geolocation"
            | "geo"
            | "track"
            | "trackpoints"
            | "latitude"
            | "longitude"
            | "lat"
            | "lng"
            | "lon"
            | "coordinates"
            | "polyline"
    ) || key.contains("latitude")
        || key.contains("longitude")
        || key.contains("coordinate")
        || coordinate_digits("lat")
        || coordinate_digits("lng")
        || coordinate_digits("lon")
        || key.contains("routepoint")
        || key.contains("trackpoint")
        || key == "latlng"
        || (key.starts_with("gps")
            && (key.contains("route")
                || key.contains("point")
                || key.contains("coord")
                || key.contains("track")))
}

fn is_local_path_key(key: &str) -> bool {
    matches!(
        key,
        "path"
            | "filepath"
            | "filename"
            | "file"
            | "sourcepath"
            | "databasepath"
            | "localpath"
            | "exportpath"
            | "directory"
            | "dirname"
    ) || key.ends_with("filepath")
        || key.ends_with("pathname")
}

/// Whether a word looks like a credential, a serial number or a device ID.
///
/// The check uses only the shape: long hex strings, long digit strings, addresses with
/// @, and MAC shapes are replaced. It is better to hide a model number than to leak a
/// token. Diagnostic report notes and letter replies share this (`letters::sanitize_reply`).
pub fn looks_like_secret(token: &str) -> bool {
    let value = token.trim_matches(|character: char| {
        !character.is_alphanumeric() && character != '@' && character != ':' && character != '-'
    });
    if value.chars().count() < 8 {
        return false;
    }
    if value.contains('@') && value.contains('.') {
        return true;
    }
    // A MAC address in aa:bb:cc:dd:ee:ff shape
    let colon_groups: Vec<&str> = value.split(':').collect();
    if colon_groups.len() >= 6
        && colon_groups
            .iter()
            .all(|group| group.len() == 2 && group.chars().all(|c| c.is_ascii_hexdigit()))
    {
        return true;
    }
    let alnum = value.chars().filter(|c| c.is_ascii_alphanumeric()).count();
    let digits = value.chars().filter(|c| c.is_ascii_digit()).count();
    if alnum >= 16
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return true;
    }
    digits >= 10
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_paths_are_removed_without_touching_urls() {
        let text = r#"see C:\Users\me\zepp.db and "\\nas\share\x" or /home/me/a.json, keep https://example.com/a/b"#;
        let cleaned = sanitize_local_paths(text);
        assert!(!cleaned.contains("Users"));
        assert!(!cleaned.contains("nas"));
        assert!(!cleaned.contains("/home"));
        assert!(cleaned.contains("https://example.com/a/b"));
        assert_eq!(cleaned.matches("[local path removed]").count(), 3);
    }

    #[test]
    fn a_long_note_is_cleaned_in_linear_time() {
        // This was quadratic before: one MB of text took tens of seconds.
        let text = "word ".repeat(200_000);
        let started = std::time::Instant::now();
        assert_eq!(sanitize_local_paths(&text), text);
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
    }
}
