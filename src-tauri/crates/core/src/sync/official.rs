//! Sync orchestration for the official open platform.
//!
//! Rule: **use the side with richer, more accurate data**. A day-by-day check of a real database copy against official samples gave:
//!
//! | Data | Source |
//! |---|---|
//! | Sleep | Official (adds naps and true REM); when both sides have the same night, show the official one |
//! | Hourly steps | Legacy per-minute detail first; official hourly totals only fill gaps (see `storage/hourly_steps.rs`) |
//! | Heart rate, daily steps, workouts, PAI, weight | Legacy is more complete; official only when it is the only connection |
//!
//! So there are two modes. Official only ([`OfficialMode::Only`]): everything in the table comes from official. Both connected
//! ([`OfficialMode::Supplement`]): the legacy channel works as before, and this only adds official sleep and hourly steps.
//!
//! Each chunk (up to 90 days, 7 days for heart rate) is stored and reported to the UI as it arrives, like the legacy `chunked.rs`.

use super::*;
use crate::official::fetch::{
    date_chunks_newest_first, ChunkOutcome, OfficialFetcher, OfficialKind,
};
use crate::official::{
    fresh_tokens, refresh_tokens, OfficialClient, OfficialStore, OfficialTokens,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OfficialMode {
    /// No legacy channel: official is the only source.
    Only,
    /// The legacy channel is also connected: only add what is official-only or better from official.
    Supplement,
}

impl OfficialMode {
    pub fn kinds(self) -> &'static [OfficialKind] {
        match self {
            Self::Only => &[
                OfficialKind::HeartRate,
                OfficialKind::ActivityDaily,
                OfficialKind::ActivityHourly,
                OfficialKind::Sleep,
                OfficialKind::Sports,
                OfficialKind::Pai,
                OfficialKind::Body,
            ],
            Self::Supplement => &[OfficialKind::Sleep, OfficialKind::ActivityHourly],
        }
    }
}

/// The maximum workout details to fetch in one sync. The rest continue next time (a cap like the legacy detail queue).
const DETAIL_BATCH: usize = 20;

pub struct OfficialSync {
    db: Mutex<Database>,
    data_dir: Option<std::path::PathBuf>,
    cancel: Arc<AtomicBool>,
    pub(super) client: OfficialClient,
    pub(super) store: OfficialStore,
    pub(super) time_zone: String,
}

impl OfficialSync {
    pub fn new(
        data_dir: &std::path::Path,
        db: Database,
        cancel: Arc<AtomicBool>,
        time_zone: String,
    ) -> Result<Self> {
        Ok(Self::with_parts(
            Some(data_dir.to_path_buf()),
            db,
            cancel,
            OfficialClient::new()?,
            OfficialStore::new(data_dir),
            time_zone,
        ))
    }

    /// For tests: local fake server, in-memory database, fake credential store.
    pub fn with_parts(
        data_dir: Option<std::path::PathBuf>,
        db: Database,
        cancel: Arc<AtomicBool>,
        client: OfficialClient,
        store: OfficialStore,
        time_zone: String,
    ) -> Self {
        Self {
            db: Mutex::new(db),
            data_dir,
            cancel,
            client,
            store,
            time_zone,
        }
    }

    fn abort_if_cancelled(&self) -> Result<()> {
        if self.cancel.load(Ordering::SeqCst) {
            return Err(ZeppBridgeError::Cancelled);
        }
        Ok(())
    }

    async fn lock(&self, sync_lease: bool) -> Result<Option<ExclusiveWriteGuard>> {
        let Some(dir) = self.data_dir.as_ref() else {
            return Ok(None);
        };
        let acquire = if sync_lease {
            write_lock::acquire_sync_lease
        } else {
            write_lock::acquire_with_timeout
        };
        let deadline = Instant::now() + WRITE_LOCK_TIMEOUT;
        loop {
            match acquire(dir, WritePurpose::Sync, std::time::Duration::ZERO) {
                Ok(guard) => return Ok(Some(guard)),
                Err(write_lock::WriteLockError::Busy { .. }) if Instant::now() < deadline => {
                    self.abort_if_cancelled()?;
                    tokio::time::sleep(LOCK_POLL).await;
                }
                Err(error @ write_lock::WriteLockError::Busy { .. }) => {
                    return Err(ZeppBridgeError::Busy(error.to_string()))
                }
                Err(error) => return Err(ZeppBridgeError::ConfigError(error.to_string())),
            }
        }
    }

    async fn write_db(&self) -> Result<WriteSession<'_>> {
        let lock = self.lock(false).await?;
        Ok(WriteSession {
            db: self.db.lock().await,
            _lock: lock,
        })
    }

    /// The data endpoint rejected the token: force one refresh. Only a rejected refresh means real re-authorization.
    /// Shares the single-flight refresh and compare-then-write-back with the status read.
    async fn recover(&self, tokens: &OfficialTokens) -> Result<OfficialTokens> {
        refresh_tokens(&self.store, &self.client, tokens, Utc::now().timestamp()).await
    }

    /// Sync the last `days` days. `mode` decides what to fetch (see the file header).
    ///
    /// With both sides connected this is only a supplement. Its failure does not change the sync state of
    /// the legacy streams and is only logged. Legacy sleep still shows; the official copy is added next time.
    pub async fn run(
        &self,
        days: i64,
        mode: OfficialMode,
        on_progress: &(dyn Fn(SyncProgress) + Send + Sync),
    ) -> Result<SyncReport> {
        self.cancel.store(false, Ordering::SeqCst);
        // Supplement mode also holds the sync lease. Without it, it could write the same streams as a CLI
        // sync or a just-queued backfill. The legacy round released its lease before this.
        let _lease = self.lock(true).await?;
        // A supplement sync only adds data. The per-stream state on the "Data health" page belongs to the legacy round (see
        // `Database::quiet_sync_state`). This connection is only for this supplement.
        if mode == OfficialMode::Supplement {
            self.db.lock().await.keep_sync_state_quiet();
        }
        let Some(mut tokens) =
            fresh_tokens(&self.store, &self.client, Utc::now().timestamp()).await?
        else {
            return Err(ZeppBridgeError::NeedsReauth(
                "Zepp official authorization is not connected yet".into(),
            ));
        };
        // If the database owner is not this official account, write nothing. In supplement mode the caller only logs it,
        // and the legacy round continues.
        self.write_db()
            .await?
            .claim_library_for_sync(&tokens.user_id)?;
        let today = crate::official::fetch::local_today(&self.time_zone);
        let start = today - Duration::days(days.clamp(1, UserPrefs::MAX_HISTORY_SYNC_DAYS) - 1);
        let kinds = mode.kinds();
        let total = kinds.len() as u32 + u32::from(mode == OfficialMode::Only);
        let mut streams: Vec<StreamReport> = Vec::new();
        for (index, kind) in kinds.iter().copied().enumerate() {
            self.abort_if_cancelled()?;
            let current = index as u32 + 1;
            let stream = kind.stream();
            on_progress(progress(stream, current, total, "syncing", false));
            let outcome = self
                .sync_kind(
                    &mut tokens,
                    kind,
                    start,
                    today,
                    stream,
                    current,
                    total,
                    on_progress,
                )
                .await;
            match outcome {
                Ok(report) => {
                    if mode == OfficialMode::Only {
                        streams.push(report);
                    }
                }
                Err(error) if error.is_cancelled() || error.needs_reauth() => return Err(error),
                Err(error) if mode == OfficialMode::Supplement => {
                    tracing::warn!("Official {} supplement sync failed: {error}", kind.label());
                }
                Err(error) => {
                    let unavailable = error.is_unavailable();
                    let report = StreamReport {
                        stream: stream.into(),
                        status: if unavailable {
                            StreamStatus::Unavailable
                        } else {
                            StreamStatus::Failed
                        },
                        records_written: 0,
                        raw_records: 0,
                        capability: if unavailable {
                            CapabilityStatus::Unavailable
                        } else {
                            CapabilityStatus::Unverified
                        },
                        needs_reauth: false,
                        message: Some(error.user_message()),
                    };
                    let db = self.write_db().await?;
                    streams.push(SyncManager::finish_stream(&db, stream, &[report], None)?);
                }
            }
            on_progress(progress(stream, current, total, "stream_completed", true));
        }
        if mode == OfficialMode::Only {
            self.abort_if_cancelled()?;
            on_progress(progress("workout_detail", total, total, "syncing", false));
            match self.sync_details(&mut tokens, false, DETAIL_BATCH).await {
                Ok(Some(report)) => streams.push(report),
                Ok(None) => {}
                Err(error) if error.is_cancelled() || error.needs_reauth() => return Err(error),
                Err(error) => {
                    // The whole detail sync failed (reading the pending list or writing failed): put it in the result, not only the log.
                    tracing::warn!("Official workout detail sync failed: {error}");
                    let report = StreamReport {
                        stream: "workout_detail".into(),
                        status: StreamStatus::Failed,
                        records_written: 0,
                        raw_records: 0,
                        capability: CapabilityStatus::Unverified,
                        needs_reauth: false,
                        message: Some(error.user_message()),
                    };
                    let db = self.write_db().await?;
                    streams.push(SyncManager::finish_stream(
                        &db,
                        "workout_detail",
                        &[report],
                        None,
                    )?);
                }
            }
            on_progress(progress(
                "workout_detail",
                total,
                total,
                "stream_completed",
                true,
            ));
        }
        let failed: Vec<String> = streams
            .iter()
            .filter(|report| report.status == StreamStatus::Failed)
            .map(|report| report.stream.clone())
            .collect();
        let core_ok = !failed.iter().any(|stream| is_core_stream(stream));
        // With official only, this is the whole sync round: full window and no failure means the next day's scheduled syncs fetch only recent days (like legacy).
        if mode == OfficialMode::Only
            && failed.is_empty()
            && days >= crate::contract::INCREMENTAL_SYNC_DAYS
        {
            if let Ok(db) = self.write_db().await {
                let _ = db.record_full_window_refresh(Utc::now());
            }
        }
        Ok(SyncReport {
            cleanup_failed: false,
            success: failed.is_empty(),
            core_ok,
            records_written: streams.iter().map(|report| report.records_written).sum(),
            message: (!failed.is_empty()).then(|| format!("These data streams failed: {}", failed.join(", "))),
            streams,
        })
    }

    /// One data type: fetch chunks newest to oldest, and notify the UI after each chunk is stored.
    #[allow(clippy::too_many_arguments)]
    async fn sync_kind(
        &self,
        tokens: &mut OfficialTokens,
        kind: OfficialKind,
        start: NaiveDate,
        end: NaiveDate,
        stream: &str,
        current: u32,
        total: u32,
        on_progress: &(dyn Fn(SyncProgress) + Send + Sync),
    ) -> Result<StreamReport> {
        let mut reports = Vec::new();
        let mut gaps: Vec<String> = Vec::new();
        for (chunk_start, chunk_end) in date_chunks_newest_first(start, end, kind.chunk_days()) {
            self.abort_if_cancelled()?;
            let outcome = match self.fetch(tokens, kind, chunk_start, chunk_end).await {
                Err(error) if error.needs_reauth() => {
                    *tokens = self.recover(tokens).await?;
                    self.fetch(tokens, kind, chunk_start, chunk_end).await?
                }
                other => other?,
            };
            // An incomplete chunk: store what arrived, record the gap, and mark the stream failed and retryable.
            if let Some(gap) = outcome.gap {
                tracing::warn!("{gap}");
                gaps.push(gap);
            }
            let db = self.write_db().await?;
            for raw in outcome.records {
                let record = FetchedRecord {
                    raw,
                    incomplete: false,
                    incomplete_reason: None,
                };
                reports.push(SyncManager::persist_record(&db, record)?.report);
            }
            drop(db);
            on_progress(progress(
                stream,
                current,
                total,
                "stream_chunk_committed",
                true,
            ));
        }
        let db = self.write_db().await?;
        let incomplete = (!gaps.is_empty()).then(|| gaps.join("; "));
        if reports.is_empty() {
            // Every chunk is empty (a valid `items: []`): official really has none of this type for the period. It is not a failure.
            // When there are gaps, finish_stream changes it to a failure.
            let empty = StreamReport {
                stream: stream.into(),
                status: StreamStatus::Success,
                records_written: 0,
                raw_records: 0,
                capability: CapabilityStatus::Verified,
                needs_reauth: false,
                message: None,
            };
            return SyncManager::finish_stream(&db, stream, &[empty], incomplete);
        }
        SyncManager::finish_stream(&db, stream, &reports, incomplete)
    }

    async fn fetch(
        &self,
        tokens: &OfficialTokens,
        kind: OfficialKind,
        start: NaiveDate,
        end: NaiveDate,
    ) -> Result<ChunkOutcome> {
        let fetcher = OfficialFetcher {
            client: &self.client,
            access_token: &tokens.access_token,
            time_zone: self.time_zone.clone(),
        };
        fetcher.fetch_chunk(kind, start, end).await
    }

    /// Fetch details for official workouts that have no per-second series yet. Returns `None` when nothing is pending.
    pub async fn history_detail_backfill(
        &self,
        limit: usize,
        retry_failed: bool,
    ) -> Result<crate::storage::detail_queue::DetailCoverage> {
        let _lease = self.lock(true).await?;
        self.abort_if_cancelled()?;
        if crate::storage::compaction_in_progress() || crate::storage::replay_in_progress() {
            return Err(ZeppBridgeError::Busy("Local maintenance is in progress".into()));
        }
        let Some(mut tokens) =
            fresh_tokens(&self.store, &self.client, Utc::now().timestamp()).await?
        else {
            return Err(ZeppBridgeError::NeedsReauth(
                "Zepp official authorization is not connected yet".into(),
            ));
        };
        {
            let db = self.write_db().await?;
            db.claim_library_for_sync(&tokens.user_id)?;
            if retry_failed {
                db.retry_failed_history_details_for_channel(true)?;
            }
        }
        self.sync_details(&mut tokens, true, limit.clamp(1, 40))
            .await?;
        self.db
            .lock()
            .await
            .workout_detail_coverage_for_channel(Some(true))
    }

    async fn sync_details(
        &self,
        tokens: &mut OfficialTokens,
        _history: bool,
        limit: usize,
    ) -> Result<Option<StreamReport>> {
        let pending = {
            let db = self.db.lock().await;
            db.pending_history_details(limit, true)?
                .into_iter()
                .map(|item| (item.workout_id, item.start_time))
                .collect::<Vec<_>>()
        };
        if pending.is_empty() {
            return Ok(None);
        }
        let mut reports = Vec::new();
        let mut failed = 0usize;
        let mut set_aside = 0usize;
        for (workout_id, start_time) in pending {
            self.abort_if_cancelled()?;
            if !self
                .db
                .lock()
                .await
                .workout_detail_request_ready(&workout_id)?
            {
                continue;
            }
            let day = DateTime::parse_from_rfc3339(&start_time)
                .map(|time| time.with_timezone(&Utc).date_naive())
                .unwrap_or_else(|_| Utc::now().date_naive());
            let fetcher = OfficialFetcher {
                client: &self.client,
                access_token: &tokens.access_token,
                time_zone: self.time_zone.clone(),
            };
            let mut attempt = fetcher.fetch_sport_detail(&workout_id, day).await;
            if matches!(&attempt, Err(error) if error.needs_reauth()) {
                // Token rejected: after the refresh, request this one again. Do not skip it.
                *tokens = self.recover(tokens).await?;
                let fetcher = OfficialFetcher {
                    client: &self.client,
                    access_token: &tokens.access_token,
                    time_zone: self.time_zone.clone(),
                };
                attempt = fetcher.fetch_sport_detail(&workout_id, day).await;
            }
            let raw = match attempt {
                Ok(raw) => raw,
                Err(error) if error.is_cancelled() || error.needs_reauth() => return Err(error),
                Err(error) => {
                    tracing::warn!("Official workout detail {workout_id} fetch failed: {error}");
                    // At the failure limit, or when official says there is none: set it aside and retry in 7 days,
                    // so one detail that never arrives does not mark every sync as failed.
                    let gone = error.is_unavailable();
                    let db = self.write_db().await?;
                    db.record_detail_request(
                        &workout_id,
                        super::details::detail_failure_status(&error),
                        Some(error.code()),
                    )?;
                    if db.record_official_detail_result(&workout_id, false, gone)? {
                        set_aside += 1;
                    } else {
                        failed += 1;
                    }
                    continue;
                }
            };
            let db = self.write_db().await?;
            let empty = crate::normalizer::payload_is_explicitly_empty(&raw.payload);
            let record = FetchedRecord {
                raw,
                incomplete: false,
                incomplete_reason: None,
            };
            let report = SyncManager::persist_record(&db, record)?.report;
            let parsed = report.status == StreamStatus::Success;
            db.record_official_detail_result(&workout_id, parsed, false)?;
            db.record_detail_request(
                &workout_id,
                if empty {
                    "empty_from_cloud"
                } else if parsed {
                    "persisted"
                } else {
                    "parse_failed"
                },
                None,
            )?;
            reports.push(report);
        }
        // Details that were not fetched must not be reported as "complete". The summary stays,
        // the stream is marked failed and says how many are missing. The next sync retries them (they stay on the pending list).
        let incomplete =
            (failed > 0).then(|| format!("{failed} official workout details were not fetched; the next sync retries them"));
        if set_aside > 0 {
            // A neutral note, not a failure: these are not available now and retry automatically in 7 days.
            reports.push(StreamReport {
                stream: "workout_detail".into(),
                status: StreamStatus::Unavailable,
                records_written: 0,
                raw_records: 0,
                capability: CapabilityStatus::Unavailable,
                needs_reauth: false,
                message: Some(format!(
                    "{set_aside} official workout details are not available now; automatic retry in 7 days"
                )),
            });
        }
        if reports.is_empty() && incomplete.is_none() {
            return Ok(None);
        }
        let db = self.write_db().await?;
        if reports.is_empty() {
            let empty = StreamReport {
                stream: "workout_detail".into(),
                status: StreamStatus::Success,
                records_written: 0,
                raw_records: 0,
                capability: CapabilityStatus::Verified,
                needs_reauth: false,
                message: None,
            };
            return SyncManager::finish_stream(&db, "workout_detail", &[empty], incomplete)
                .map(Some);
        }
        SyncManager::finish_stream(&db, "workout_detail", &reports, incomplete).map(Some)
    }
}

fn progress(stream: &str, current: u32, total: u32, code: &str, completed: bool) -> SyncProgress {
    SyncProgress {
        completed,
        stream: stream.into(),
        current,
        total,
        message: if completed {
            String::new()
        } else {
            format!("Syncing {stream} from Zepp official")
        },
        code: code.into(),
        detail: None,
    }
}

#[cfg(test)]
#[path = "official_tests.rs"]
mod tests;
