//! Backfill history by month and record the results in the coverage ledger (split out of sync/mod.rs, same logic).

use super::*;

impl SyncManager {
    /// Full history backfill.
    ///
    /// It works in calendar-month chunks and records each chunk. So after an interruption it continues
    /// from the unfinished chunk, a repeat run does not write twice, and "the cloud returned nothing" and "we never asked" are two different ledger states.
    ///
    /// A backfill **does not clean up**. Deleting just-fetched history in the same round with retention
    /// destroys user trust. `backfill_would_be_cleaned_up` must stop the caller before it starts.
    pub async fn history_backfill<F>(
        &self,
        from: chrono::NaiveDate,
        to: chrono::NaiveDate,
        max_chunks: usize,
        on_progress: F,
    ) -> Result<CoverageLedger>
    where
        F: Fn(SyncProgress) + Send + Sync,
    {
        // See the same comment in `sync_report`: take `run_lock` first, then clear the cancel flag.
        // Otherwise it races with the "raise flag, then wait for lock" order of `cancel_and_wait()`.
        let _run_guard = self.run_lock.lock().await;
        self.cancel.store(false, Ordering::SeqCst);
        self.stand_aside_if_local_maintenance()?;
        // A backfill can take minutes. Hold only the sync lease for the whole run; take the write lock only to record and store.
        let _lease = self.begin_run(WritePurpose::HistoryBackfill).await?;
        self.abort_if_cancelled()?;

        {
            let db = self.write_db().await?;
            let prefs = db.user_prefs()?;
            let requested_days = (Utc::now().date_naive() - from).num_days().max(0);
            if prefs.backfill_would_be_cleaned_up(requested_days) {
                return Err(ZeppBridgeError::ConfigError(format!(
                    "This backfill fetches {requested_days} days of history, but this device keeps only the last {} days. The next successful sync would delete the data just fetched. Turn on \"Long-term archive\" first, or make the retention period longer.",
                    prefs.retention_days
                )));
            }
            db.plan_backfill(from, to)?;
        }

        let time_zone = {
            let db = self.db.lock().await;
            db.device_time_zone().unwrap_or(None)
        }
        .unwrap_or_else(crate::official::fetch::system_time_zone);

        // Get all chunks for this round **at once**, not "the current first chunk" from the database each time.
        //
        // The old code called `pending_backfill_chunks(1)` each time. A failed chunk was written back as
        // `failed`, which still counts as pending with the same sort key. So the next round
        // always picked the same chunk and used the whole max_chunks budget on it. Other streams of
        // the same month and all older months never got a turn. This was the cause of issue #10
        // ("after one heart rate chunk fails, the whole backfill stops").
        //
        // With all chunks taken at once, each (stream, month) is tried at most once per round. A failed chunk
        // waits for the next round (or an explicit user retry) and blocks nobody.
        let queue = {
            let db = self.db.lock().await;
            db.pending_backfill_chunks(max_chunks)?
        };
        let total = {
            let db = self.db.lock().await;
            db.coverage_ledger()?.total_chunks.max(1) as u32
        };

        let mut processed = 0usize;
        for chunk in queue {
            if self.cancel.load(Ordering::SeqCst) {
                break;
            }
            processed += 1;

            on_progress(SyncProgress {
                completed: false,
                stream: chunk.stream.clone(),
                current: processed as u32,
                total,
                message: format!(
                    "Backfilling {} · {}",
                    chunk.stream,
                    chunk_month_label(&chunk.chunk_start)
                ),
                code: "backfilling".into(),
                detail: Some(chunk_month_label(&chunk.chunk_start).to_string()),
            });

            let outcome = self.backfill_one_chunk(&chunk, &time_zone).await;
            let db = self.write_db().await?;
            match outcome {
                Ok((status, records, reason)) => db.record_backfill_chunk(
                    &chunk.stream,
                    &chunk.chunk_start,
                    status,
                    records,
                    reason.as_ref().map(|(_, text)| text.as_str()),
                    reason.as_ref().map(|(code, _)| *code),
                )?,
                Err(error) if error.is_cancelled() => break,
                // An expired session makes every remaining chunk in the queue fail. Continuing only uses
                // the whole budget on requests that must fail, and it hides the sign-in signal.
                // Unlike `sync_report`, this code did not pass needs_reauth up before.
                Err(error) if error.needs_reauth() => return Err(error),
                Err(error) => db.record_backfill_chunk(
                    &chunk.stream,
                    &chunk.chunk_start,
                    ChunkStatus::Failed,
                    0,
                    Some(&error.user_message()),
                    Some(error.code()),
                )?,
            }
            drop(db);
            on_progress(SyncProgress {
                completed: true,
                stream: chunk.stream.clone(),
                current: processed as u32,
                total,
                message: String::new(),
                code: "stream_completed".into(),
                detail: Some(chunk_month_label(&chunk.chunk_start).to_string()),
            });
        }

        let db = self.db.lock().await;
        db.coverage_ledger()
    }

    /// Fetch and write one chunk. Returns the chunk result, the written count and, on failure, the reason.
    ///
    /// The reason goes into the ledger. If the UI only shows "N chunks failed" without a reason, the user
    /// cannot decide whether to retry and cannot report useful information.
    pub(super) async fn backfill_one_chunk(
        &self,
        chunk: &CoverageChunk,
        time_zone: &str,
    ) -> Result<(ChunkStatus, i64, Option<(&'static str, String)>)> {
        let start = chrono::NaiveDate::parse_from_str(&chunk.chunk_start, "%Y-%m-%d")
            .map_err(|_| ZeppBridgeError::ParseError("Invalid date in the coverage ledger".into()))?;
        let end = chrono::NaiveDate::parse_from_str(&chunk.chunk_end, "%Y-%m-%d")
            .map_err(|_| ZeppBridgeError::ParseError("Invalid date in the coverage ledger".into()))?;
        // The current month's chunk ends in the future. Zepp's per-day event endpoint returns HTTP 400 for future days,
        // so the whole chunk would be "partially written" and a retry would not fix it. Ask only up to now.
        let window_end = coverage::to_utc(end).min(Utc::now());
        if window_end <= coverage::to_utc(start) {
            return Ok((ChunkStatus::EmptyFromCloud, 0, None));
        }
        let window = FetchWindow::between(coverage::to_utc(start), window_end)?;

        let records = match chunk.stream.as_str() {
            "heart_rate" => self.fetcher.fetch_heart_rate_records(window).await,
            "daily_summary" => self.fetcher.fetch_daily_statistics_records(window).await,
            "workouts" => self.fetcher.fetch_workout_records(window).await,
            "sleep" => self.fetcher.fetch_sleep_records(window).await,
            "hrv" => self.fetcher.fetch_hrv_records(window).await,
            "wellness" => self.fetcher.fetch_wellness_records(window, time_zone).await,
            "weight" => self.fetcher.fetch_weight_records(window).await,
            other => {
                return Err(ZeppBridgeError::ConfigError(format!(
                    "Unknown backfill data stream: {other}"
                )))
            }
        };

        match records {
            Ok(records) if records.is_empty() => {
                // The request was made and the cloud clearly has no data for this period. This is not a failure and needs no retry.
                Ok((ChunkStatus::EmptyFromCloud, 0, None))
            }
            Ok(records) => {
                let incomplete = records.iter().any(|record| record.incomplete);
                // The backfill result goes into the coverage ledger, not into the per-stream rows of the "Data health" page.
                let report = self.persist_records_quietly(&chunk.stream, records).await?;
                Ok(classify_backfill_report(&report, incomplete))
            }
            Err(error) if error.is_cancelled() || error.needs_reauth() => Err(error),
            Err(error) if error.is_unavailable() => Ok((ChunkStatus::EmptyFromCloud, 0, None)),
            Err(error) => Err(error),
        }
    }
}
