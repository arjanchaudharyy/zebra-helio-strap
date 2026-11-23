//! Diagnostic reports (split from models/types.rs by domain; shapes unchanged).

use super::*;

/// One allowlisted field description. Only the key and JSON kind are carried;
/// the value is structurally impossible to serialize into a diagnostic report.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticField {
    pub name: String,
    pub json_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticObjectShape {
    pub path: String,
    pub fields: Vec<DiagnosticField>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticDeviceCandidate {
    pub catalog_id: String,
    pub canonical_name: String,
    pub firmware: Option<String>,
    pub match_status: DeviceMatchStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticDeviceEvidence {
    pub status: String,
    pub object_count: usize,
    pub unknown_device_count: usize,
    pub id_alias_objects: usize,
    pub serial_alias_objects: usize,
    pub name_field_objects: usize,
    pub firmware_field_objects: usize,
    pub candidates: Vec<DiagnosticDeviceCandidate>,
    pub unmatched_product_hints: Vec<String>,
    /// Model numbers of devices the catalog (with the official Zepp table) does not know, such as `deviceSource:7930112`.
    ///
    /// Some accounts have no product name field in the device response, so this number is
    /// the only model clue. It names a model, not a single watch: no serial number, MAC,
    /// bind time or any per-device value. `deviceType` (the family code) is no longer
    /// collected, and neither are numbers of known devices and body scales.
    #[serde(default)]
    pub model_identifier_hints: Vec<String>,
    pub shapes: Vec<DiagnosticObjectShape>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticWorkoutCode {
    pub code: i32,
    pub records: i64,
}

/// The "not successful" that the cloud writes inside an HTTP 200.
///
/// It has three fields and none is free text: which stream, which code, and when.
/// The raw cloud text (`message`) is not collected on purpose. It is server free text,
/// and this report promises the user to send only allowlisted fields.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticCloudRejection {
    /// Which stream was rejected (`workouts` / `sleep` / ...).
    pub stream: String,
    /// The `code` in the payload. Success is 1; no other value has been seen yet.
    pub code: i64,
    /// RFC3339. The first of the three stages that has a value.
    pub at: Option<String>,
}

/// What the user corrected a Zepp workout number to.
///
/// This is the only possible evidence for problems like issue #24. The reporter said
/// "trail running was read as open water swimming", but `unknown_workout_codes` was
/// empty and `workout_type_conflicts` was 0, because we **knew** the number but read it
/// wrong. In the old fields, a wrong match and an unknown number look the same.
///
/// All three fields are type-level facts: the cloud number, our reading and the user's
/// reading. **No instance data**: no workout_id, time, distance or GPS.
/// `corrected` can only be a key from the bundled sport catalog (see
/// `set_workout_type_override`), not free text.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticWorkoutCorrection {
    /// The raw workout number from the cloud.
    pub code: i32,
    /// The sport key ZeppBridge chose (for example `open_water_swimming`).
    pub interpreted: String,
    /// The sport key the user chose (for example `trail_running`).
    pub corrected: String,
    /// How many records the user changed this way. One record and twenty records weigh differently.
    pub records: i64,
}

/// Strongly typed, allowlist-only issue report. It has no slots for account
/// identifiers, tokens, serial values, GPS, health measurements, raw payloads,
/// or filesystem paths.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticReport {
    pub format: String,
    pub app_version: String,
    pub schema_version: i64,
    pub normalizer_revision: String,
    pub operating_system: String,
    pub device_evidence: DiagnosticDeviceEvidence,
    pub unknown_workout_codes: Vec<DiagnosticWorkoutCode>,
    /// The user's sport type corrections, grouped by "number -> our reading -> user's reading".
    ///
    /// With no corrections, the field is absent, so the report does not grow.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub workout_type_corrections: Vec<DiagnosticWorkoutCorrection>,
    pub workout_type_conflicts: i64,
    /// The problem type the user chose (`device` / `workout` / `data` / `other`).
    ///
    /// Local auto-detection only finds problems like "unknown device or workout number". The
    /// user can have other problems, such as wrong data or an always-empty field. Without
    /// this field, those users cannot report, because the server finds "nothing to process".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    /// A note the user wrote ("my watch is a Balance 2, but it is not recognized").
    ///
    /// With only field structure and numbers, the reader often cannot tell the watch model.
    /// One human sentence is often more useful than ten fields. It is free text, so it is
    /// redacted and length-limited before it is sent: the user can paste a token or a
    /// local path. When empty, the field is absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_note: Option<String>,
    /// The most recent cloud business rejection. When there is none, the field is absent.
    ///
    /// This closes the loop for `classify_business_code`. It already detects "HTTP 200 but the
    /// cloud says not successful", but it does not decide whether that means "sign in again",
    /// because no failure code has been seen. A report with a real code lets us map it to
    /// `NeedsReauth` exactly.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_cloud_rejection: Option<DiagnosticCloudRejection>,
}

/// The limit for the free-text note. It is enough for "device is a Balance 2, firmware
/// 3.5.1, workout type shows as unknown", but too short to paste a whole log.
pub const DIAGNOSTIC_NOTE_MAX_CHARS: usize = 500;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FeedbackSubmissionResult {
    pub report_id: String,
    pub submitted_at: DateTime<Utc>,
}
