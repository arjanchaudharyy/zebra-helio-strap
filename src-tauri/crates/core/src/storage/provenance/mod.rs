use super::{Database, NORMALIZER_REVISION};

use crate::models::error::Result;

use chrono::{Duration, NaiveDate, Utc};

use serde::{Deserialize, Serialize};

use std::collections::BTreeMap;

mod health;

pub const LAST_LOCAL_REPLAY_AT_KEY: &str = "last_local_replay_at";

pub const LAST_MANUAL_REPROCESS_AT_KEY: &str = "last_manual_reprocess_at";

pub const LAST_INTEGRITY_CHECK_KEY: &str = "last_integrity_check";

/// Maximum number of gap dates listed in a coverage explanation. Listing every gap
/// when there are many is only noise, so the page shows the first N plus a total.
const MAX_REPORTED_GAPS: usize = 12;

/// The cadence of a data stream. A "blank" means different things per cadence: a missing day
/// in a continuous stream is a gap, in an occasional stream it is normal. One completeness percentage misleads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StreamCadence {
    /// Many samples per day, for example heart rate.
    Continuous,
    /// One record per day, for example steps or resting heart rate.
    Daily,
    /// One record per night, for example sleep or HRV.
    Nightly,
    /// Present only when something happens, for example workouts.
    PerEvent,
    /// The watch reports it only now and then, for example VO₂max or lactate threshold. A blank is not a fault.
    Occasional,
}

impl StreamCadence {
    /// Whether this cadence can be explained as "which days are missing". Occasional and per-event streams cannot.
    fn has_expected_days(self) -> bool {
        matches!(
            self,
            StreamCadence::Continuous | StreamCadence::Daily | StreamCadence::Nightly
        )
    }
}

/// The state of one of the three stages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StageState {
    /// `ok` / `failed` / `never`. `never` means "this step never ran".
    /// It is not a failure, and the UI must not draw it in red.
    pub state: String,
    /// Time of the most recent success (`ok`) or failure (`failed`).
    pub at: Option<String>,
    /// Time of the most recent success. Kept even in a failed state, to show "when it last worked".
    pub last_ok_at: Option<String>,
    /// Stable failure category, see [`StageErrorKind`].
    pub error_kind: Option<String>,
    pub message: Option<String>,
}

impl StageState {
    fn never() -> Self {
        Self {
            state: "never".into(),
            at: None,
            last_ok_at: None,
            error_kind: None,
            message: None,
        }
    }
}

/// Failure category. The strings are part of the contract: the CLI, MCP and UI all branch on them.
/// Do not change these values to make the text look better.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StageErrorKind {
    /// Network unreachable, timeout, connection refused.
    Network,
    /// The user must sign in to Zepp again.
    Auth,
    /// The cloud states that this account/device has no such stream.
    NotAvailable,
    /// The payload arrived, but the current normalizer does not recognize its structure.
    UnrecognizedPayload,
    /// HTTP 200, but the payload itself says "not successful". See `ZeppBridgeError::CloudRejected`.
    ///
    /// Carries the code: it is the only number in the system that can tell us "what an expired
    /// credential looks like", and today we can only get it from users (all 1075 locally
    /// retained payloads have `code = 1`). It lives in the type, not only in the human-readable message,
    /// so that a diagnostic report can send only this one integer and not the cloud's original text.
    CloudRejected {
        code: i64,
    },
    /// Writing to the local database failed.
    Storage,
    /// Another process is writing to the same database, so this run stepped aside. Retryable, not broken.
    Busy,
    /// Cancelled by the user.
    Cancelled,
    Unknown,
}

impl StageErrorKind {
    pub fn as_str(self) -> &'static str {
        match self {
            StageErrorKind::Network => "network",
            StageErrorKind::Auth => "auth",
            StageErrorKind::NotAvailable => "not_available",
            StageErrorKind::UnrecognizedPayload => "unrecognized_payload",
            StageErrorKind::CloudRejected { .. } => "cloud_rejected",
            StageErrorKind::Storage => "storage",
            StageErrorKind::Busy => "busy",
            StageErrorKind::Cancelled => "cancelled",
            StageErrorKind::Unknown => "unknown",
        }
    }
}

impl StageErrorKind {
    /// Maps an internal error to a stable stage failure category.
    ///
    /// This classifies "what the user should do next". It is not a transcription of the error text:
    /// `auth` means reconnect, `network` is worth a retry, `not_available` only says this
    /// endpoint gave no usable data. It cannot tell whether the device measured or other endpoints have data.
    pub fn classify(error: &crate::models::ZeppBridgeError) -> Self {
        use crate::models::error::HeadlessProblem;
        use crate::models::ZeppBridgeError as E;
        match error {
            E::Cancelled => StageErrorKind::Cancelled,
            E::NeedsReauth(_) | E::AuthError(_) | E::CredentialStore(_) => StageErrorKind::Auth,
            // The database belongs to another account: same class as auth. The user must change account settings; retry does not help.
            E::AccountMismatch => StageErrorKind::Auth,
            // Three headless cases: two are "cannot get the token", one is "the database must be upgraded first".
            // The first two are classed as auth (the user must supply credentials). The third is a local storage
            // issue: retry cannot fix it, a reprocess must run first.
            // A database newer than the program (old CLI / MCP) is the same: replace the program, do not retry.
            E::Headless(
                HeadlessProblem::SchemaUpgradeRequired { .. }
                | HeadlessProblem::ProgramOutdated { .. },
            ) => StageErrorKind::Storage,
            E::Headless(_) => StageErrorKind::Auth,
            E::Unavailable(_) | E::DataUnavailable(_) | E::WorkoutFitOriginalMissing => {
                StageErrorKind::NotAvailable
            }
            // A timeout is usually a slow network: same class as network, a retry is enough.
            E::NetworkError(_)
            | E::RetryExhausted { .. }
            | E::HttpStatus { .. }
            | E::TimedOut(_) => StageErrorKind::Network,
            E::ParseError(_) => StageErrorKind::UnrecognizedPayload,
            // Transport succeeded, business layer rejected. A separate class, not merged into `unknown`: in the
            // diagnostic report this cell is the only place we can see "which code the cloud actually returned".
            E::CloudRejected { code, .. } => StageErrorKind::CloudRejected { code: *code },
            E::Busy(_) => StageErrorKind::Busy,
            E::DatabaseError(_) | E::IoError(_) => StageErrorKind::Storage,
            // ai_task / training plan errors do not occur in the sync pipeline. If one does, it counts as "unknown".
            E::InvalidHost(_)
            | E::ConfigError(_)
            | E::AiTask(_)
            | E::TrainingPlan { .. }
            | E::Unknown(_) => StageErrorKind::Unknown,
        }
    }
}

/// The result of one stage. The smallest unit written to the provenance table.
#[derive(Debug, Clone)]
pub enum StageOutcome {
    Ok,
    Failed {
        kind: StageErrorKind,
        message: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Fetch,
    Parse,
    Write,
}

impl Stage {
    fn column_prefix(self) -> &'static str {
        match self {
            Stage::Fetch => "fetch",
            Stage::Parse => "parse",
            Stage::Write => "write",
        }
    }
}

/// Record count of a data stream, split by source.
///
/// `device` = reported by a single device, `user_fused` = fused by Zepp in the cloud, `unknown` = the payload
/// did not say. An unknown source is not silently treated as device data, and values from different devices are never summed or averaged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceBreakdown {
    pub source: String,
    pub records: i64,
}

/// Coverage explanation. Phrased per stream cadence, never as one shared completeness percentage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoverageExplanation {
    /// `gaps` = we can say "which days are missing"; `observations` = we can only say "which days were observed".
    pub kind: String,
    pub window_days: i64,
    pub observed_days: i64,
    /// Meaningful only when `kind == "gaps"`. Lists at most [`MAX_REPORTED_GAPS`] entries.
    pub gap_dates: Vec<String>,
    pub gap_total: i64,
    pub first_observed_at: Option<String>,
    pub latest_observed_at: Option<String>,
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamHealth {
    pub stream: String,
    pub label: String,
    pub cadence: StreamCadence,
    pub fetch: StageState,
    pub parse: StageState,
    pub write: StageState,
    pub raw_records: i64,
    pub canonical_records: i64,
    pub last_written_records: i64,
    pub sources: Vec<SourceBreakdown>,
    pub coverage: CoverageExplanation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DatabaseHealth {
    pub schema_version: i64,
    /// The rule version this program parses with.
    pub normalizer_revision: String,
    /// The rule version that produced the derived data in the database. `None` = never replayed.
    ///
    /// Kept separate from the value above because the two can differ, and a difference is exactly what must be reported:
    /// reporting only the program's own revision tells a database still on old rules
    /// "revision: current". The desktop app replays at startup, so there they are almost always equal.
    /// Only CLI users skip that startup, so the two values can differ by several versions.
    #[serde(default)]
    pub stored_normalizer_revision: Option<String>,
    /// The database holds old or unprocessed payloads that need a local replay.
    #[serde(default)]
    pub normalizer_replay_pending: bool,
    /// A background replay is running. Cloud sync yields with `deferred` meanwhile. This is not a failure.
    pub replay_in_progress: bool,
    pub database_bytes: u64,
    pub raw_records: i64,
    pub canonical_records: i64,
    /// Raw records without a successful attempt or quarantine at the current revision.
    pub pending_normalization: i64,
    /// Per-stream processing outcomes. `processed_without_output` means the
    /// parser returned no rows; it does not claim the payload is understood.
    /// `empty_response` means Zepp explicitly answered with an empty list for
    /// that window (nothing there); `quarantined` is reserved for payloads the
    /// parser could not read (#143).
    #[serde(default)]
    pub normalization_by_stream: Vec<NormalizationHealth>,
    /// Result of the most recent `PRAGMA integrity_check`. `None` = never run.
    /// This is an explicit action. It does not run each time the page opens.
    pub last_integrity_check: Option<IntegrityCheckResult>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NormalizationHealth {
    pub stream: String,
    pub state: String,
    pub records: i64,
}

impl Database {
    fn normalization_health(&self) -> Result<Vec<NormalizationHealth>> {
        let mut stmt = self.conn.prepare(
            "SELECT r.stream,
                CASE WHEN q.revision = ?1 AND q.empty_response = 1 THEN 'empty_response'
                     WHEN q.revision = ?1 THEN 'quarantined'
                     WHEN n.revision = ?1 AND n.records_written > 0 THEN 'normalized'
                     WHEN n.revision = ?1 THEN 'processed_without_output'
                     ELSE 'pending' END AS state,
                COUNT(*)
             FROM raw_records r
             LEFT JOIN raw_normalization n ON n.raw_record_id = r.id
             LEFT JOIN raw_quarantine q ON q.raw_record_id = r.id
             GROUP BY r.stream, state ORDER BY r.stream, state",
        )?;
        let rows = stmt.query_map([NORMALIZER_REVISION], |row| {
            Ok(NormalizationHealth {
                stream: row.get(0)?,
                state: row.get(1)?,
                records: row.get(2)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntegrityCheckResult {
    pub checked_at: String,
    pub ok: bool,
    /// SQLite's first message on failure. Contains no file paths.
    pub detail: Option<String>,
}

/// Four separate timestamps that must not stand in for each other.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthTimings {
    pub last_cloud_sync_at: Option<String>,
    pub last_cloud_sync_outcome: Option<String>,
    pub last_local_replay_at: Option<String>,
    pub last_manual_reprocess_at: Option<String>,
    /// Time of the newest health sample in the whole database. Not the same as any of the three above.
    pub newest_sample_at: Option<String>,
}

/// Repair actions the page can run directly. The id is a stable contract; the frontend maps it to existing commands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthAction {
    pub id: String,
    /// Stable code of the action. The UI picks its text from it.
    ///
    /// Kept separate from `id` because `id` is for **execution** (two different actions run the same sync
    /// command, so their id is the same), while the text must tell "sync again" from "run the first sync".
    /// `label` / `reason` are CLI output and do not follow the UI language.
    #[serde(default)]
    pub code: String,
    pub label: String,
    pub reason: String,
    /// Actions that need a second confirmation (cleanup, restore and so on).
    pub destructive: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataHealth {
    pub generated_at: String,
    pub database: DatabaseHealth,
    pub timings: HealthTimings,
    pub streams: Vec<StreamHealth>,
    /// Occasional metrics form their own group: they show only observed dates and the latest one, with no gap check.
    pub occasional_metrics: Vec<StreamHealth>,
    pub actions: Vec<HealthAction>,
}

/// Sync stream catalog. The order is the UI order.
const STREAM_CATALOG: [(&str, &str, StreamCadence); 8] = [
    ("heart_rate", "Heart rate", StreamCadence::Continuous),
    ("daily_summary", "Daily summary", StreamCadence::Daily),
    ("sleep", "Sleep", StreamCadence::Nightly),
    ("hrv", "Heart rate variability", StreamCadence::Nightly),
    ("wellness", "Stress / SpO2 and other optional metrics", StreamCadence::Daily),
    ("workouts", "Workouts", StreamCadence::PerEvent),
    ("workout_detail", "Workout details and tracks", StreamCadence::PerEvent),
    // Weighing exists only when the user weighs in. It is not daily data: a week off the scale is not a fault, so
    // it is Occasional, not Daily. That way this row is not drawn as a red gap all year.
    ("weight", "Weight and body composition", StreamCadence::Occasional),
];

/// Metrics with a known cadence. Any metric not listed here is treated as `Occasional`. Better to report
/// one gap too few than to draw a metric the watch rarely sends as a red fault.
fn metric_cadence(metric: &str) -> StreamCadence {
    match metric {
        "heart_rate" => StreamCadence::Continuous,
        "steps" | "calories" | "distance" | "active_minutes" | "resting_heart_rate"
        | "training_load" | "stress" | "blood_oxygen" | "all_day_stress" => StreamCadence::Daily,
        "hrv" | "hrv_rmssd" | "sleep_score" | "breathing_rate" | "skin_temperature" => {
            StreamCadence::Nightly
        }
        _ => StreamCadence::Occasional,
    }
}

fn metric_label(metric: &str) -> String {
    match metric {
        "vo2max" => "VO₂max".into(),
        "lactate_threshold_hr" => "Lactate threshold heart rate".into(),
        "lactate_threshold_pace" => "Lactate threshold pace".into(),
        "resting_heart_rate" => "Resting heart rate".into(),
        "training_load" => "Training load".into(),
        "blood_oxygen" => "Blood oxygen".into(),
        "breathing_rate" => "Breathing rate".into(),
        "skin_temperature" => "Skin temperature".into(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests;
