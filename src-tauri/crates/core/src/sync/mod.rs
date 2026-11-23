use crate::fetcher::{DataFetcher, FetchWindow, FetchedRecord};

use crate::models::{error::*, *};

use crate::storage::coverage::{self, ChunkStatus, CoverageChunk, CoverageLedger};

use crate::storage::provenance::{Stage, StageErrorKind, StageOutcome};

use crate::storage::write_lock::{self, ExclusiveWriteGuard, WritePurpose};

use crate::storage::Database;

use chrono::{DateTime, Duration, NaiveDate, Utc};

use serde::{Deserialize, Serialize};

use std::sync::atomic::{AtomicBool, Ordering};

use std::sync::Arc;

use std::time::Instant;

/// The maximum wait for another writer. After it, tell the user "another operation is in progress"
/// instead of a spinner that never stops.
const WRITE_LOCK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

/// The interval between lock attempts.
const LOCK_POLL: std::time::Duration = std::time::Duration::from_millis(120);

use tokio::sync::Mutex;

mod backfill;
mod chunked;
mod details;
mod official;
mod persist;
mod probe;
mod report;

use chunked::OnChunkError;
pub use official::{OfficialMode, OfficialSync};
pub use probe::{conclude as conclude_cloud_probe, probe_since, CloudProbe, PROBE_TIMEOUT};
use report::*;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StreamStatus {
    Success,
    Failed,
    Unavailable,
    Unverified,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamReport {
    pub stream: String,
    pub status: StreamStatus,
    pub records_written: i64,
    pub raw_records: i64,
    pub capability: CapabilityStatus,
    pub needs_reauth: bool,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncReport {
    /// Whether this sync succeeded as a whole.
    ///
    /// **Any data stream that really is `Failed` makes it false**, not only the three core
    /// streams. Before, it only checked `heart_rate` / `daily_summary` / `workouts`, so when sleep
    /// or hrv failed, the UI still showed "updated" or "no new data" while a whole
    /// stream was missing.
    ///
    /// `Unavailable` / `Unverified` **are not failures**: the user's device may not provide
    /// HRV. That is a capability limit, not an error.
    pub success: bool,
    /// Whether none of the three core streams (`heart_rate` / `daily_summary` / `workouts`)
    /// failed. It decides whether the credentials count as verified. Retention
    /// cleanup uses `success` instead: if any stream is `Failed`, old data is not deleted.
    pub core_ok: bool,
    pub streams: Vec<StreamReport>,
    pub records_written: i64,
    pub message: Option<String>,
    /// Old-data cleanup after the sync failed (disk full, lock held). The data is synced, so the result does not change.
    /// Without this flag the database keeps growing while the UI stays green.
    #[serde(default)]
    pub cleanup_failed: bool,
}

/// These three are the core streams. Without them the app has no purpose; the others are secondary.
///
/// It is only used for `core_ok`. **`success` treats all streams the same**: a failed secondary
/// stream is still a failure, but it does not downgrade the whole sync to `failed`.
const CORE_STREAMS: [&str; 3] = ["heart_rate", "daily_summary", "workouts"];

/// Whether a stream name is a core stream.
pub fn is_core_stream(stream: &str) -> bool {
    CORE_STREAMS.contains(&stream)
}

pub struct SyncManager {
    fetcher: Arc<DataFetcher>,
    db: Arc<Mutex<Database>>,
    run_lock: Arc<Mutex<()>>,
    /// The scope of the cross-process write lock. With `None`, only in-process mutual exclusion (in-memory test database).
    data_dir: Option<std::path::PathBuf>,
    /// Whether this round is a sync or a backfill. The write lock reports this purpose: it is shown as is
    /// to the other writer that waits, so it must name the real work.
    run_purpose: std::sync::Mutex<WritePurpose>,
    pub cancel: Arc<AtomicBool>,
    /// The account this syncer writes for. When set, each round checks the database owner first.
    account: Option<String>,
}

/// A short database write: take the cross-process write lock first, then this connection. Both release at scope end.
///
/// A sync holds only the sync lease for its whole run (see `write_lock::acquire_sync_lease`). The write lock is held only
/// in these short sections, so during the network time other writers (settings, life events, backups)
/// can still write.
pub(super) struct WriteSession<'a> {
    db: tokio::sync::MutexGuard<'a, Database>,
    _lock: Option<ExclusiveWriteGuard>,
}

impl std::ops::Deref for WriteSession<'_> {
    type Target = Database;

    fn deref(&self) -> &Database {
        &self.db
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncProgress {
    /// True only after this stream has finished persisting and released its DB lock.
    #[serde(default)]
    pub completed: bool,
    pub stream: String,
    pub current: u32,
    pub total: u32,
    /// The fallback text. The CLI and logs use it, whatever the UI language is.
    pub message: String,
    /// A stable code for this step (`syncing` / `backfilling`). The UI builds its own sentence from it and
    /// `stream`. The backend does not make text per locale, so all four outputs say the same thing.
    #[serde(default)]
    pub code: String,
    /// The month of this backfill chunk (`YYYY-MM`). Empty during a sync.
    #[serde(default)]
    pub detail: Option<String>,
}

struct PersistResult {
    report: StreamReport,
}

impl SyncManager {
    /// The `cancel` flag is shared with the underlying connector so an
    /// in-flight HTTP retry loop aborts as soon as cancellation is requested.
    pub fn new(fetcher: DataFetcher, db: Database, cancel: Arc<AtomicBool>) -> Self {
        Self {
            fetcher: Arc::new(fetcher),
            db: Arc::new(Mutex::new(db)),
            run_lock: Arc::new(Mutex::new(())),
            data_dir: None,
            run_purpose: std::sync::Mutex::new(WritePurpose::Sync),
            cancel,
            account: None,
        }
    }

    /// Declare which account's data this syncer writes. Each round checks the database owner first. If it is
    /// not the same account, nothing is written.
    pub fn with_account(mut self, user_id: impl Into<String>) -> Self {
        self.account = Some(user_id.into());
        self
    }

    /// With a data directory set, the sync also takes the cross-process write lock.
    pub fn with_data_dir(mut self, data_dir: std::path::PathBuf) -> Self {
        self.data_dir = Some(data_dir);
        self
    }

    /// Wait for a cross-process lock. On timeout, tell the caller who holds it instead of hanging.
    ///
    /// Poll in async code instead of the blocking `acquire_with_timeout`: the wait must not hold
    /// a runtime worker thread, and a user cancel during the wait must stop it at once.
    async fn acquire_lock(
        &self,
        acquire: fn(
            &std::path::Path,
            WritePurpose,
            std::time::Duration,
        )
            -> std::result::Result<ExclusiveWriteGuard, write_lock::WriteLockError>,
        purpose: WritePurpose,
    ) -> Result<Option<ExclusiveWriteGuard>> {
        let Some(data_dir) = self.data_dir.as_ref() else {
            return Ok(None);
        };
        let deadline = Instant::now() + WRITE_LOCK_TIMEOUT;
        loop {
            match acquire(data_dir, purpose, std::time::Duration::ZERO) {
                Ok(guard) => return Ok(Some(guard)),
                Err(write_lock::WriteLockError::Busy { .. }) if Instant::now() < deadline => {
                    self.abort_if_cancelled()?;
                    tokio::time::sleep(LOCK_POLL).await;
                }
                // Keep "someone is writing" apart from "the lock cannot be created": the first is retryable, the second needs a person.
                Err(error @ write_lock::WriteLockError::Busy { .. }) => {
                    return Err(ZeppBridgeError::Busy(error.to_string()))
                }
                Err(error) => return Err(ZeppBridgeError::ConfigError(error.to_string())),
            }
        }
    }

    /// Start a sync or backfill round: take the sync lease and record the purpose of this round.
    pub(super) async fn begin_run(
        &self,
        purpose: WritePurpose,
    ) -> Result<Option<ExclusiveWriteGuard>> {
        let lease = self
            .acquire_lock(write_lock::acquire_sync_lease, purpose)
            .await?;
        *self
            .run_purpose
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = purpose;
        if let Some(account) = self.account.as_deref() {
            self.write_db_for(purpose)
                .await?
                .claim_library_for_sync(account)?;
        }
        Ok(lease)
    }

    /// A short database write, with the purpose of the current round.
    pub(super) async fn write_db(&self) -> Result<WriteSession<'_>> {
        let purpose = *self
            .run_purpose
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        self.write_db_for(purpose).await
    }

    pub(super) async fn write_db_for(&self, purpose: WritePurpose) -> Result<WriteSession<'_>> {
        let lock = self
            .acquire_lock(write_lock::acquire_with_timeout, purpose)
            .await?;
        Ok(WriteSession {
            db: self.db.lock().await,
            _lock: lock,
        })
    }

    pub fn request_cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    /// While local maintenance (replay / compaction) writes the database, do not compete for the 20-second write lock.
    pub(super) fn stand_aside_if_local_maintenance(&self) -> Result<()> {
        if crate::storage::compaction_in_progress() || crate::storage::replay_in_progress() {
            let message = if crate::storage::compaction_in_progress() {
                "Compacting history payloads to save disk space"
            } else {
                "Rebuilding derived data from local raw payloads"
            };
            return Err(ZeppBridgeError::Busy(message.into()));
        }
        Ok(())
    }

    pub(super) fn abort_if_cancelled(&self) -> Result<()> {
        if self.cancel.load(Ordering::SeqCst) {
            return Err(ZeppBridgeError::Cancelled);
        }
        Ok(())
    }

    /// Signal cancellation and wait until any in-flight run has released
    /// `run_lock`. Returns immediately when nothing is running.
    pub async fn cancel_and_wait(&self) {
        self.request_cancel();
        let _guard = self.run_lock.lock().await;
    }

    /// Ask the server which optional event streams this account and these
    /// devices actually expose.
    ///
    /// Only probe metadata is saved for the capability board; no health
    /// readings or raw responses are imported by this action. The day probed
    /// is yesterday, which is the most recent day a watch has certainly
    /// finished syncing.
    pub async fn probe_capabilities(&self) -> Result<Vec<CapabilityProbe>> {
        // Like a sync: clear the cancel flag after run_lock is held. Before, the probe did not clear it. After one cancelled sync,
        // the flag stayed up, every connector request returned Cancelled, and the probe silently returned nothing.
        // If a sync is running, do not wait for it (it can take minutes). Report busy; that round refreshes capabilities itself.
        let Ok(_run_guard) = self.run_lock.try_lock() else {
            return Err(ZeppBridgeError::Busy("A sync is in progress; probe again later".into()));
        };
        self.cancel.store(false, Ordering::SeqCst);
        let day = (Utc::now() - Duration::days(1)).date_naive();
        // The dateString surface wants an IANA zone name; the devices already
        // told us theirs, so ask them rather than assuming UTC.
        let time_zone = {
            let database = self.db.lock().await;
            database.device_time_zone().unwrap_or(None)
        }
        .unwrap_or_else(crate::official::fetch::system_time_zone);
        let probes = self
            .fetcher
            .probe_event_streams(day, &time_zone, None)
            .await;
        if !probes.is_empty() {
            let database = self.write_db_for(WritePurpose::Metadata).await?;
            database.save_capability_probe(&probes)?;
        }
        Ok(probes)
    }

    /// Check only the streams that leave no local trace, and remember the answer.
    ///
    /// Capability is not something a person should have to ask for by pressing a
    /// button: it is a fact about their account, learned the same way heart rate
    /// is. Most of it comes free from stored data; this covers the remainder, at
    /// three requests, and only once the last answer has gone stale.
    pub async fn refresh_capabilities_if_stale(&self, max_age_days: i64) -> Result<bool> {
        let stale = {
            let database = self.db.lock().await;
            database.capability_probe_is_stale(max_age_days)?
        };
        if !stale {
            return Ok(false);
        }
        let day = (Utc::now() - Duration::days(1)).date_naive();
        let time_zone = {
            let database = self.db.lock().await;
            database.device_time_zone().unwrap_or(None)
        }
        .unwrap_or_else(crate::official::fetch::system_time_zone);
        let probes = self
            .fetcher
            .probe_event_streams(
                day,
                &time_zone,
                Some(&crate::storage::PROBE_ONLY_CAPABILITIES),
            )
            .await;
        self.abort_if_cancelled()?;
        if probes.is_empty() {
            return Ok(false);
        }
        let database = self.write_db().await?;
        database.save_capability_probe(&probes)?;
        Ok(true)
    }

    pub async fn initial_sync_report(&self) -> Result<SyncReport> {
        self.history_sync_report(UserPrefs::DEFAULT_HISTORY_SYNC_DAYS)
            .await
    }

    pub async fn history_sync_report(&self, days: i64) -> Result<SyncReport> {
        self.sync_report(days, None).await
    }

    pub async fn history_sync_report_with_progress<F>(
        &self,
        days: i64,
        on_progress: F,
    ) -> Result<SyncReport>
    where
        F: Fn(SyncProgress) + Send + Sync,
    {
        self.sync_report(days, Some(&on_progress)).await
    }

    pub async fn incremental_sync_report(&self) -> Result<SyncReport> {
        self.sync_report(crate::contract::INCREMENTAL_SYNC_DAYS, None)
            .await
    }

    /// A quiet scheduled sync: fetch only the last [`crate::contract::QUICK_SYNC_DAYS`] days.
    /// When the last successful full-window sync is more than one day old, fetch the full window, so late data is not missed.
    pub async fn quick_sync_report_with_progress<F>(&self, on_progress: F) -> Result<SyncReport>
    where
        F: Fn(SyncProgress) + Send + Sync,
    {
        let refresh_due = {
            let db = self.db.lock().await;
            db.full_window_refresh_due(Utc::now())?
        };
        self.sync_report(quick_window_days(refresh_due), Some(&on_progress))
            .await
    }

    pub async fn incremental_sync_report_with_progress<F>(
        &self,
        on_progress: F,
    ) -> Result<SyncReport>
    where
        F: Fn(SyncProgress) + Send + Sync,
    {
        self.sync_report(crate::contract::INCREMENTAL_SYNC_DAYS, Some(&on_progress))
            .await
    }
}

/// The scheduled sync window: the full window when a full refresh is due, otherwise only the last few days.
pub fn quick_window_days(full_refresh_due: bool) -> i64 {
    if full_refresh_due {
        crate::contract::INCREMENTAL_SYNC_DAYS
    } else {
        crate::contract::QUICK_SYNC_DAYS
    }
}

#[cfg(test)]
mod tests;
