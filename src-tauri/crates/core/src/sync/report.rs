//! One sync round: fetch each stream, store it, and build a report (split out of sync/mod.rs, same logic).

use super::*;

impl SyncManager {
    pub(super) async fn sync_report(
        &self,
        days: i64,
        on_progress: Option<&(dyn Fn(SyncProgress) + Send + Sync)>,
    ) -> Result<SyncReport> {
        // Clear the cancel flag only after `run_lock` is held. `cancel_and_wait()` raises
        // the flag first and then waits for this lock. If the flag is cleared before the lock,
        // a concurrent call erases the new cancel request, and the caller thinks it worked.
        let _run_guard = self.run_lock.lock().await;
        self.cancel.store(false, Ordering::SeqCst);
        // Replay/compaction raises the flag before it takes the write lock. Check the flag first,
        // not after a 20-second wait for Busy, so the caller can mark this sync deferred and retry.
        self.stand_aside_if_local_maintenance()?;
        // The in-process run_lock cannot stop a second process. The CLI `sync` and the desktop app
        // can run at the same time, so hold the sync lease for the whole run. Take the write lock
        // only for the real database writes (`write_db`), so other writers can write during network calls.
        let _lease = self.begin_run(WritePurpose::Sync).await?;
        self.abort_if_cancelled()?;
        let window = FetchWindow::days(days)?;
        let mut streams = Vec::new();
        let started = Instant::now();
        // The old two-part formula was not continuous at days=7/8: the 8-14 day budget (45+3*days)
        // was shorter than the fixed 90 seconds for 7 days or less. Use one monotonic formula,
        // and `.clamp(90, ..)` makes sure every day count gets at least 90 seconds.
        let budget = 45u64
            .saturating_add((days.max(0) as u64).saturating_mul(3))
            .clamp(90, 20 * 60);
        let deadline = started + std::time::Duration::from_secs(budget);

        let emit = |stream: &str, current: u32, total: u32, message: &str| {
            if let Some(callback) = on_progress {
                callback(SyncProgress {
                    completed: false,
                    stream: stream.into(),
                    current,
                    total,
                    message: message.into(),
                    code: "syncing".into(),
                    detail: None,
                });
            }
        };

        let completed = |stream: &str, current: u32| {
            if let Some(callback) = on_progress {
                callback(SyncProgress {
                    completed: true,
                    stream: stream.into(),
                    current,
                    total: 8,
                    message: String::new(),
                    code: "stream_completed".into(),
                    detail: None,
                });
            }
        };

        // Tell the UI to read after each chunk is stored. The frontend treats an event with
        // `completed` true as "this stream has new data". The end of the whole stream is still `stream_completed`.
        let chunk_committed = |stream: &str, current: u32| {
            if let Some(callback) = on_progress {
                callback(SyncProgress {
                    completed: true,
                    stream: stream.into(),
                    current,
                    total: 8,
                    message: String::new(),
                    code: "stream_chunk_committed".into(),
                    detail: None,
                });
            }
        };

        // A used-up time budget is not an error. Data already written still counts. The remaining
        // streams this round are marked "timed out, continue next time" (`timed_out_report`).
        // The report is still returned and the sync time is still recorded.
        let past_deadline = || Instant::now() > deadline;

        emit("heart_rate", 1, 8, "Syncing heart rate");
        self.abort_if_cancelled()?;
        if past_deadline() {
            streams.push(self.timed_out_report("heart_rate").await?);
        } else {
            match self
                .sync_chunked_stream(
                    "heart_rate",
                    window,
                    7,
                    OnChunkError::StopUnlessUnavailable,
                    |chunk| self.fetcher.fetch_heart_rate_records(chunk),
                    || chunk_committed("heart_rate", 1),
                )
                .await
            {
                Ok(report) => streams.push(report),
                Err(error) if error.is_cancelled() => return Err(error),
                Err(error) => streams.push(self.heart_rate_fetch_error(&error).await?),
            }
        }
        completed("heart_rate", 1);
        emit("daily_summary", 2, 8, "Syncing daily summary");
        self.abort_if_cancelled()?;
        if past_deadline() {
            streams.push(self.timed_out_report("daily_summary").await?);
        } else {
            match self
                .sync_chunked_stream(
                    "daily_summary",
                    window,
                    30,
                    OnChunkError::Continue,
                    |chunk| self.fetcher.fetch_daily_statistics_records(chunk),
                    || chunk_committed("daily_summary", 2),
                )
                .await
            {
                Ok(report) => streams.push(report),
                Err(error) if error.is_cancelled() => return Err(error),
                Err(error) => streams.push(self.failure_report("daily_summary", &error).await?),
            }
        }
        completed("daily_summary", 2);
        // Optional streams are retained and reported, never promoted to a
        // verified empty success.
        emit("sleep", 3, 8, "Syncing sleep");
        self.abort_if_cancelled()?;
        if past_deadline() {
            streams.push(self.timed_out_report("sleep").await?);
        } else {
            match self
                .sync_chunked_stream(
                    "sleep",
                    window,
                    7,
                    OnChunkError::Continue,
                    |chunk| self.fetcher.fetch_sleep_records(chunk),
                    || chunk_committed("sleep", 3),
                )
                .await
            {
                Ok(report) => streams.push(report),
                Err(error) if error.is_cancelled() => return Err(error),
                Err(error) if error.is_unavailable() => {
                    streams.push(self.unavailable_report("sleep", &error).await?)
                }
                Err(error) => streams.push(self.failure_report("sleep", &error).await?),
            }
        }
        completed("sleep", 3);
        emit("workouts", 4, 8, "Syncing workouts");
        self.abort_if_cancelled()?;
        if past_deadline() {
            streams.push(self.timed_out_report("workouts").await?);
        } else {
            match self.fetcher.fetch_workout_records(window).await {
                Ok(records) => streams.push(self.persist_records("workouts", records).await?),
                Err(error) if error.is_cancelled() => return Err(error),
                Err(error) if error.is_unavailable() => {
                    streams.push(self.unavailable_report("workouts", &error).await?)
                }
                Err(error) => streams.push(self.failure_report("workouts", &error).await?),
            }
        }
        completed("workouts", 4);
        emit("workout_detail", 5, 8, "Syncing run details");
        self.abort_if_cancelled()?;
        if past_deadline() {
            streams.push(self.timed_out_report("workout_detail").await?);
        } else {
            match self.sync_pending_running_details(deadline).await {
                Ok(reports) if reports.is_empty() => {
                    streams.push(self.persist_empty_pending_details().await?);
                }
                Ok(reports) => {
                    let db = self.write_db().await?;
                    streams.push(Self::finish_stream(&db, "workout_detail", &reports, None)?);
                }
                Err(error) if error.is_cancelled() => return Err(error),
                Err(error) if error.is_unavailable() => {
                    streams.push(self.unavailable_report("workout_detail", &error).await?)
                }
                Err(error) => streams.push(self.failure_report("workout_detail", &error).await?),
            }
        }

        completed("workout_detail", 5);
        emit("hrv", 6, 8, "Syncing heart rate variability");
        self.abort_if_cancelled()?;
        if past_deadline() {
            streams.push(self.timed_out_report("hrv").await?);
        } else {
            match self.fetcher.fetch_hrv_records(window).await {
                Ok(records) => streams.push(self.persist_records("hrv", records).await?),
                Err(error) if error.is_cancelled() => return Err(error),
                Err(error) if error.is_unavailable() => {
                    streams.push(self.unavailable_report("hrv", &error).await?)
                }
                Err(error) => streams.push(self.failure_report("hrv", &error).await?),
            }
        }
        completed("hrv", 6);
        emit("wellness", 7, 8, "Syncing stress, SpO2 and other optional metrics");
        self.abort_if_cancelled()?;
        if past_deadline() {
            streams.push(self.timed_out_report("wellness").await?);
        } else {
            // The dateString surface needs an IANA zone name, and the devices
            // already told us theirs.
            let wellness_time_zone = {
                let database = self.db.lock().await;
                database.device_time_zone().unwrap_or(None)
            }
            .unwrap_or_else(crate::official::fetch::system_time_zone);
            match self
                .fetcher
                .fetch_wellness_records(window, &wellness_time_zone)
                .await
            {
                Ok(records) => streams.push(self.persist_records("wellness", records).await?),
                Err(error) if error.is_cancelled() => return Err(error),
                Err(error) if error.is_unavailable() => {
                    streams.push(self.unavailable_report("wellness", &error).await?)
                }
                Err(error) => streams.push(self.failure_report("wellness", &error).await?),
            }
        }

        // Weight and body composition. The capability probe used
        // `/v2/users/me/events?eventType=weight`, which is empty for every account.
        // The real data is at `/users/{id}/members/-1/weightRecords`.
        //
        // Accounts without a scale also have records here (weight typed by hand in the Zepp App
        // uses this path too), so this stream is not only useful with a scale.
        completed("wellness", 7);
        emit("weight", 8, 8, "Syncing weight and body composition");
        self.abort_if_cancelled()?;
        if past_deadline() {
            streams.push(self.timed_out_report("weight").await?);
        } else {
            match self.fetcher.fetch_weight_records(window).await {
                Ok(records) => streams.push(self.persist_records("weight", records).await?),
                Err(error) if error.is_cancelled() => return Err(error),
                Err(error) if error.is_unavailable() => {
                    streams.push(self.unavailable_report("weight", &error).await?)
                }
                Err(error) => streams.push(self.failure_report("weight", &error).await?),
            }
        }

        completed("weight", 8);

        // Learned quietly, alongside everything else the sync brings back. A
        // failure here must not colour the sync outcome: it is a convenience,
        // not one of the data streams.
        if let Err(error) = self.refresh_capabilities_if_stale(7).await {
            if error.is_cancelled() {
                return Err(error);
            }
        }

        // The streams that really failed. `Unavailable` / `Unverified` are not in this list:
        // they mean "this device does not have this capability", not "the fetch failed".
        let failed: Vec<String> = streams
            .iter()
            .filter(|report| report.status == StreamStatus::Failed)
            .map(|report| report.stream.clone())
            .collect();
        let core_failed = failed.iter().any(|stream| is_core_stream(stream));
        let core_ok = !core_failed;
        // A failed secondary stream is also a failure. Before, only the core streams counted,
        // so a failed sleep fetch still showed "updated".
        let success = failed.is_empty();
        let total_written = streams.iter().map(|report| report.records_written).sum();
        // Retention runs only when no stream is Failed. If it ran while a secondary stream
        // failed, it could delete the days that just failed and are not filled yet.
        // Unavailable / Unverified are not Failed, so cleanup is still allowed.
        //
        // A cleanup failure must not replace the successful sync report with `?`: the data is written.
        let mut cleanup_warning = None;
        if success {
            let prefs = self.db.lock().await.user_prefs()?;
            // With long-term archive on, do not clean up automatically. Deleting history that was
            // just backfilled after the next sync destroys user trust.
            if !prefs.archive_enabled {
                let cleaned = match self.write_db().await {
                    Ok(db) => db.cleanup_old_data(prefs.retention_days),
                    Err(error) => Err(error),
                };
                if let Err(error) = cleaned {
                    tracing::warn!("Failed to clean up old data after sync: {error}");
                    cleanup_warning = Some(format!(
                        "Data synced; failed to clean up old data: {}",
                        error.user_message()
                    ));
                }
            }
        }
        // Old heart rate pages / workout history payloads that newer payloads replaced and no row
        // points to (see `prune_superseded_page_raws`). The archive switch does not apply: this deletes nobody's data.
        // A failure is only logged. The next sync or startup compaction tries again.
        if let Ok(db) = self.write_db().await {
            if let Err(error) = db.prune_superseded_page_raws() {
                tracing::warn!("Failed to clean up superseded old page payloads: {error}");
            }
        }
        // Full window and no failed stream: record it, so scheduled syncs in the next day fetch only recent days.
        // If a stream failed, do not record it. The next scheduled sync uses the full window to fill the gap.
        if success && days >= crate::contract::INCREMENTAL_SYNC_DAYS {
            if let Ok(db) = self.write_db().await {
                let _ = db.record_full_window_refresh(Utc::now());
            }
        }
        let cleanup_failed = cleanup_warning.is_some();
        Ok(SyncReport {
            success,
            core_ok,
            streams,
            records_written: total_written,
            cleanup_failed,
            message: if core_failed {
                Some("At least one core data stream failed; the sync did not report success".into())
            } else if !failed.is_empty() {
                // Name the failed streams. "Partial failure" alone does not tell the user what is missing.
                Some(format!("These data streams failed: {}", failed.join(", ")))
            } else {
                cleanup_warning
            },
        })
    }
}

pub(super) fn status_name(status: &StreamStatus) -> &'static str {
    match status {
        StreamStatus::Success => "success",
        StreamStatus::Failed => "failed",
        StreamStatus::Unavailable => "unavailable",
        StreamStatus::Unverified => "unverified",
    }
}

pub(super) fn chunk_month_label(chunk_start: &str) -> &str {
    chunk_start.get(..7).unwrap_or(chunk_start)
}

/// The final result for pending details: tried but got nothing means failure.
/// Only no pending details at all (`last_error` is None) is an empty success.
pub(super) fn pending_details_outcome<T>(
    records: Vec<T>,
    last_error: Option<ZeppBridgeError>,
) -> Result<Vec<T>> {
    if records.is_empty() {
        if let Some(error) = last_error {
            return Err(error);
        }
    }
    Ok(records)
}

/// Merge the reports of many payloads on one stream into one report.
///
/// Ranking: if any report is `Failed`, the aggregate is `Failed`. Usable records written
/// at the same time do not raise it back to `Success`. Only with no Failed report can
/// "some successful writes" turn Unverified / Unavailable into Success.
pub(super) fn aggregate_stream_reports(stream: &str, reports: &[StreamReport]) -> StreamReport {
    let mut aggregate = StreamReport {
        stream: stream.into(),
        status: StreamStatus::Success,
        records_written: 0,
        raw_records: 0,
        capability: CapabilityStatus::Verified,
        needs_reauth: false,
        message: None,
    };
    let mut successes = 0usize;
    let mut notices = 0usize;
    let mut last_failed: Option<&StreamReport> = None;
    let mut last_notice: Option<&StreamReport> = None;

    for one in reports {
        aggregate.records_written += one.records_written;
        aggregate.raw_records += one.raw_records;
        aggregate.needs_reauth |= one.needs_reauth;
        match one.status {
            StreamStatus::Success => successes += 1,
            StreamStatus::Failed => {
                notices += 1;
                last_failed = Some(one);
            }
            StreamStatus::Unavailable | StreamStatus::Unverified => {
                notices += 1;
                last_notice = Some(one);
            }
        }
    }

    if let Some(failed_one) = last_failed {
        aggregate.status = StreamStatus::Failed;
        aggregate.capability = failed_one.capability.clone();
        aggregate.message = if successes > 0 && aggregate.records_written > 0 {
            Some(format!(
                "Usable data parsed; {notices} optional responses had no recognizable records"
            ))
        } else {
            failed_one.message.clone()
        };
        return aggregate;
    }

    if successes > 0 && aggregate.records_written > 0 {
        aggregate.status = StreamStatus::Success;
        aggregate.capability = CapabilityStatus::Verified;
        aggregate.needs_reauth = false;
        aggregate.message =
            (notices > 0).then(|| format!("Usable data parsed; {notices} optional responses had no recognizable records"));
        return aggregate;
    }

    if let Some(notice) = last_notice {
        aggregate.status = notice.status;
        aggregate.capability = notice.capability.clone();
        aggregate.message = notice.message.clone();
    }
    aggregate
}

pub(super) fn partial_window_reason() -> (&'static str, String) {
    (
        "err.backfill.partial_window",
        "Only part of this chunk was written; it needs a retry".to_string(),
    )
}

pub(super) fn no_canonical_reason() -> (&'static str, String) {
    (
        "err.backfill.no_canonical_records",
        "The cloud returned a payload, but no usable records were parsed".to_string(),
    )
}

/// The ledger result after one backfill chunk is written.
///
/// Rows already in the database stay. A sub-slice 404, a parse failure or a Failed aggregate
/// must not mark this month as a complete copy. `Unverified` also does not mean "the cloud has none".
pub(super) fn classify_backfill_report(
    report: &StreamReport,
    incomplete: bool,
) -> (ChunkStatus, i64, Option<(&'static str, String)>) {
    let written = report.records_written;
    if incomplete {
        let reason = (
            partial_window_reason().0,
            report
                .message
                .clone()
                .unwrap_or_else(|| partial_window_reason().1),
        );
        let status = if written > 0 {
            ChunkStatus::Partial
        } else {
            ChunkStatus::Failed
        };
        return (status, written, Some(reason));
    }
    match report.status {
        StreamStatus::Success if written > 0 => (ChunkStatus::Persisted, written, None),
        StreamStatus::Success | StreamStatus::Unavailable => (ChunkStatus::EmptyFromCloud, 0, None),
        StreamStatus::Unverified if written > 0 => {
            (ChunkStatus::Partial, written, Some(partial_window_reason()))
        }
        StreamStatus::Unverified => (ChunkStatus::Failed, 0, Some(no_canonical_reason())),
        StreamStatus::Failed if written > 0 => {
            (ChunkStatus::Partial, written, Some(partial_window_reason()))
        }
        StreamStatus::Failed => (ChunkStatus::Failed, 0, Some(no_canonical_reason())),
    }
}
