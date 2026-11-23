//! User preferences, storage estimates, sync status and the capability board (split from models/types.rs by domain; shapes unchanged).

use super::*;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UserPrefs {
    #[serde(default)]
    pub demo_mode: bool,
    #[serde(default)]
    pub ai_profile_note: String,
    /// How many recent days this machine keeps. Cleanup runs after each successful sync.
    pub retention_days: i64,
    /// How many days back one history backfill covers.
    ///
    /// **Independent** of `retention_days`: retention sets how long data stays here, backfill
    /// sets how far back to fetch. Before, both shared a 1-365 limit, so the UI could not
    /// express "get my records from three years ago".
    pub history_sync_days: i64,
    /// Long-term archive. When on, a successful sync does not clean up history, and
    /// `retention_days` is only kept as the value for when the archive is off.
    #[serde(default)]
    pub archive_enabled: bool,
}

impl UserPrefs {
    pub const DEFAULT_RETENTION_DAYS: i64 = 365;
    pub const DEFAULT_HISTORY_SYNC_DAYS: i64 = 180;
    /// The backfill limit: ten years. Zepp has no older records, and an input with no limit
    /// lets a user start a job that runs for days by mistake.
    pub const MAX_HISTORY_SYNC_DAYS: i64 = 3650;

    /// The allowed range for retention.
    pub fn clamp_days(value: i64) -> std::result::Result<i64, String> {
        if (1..=365).contains(&value) {
            Ok(value)
        } else {
            Err("Retention days must be between 1 and 365".into())
        }
    }

    /// The allowed range for history backfill.
    pub fn clamp_history_days(value: i64) -> std::result::Result<i64, String> {
        if (1..=Self::MAX_HISTORY_SYNC_DAYS).contains(&value) {
            Ok(value)
        } else {
            Err(format!(
                "History backfill days must be between 1 and {}",
                Self::MAX_HISTORY_SYNC_DAYS
            ))
        }
    }

    /// Whether this backfill fetches data that cleanup removes right after.
    ///
    /// "Backfilled, then deleted at the next successful sync" destroys trust, so this
    /// combination is blocked before it starts, not explained after.
    pub fn backfill_would_be_cleaned_up(&self, requested_days: i64) -> bool {
        !self.archive_enabled && requested_days > self.retention_days
    }
}

/// The storage estimate for one stream.
///
/// It is per stream because "how much space for three more years" depends on whether the
/// user wears the watch to sleep or runs. One global constant gives the same number to a
/// daily runner and to someone who runs twice a year, and it helps neither.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StreamStorageEstimate {
    pub stream: String,
    /// How many distinct days this machine already has. Too few samples cannot be extrapolated.
    pub observed_days: i64,
    /// The raw payload bytes of this stream on this machine.
    pub observed_bytes: u64,
    pub bytes_per_day: u64,
    /// true = computed from the local data volume; false = too few local samples, no estimate.
    pub measured: bool,
    pub estimated_add_bytes: u64,
}

/// The result of one history payload compaction.
///
/// It reports "compacted" and "skipped" counts separately. A skip is not a failure, but
/// it is not a success either. The user pressed a button and must know what changed.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RawPayloadCompaction {
    pub compacted: u64,
    pub skipped: u64,
    pub bytes_before: u64,
    pub bytes_after: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StorageEstimate {
    pub free_bytes: u64,
    pub estimated_add_bytes: u64,
    pub database_bytes: u64,
    pub allow_long_history: bool,
    pub warn_tight_space: bool,
    pub message: String,
    /// The stable code for the `message` text. The UI picks its own text by it and formats
    /// the numbers below. The backend does not produce locale text.
    #[serde(default)]
    pub message_code: String,
    /// How many days this estimate covers.
    #[serde(default)]
    pub requested_days: i64,
    #[serde(default)]
    pub streams: Vec<StreamStorageEstimate>,
    /// True only when all six streams have enough local samples. When false, the total is only a rough guide.
    #[serde(default)]
    pub measured: bool,
    /// Not None means there is not enough space to start this backfill; the value is the reason for the user.
    #[serde(default)]
    pub stop_reason: Option<String>,
    /// The stable code for the `stop_reason` text.
    #[serde(default)]
    pub stop_reason_code: Option<String>,
    /// The estimated bytes this backfill needs, with a safety margin. The UI uses it in the stop_reason text.
    #[serde(default)]
    pub needed_bytes: u64,
}

/// The storage representation of a sync stream, carrying the cursor/capability
/// bookkeeping needed by the real pipeline.
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SyncStateInfo {
    pub stream: String,
    pub last_sync: Option<DateTime<Utc>>,
    pub cursor: Option<String>,
    pub status: String,
    pub error: Option<String>,
    pub needs_reauth: bool,
    pub records_written: i64,
    pub capability: String,
    pub message: Option<String>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityStatus {
    Verified,
    Unverified,
    Unavailable,
}

impl CapabilityStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Verified => "verified",
            Self::Unverified => "unverified",
            Self::Unavailable => "unavailable",
        }
    }
}

/// One row of the capability overview shown in settings.
///
/// `status` is deliberately not a boolean. This API answers "200 with no
/// items" for event names that cannot possibly exist, so an absence of data
/// never proves a device lacks a sensor - only an outright rejection does.
/// Telling someone their watch does not support blood pressure when they have
/// simply never measured would send them shopping for hardware they own.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityItem {
    /// Stable key the UI maps to a label.
    pub stream: String,
    /// `available` - data is on disk.
    /// `no_records` - no local rows or no records returned by a candidate.
    /// `unsupported` - reserved for established support evidence, not guesses.
    /// `unknown` - never checked, request failure or unrecognized response.
    pub status: String,
    /// How many rows back this up, when there are any.
    pub records: i64,
    /// Unit for `records`, e.g. the word for "days" or "records".
    ///
    /// This one is for the CLI / MCP / local API. Their output ignores the UI language, so a
    /// change here changes what external tools see. The UI reads `records_unit_code` below.
    pub records_unit: String,
    /// The stable unit code (`days` / `records`). The UI builds its text from it.
    ///
    /// The backend does not produce locale text on purpose: GUI / CLI / MCP / export must give
    /// the same answer to the same question. So the backend sends codes and the UI translates.
    #[serde(default)]
    pub records_unit_code: String,
    /// The number of days in this stream's check window. The UI uses it to say "no records in the last N days".
    #[serde(default)]
    pub window_days: i64,
    /// Newest calendar date behind this capability.
    pub latest_date: Option<String>,
    /// One plain sentence about the data - never a claim about the hardware
    /// unless the server actually rejected the stream.
    pub note: Option<String>,
    /// `derived` when read from stored data, `probed` when it took a request.
    pub source: String,
    /// Whether ZeppBridge really read this stream into the local database.
    ///
    /// A probe that says "the cloud has 42" does not mean the local database has them. A
    /// candidate endpoint can return an unknown format. This is true only when usable readings exist locally.
    #[serde(default)]
    pub ingested: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityOverview {
    pub items: Vec<CapabilityItem>,
    /// When the streams that needed a request were last checked.
    pub probed_at: Option<String>,
}

/// The result of asking the server whether one candidate stream exists.
///
/// Zepp's mobile event endpoint has no discovery call, and which streams
/// answer depends on the account, the devices and the region. A probe records
/// only whether a stream answered and the field *names* it used - never a
/// measured value, and nothing is written to the database.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityProbe {
    /// The ZeppBridge stream this candidate would feed, e.g. `spo2`.
    pub stream: String,
    /// Which event surface answered: `v2_events`, `user_events` or
    /// `user_events_day`. The same event name behaves differently on each.
    pub surface: String,
    /// `continuous` or `episodic` - how often the stream is measured, which
    /// decides how far back the probe looks and how silence should be read.
    pub cadence: String,
    pub window_days: i64,
    pub event_type: String,
    pub sub_type: String,
    /// `available` | `empty` | `unavailable` | `error` | `unrecognized`
    pub status: String,
    pub records: usize,
    /// Calendar date of the newest item, for streams measured occasionally.
    pub latest_date: Option<String>,
    pub fields: Vec<String>,
    /// Sanitized machine/numeric failure only; never a server response body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<String>,
}
