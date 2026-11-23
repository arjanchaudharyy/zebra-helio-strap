use crate::decoder::{decode_workout_detail, DecodedWorkout};

use crate::models::{error::*, *};

use crate::normalizer::Normalizer;

use chrono::{DateTime, Duration, Local, NaiveDate, Utc};

use rusqlite::{params, Connection, OptionalExtension};

use serde::{Deserialize, Serialize};

use sha2::{Digest, Sha256};

use std::collections::{BTreeMap, BTreeSet, HashMap};

use std::path::{Path, PathBuf};

/// Current SQLite schema version (`PRAGMA user_version`). A new version may only append migration
/// steps. Do not change existing DDL.
pub const CURRENT_SCHEMA_VERSION: i64 = 53;

/// App version written to the backup manifest. Core is a separate crate and uses its own package version.
const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Accepted `ExportSelection::data_types`, shared with callers that validate input.
pub const EXPORT_DATA_TYPES: [&str; 19] = [
    "blood_pressure",
    "heart_rate",
    "hrv",
    "hrv_rmssd",
    "respiratory_rate",
    "pai",
    "lactate_threshold",
    "daily_activity",
    "sleep",
    "workouts",
    "recovery",
    "steps",
    "spo2",
    "stress",
    "training_load",
    "vo2max",
    "weight",
    // Food was once registered only in the capability table (`CapabilityEvidence::DailyPrefix("intake_")`)
    // and never in this allow list: `--types food` was silently dropped, so intake data
    // was stored and charted but could not be exported.
    "food",
    "life_events",
];

/// Parser revision. **Bump it by one whenever you change the workout catalog or any normalization rule.**
///
/// When the stored revision differs from this one at start,
/// `raw_records` is replayed. Without a bump, new type ids apply only to records synced later,
/// and the 199 records already stored as `unknown:211` stay that way forever. The person who reported the problem
/// reported it because of those old records.
pub const NORMALIZER_REVISION: &str = "zepp-normalizer-2026-11-v46-strength-rows";

/// A metric actually present in the local library. This inventory deliberately
/// includes names outside the chart contract so new normalized data is findable.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredMetric {
    pub metric: String,
    pub source: String,
    pub unit: String,
    pub records: i64,
    pub first_date: String,
    pub last_date: String,
}

/// One stored reading, without device identifiers or raw cloud payloads.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredMetricRecord {
    pub date: String,
    pub timestamp: Option<String>,
    pub value: f64,
    pub unit: String,
    pub source_scope: String,
}

/// Revision of an earlier public release, used to test upgrades across versions.
///
/// v21 does not have the v22 lap parsing or the v23 Rucking mapping. An upgrade that skips versions must add both.
#[cfg(test)]
const PREVIOUS_RELEASE_NORMALIZER_REVISION: &str = "zepp-normalizer-2026-09-v21-elliptical";

const LAST_CLOUD_SYNC_AT_KEY: &str = "last_cloud_sync_at";

const LAST_CLOUD_SYNC_OUTCOME_KEY: &str = "last_cloud_sync_outcome";

const LAST_LOCAL_REPROCESS_AT_KEY: &str = "last_local_reprocess_at";

const REPLAY_LAST_FAILURES_KEY: &str = "replay_last_failures";

const RETENTION_DAYS_KEY: &str = "retention_days";

const HISTORY_SYNC_DAYS_KEY: &str = "history_sync_days";

const ARCHIVE_ENABLED_KEY: &str = "archive_enabled";

const HEART_RATE_ZONE_PREF_KEY: &str = "heart_rate_zone_preference";

const BYTES_PER_HISTORY_DAY: u64 = 800_000;

/// How many raw payloads one replay transaction handles.
///
/// There was once no transaction here, so each derived row insert auto-committed. On an 842 MB database, replaying
/// only the wellness stream took 237 seconds, and almost none of that time went to parsing. It all went to
/// the fsync of each commit. One big transaction for the whole database is a bit faster, but the WAL must then hold all
/// derived data. Batch commits give a speedup of the same order and keep peak WAL within one batch,
/// which matters because this code runs on NAS devices and containers.
const REPLAY_BATCH_RECORDS: usize = 64;

/// Plain payloads read into memory per compaction step. Loading the whole table into one Vec fills RAM on old databases.
const COMPACTION_BATCH_RECORDS: usize = 32;

const RAW_PAYLOAD_STATS_KEY: &str = "raw_payload_stats_v1";

const RAW_PAYLOAD_STATS_GEN_KEY: &str = "raw_payload_stats_gen";

/// Fewer days of local samples than this are not enough to extrapolate the usage rate.
const MIN_OBSERVED_DAYS: i64 = 7;

/// Keep 200 MB on top of the estimate. A disk filled exactly full is as bad as one that is too small.
const SPACE_SAFETY_MARGIN_BYTES: u64 = 200 * 1024 * 1024;

pub mod backup;
pub mod detail_queue;
pub mod workout_edits;

pub mod corrections;
pub mod device_local_state;

pub mod coverage;

pub mod life_events;
pub mod logbook;
pub mod rings;

mod migrations;

pub mod provenance;

pub mod write_lock;

mod ai_export;
mod capability;
mod devices;
mod diagnostics;
mod estimate;
mod event_windows;
pub mod feedback_toast;
pub mod fit_override;
mod freshness;
mod guards;
mod hourly_steps;
mod hr_zones;
pub mod import_health;
pub mod imports;
mod ingest;
mod maintenance;
mod meta;
pub mod metric_prefs;
mod metric_spec;
mod metrics;
pub mod my_records;
mod official;
mod open;
mod owner;
pub use owner::configured_accounts;
pub mod ai_exchanges;
pub mod ai_strip;
pub mod card_collection;
pub mod coach_notes;
pub mod mcp_writes;
mod payload;
pub mod plan_adherence;
mod queries;
mod replay;
pub mod training_context;
pub(crate) mod training_coverage;
pub mod training_plan;
mod util;
pub mod workout_feedback;
mod workout_row;
mod workout_series;
mod writes;

pub(crate) use ai_export::*;
pub use capability::*;
pub use devices::*;
pub use fit_override::{FitOverrideOutcome, WorkoutFitOverride, FIT_START_TOLERANCE_SECONDS};
pub use freshness::*;
pub use guards::*;
use ingest::SerializedPayload;
pub(crate) use metric_spec::*;
pub use open::*;
use payload::*;
pub(crate) use queries::*;
pub use replay::*;
use util::*;
pub(crate) use workout_row::*;

pub struct Database {
    /// Visible inside the crate: Core modules such as insights and backup reuse this connection directly
    /// instead of each opening another one that competes for the lock. Outside the crate, only public methods are available.
    pub(crate) conn: Connection,
    /// When greater than 0, per-stream sync state (`sync_state`, phase, write counts) is not written. See
    /// [`Database::quiet_sync_state`].
    quiet_state: std::sync::atomic::AtomicU32,
    /// Which fallback a read-only open used, if any (see `open_read_only_any_version`).
    read_fallback: Option<ReadFallback>,
}

/// While held, this connection does not write user-visible per-stream sync state (it writes only data).
pub struct QuietSyncState<'a> {
    db: &'a Database,
}

impl Drop for QuietSyncState<'_> {
    fn drop(&mut self) {
        self.db
            .quiet_state
            .fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

impl Database {
    pub(super) fn from_conn(conn: Connection) -> Self {
        Self {
            conn,
            quiet_state: std::sync::atomic::AtomicU32::new(0),
            read_fallback: None,
        }
    }

    /// The next writes store only data. They do not touch the per-stream state rows that the "data health" page reads.
    ///
    /// Official supplemental sync and history backfill once wrote `sync_state` directly: the legacy connector's sleep failed, but the official supplement
    /// turned it green on success. The `daily_summary` synced count showed the number of official hourly step rows. A backfill of one
    /// 2019 month made the heart rate row show that month's count or failure. The user-visible rows belong only to regular
    /// sync. Backfill has its own coverage ledger, and supplemental sync results go in their own report.
    pub fn quiet_sync_state(&self) -> QuietSyncState<'_> {
        self.quiet_state
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        QuietSyncState { db: self }
    }

    /// Same as [`Self::quiet_sync_state`], but it lasts until this connection closes. For connections opened only for one supplemental sync
    /// (a borrowing guard cannot be held across await).
    pub fn keep_sync_state_quiet(&self) {
        self.quiet_state
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }

    pub(crate) fn sync_state_is_quiet(&self) -> bool {
        self.quiet_state.load(std::sync::atomic::Ordering::SeqCst) > 0
    }
}

#[cfg(test)]
mod tests;
