//! The workout detail queue (write each one as it arrives), storing fetched records, finishing a stream, and failure and unavailable reports.

use super::*;

impl SyncManager {
    /// Fetch pending run details and **write each one as it arrives**.
    ///
    /// Before, all details were fetched into one Vec and stored together: 40 details sat in memory, and when the user
    /// cancelled or the deadline hit, nothing downloaded was kept, and the next run started again.
    /// Returns a report for each one. The caller finishes the stream (summary, sync state).
    pub(super) async fn sync_pending_running_details(
        &self,
        deadline: Instant,
    ) -> Result<Vec<StreamReport>> {
        let pending = {
            let db = self.db.lock().await;
            db.pending_history_details(40, false)?
        };
        let queue_len = pending.len();
        let mut reports = Vec::new();
        let mut last_error = None;
        let mut attempted = 0usize;
        for item in pending {
            if !self.db.lock().await.detail_item_still_ready(&item)? {
                continue;
            }
            if self.cancel.load(Ordering::SeqCst) {
                return Err(ZeppBridgeError::Cancelled);
            }
            if Instant::now() >= deadline {
                break;
            }
            attempted += 1;
            match self
                .fetcher
                .fetch_sport_detail_record(&item.workout_id, &item.source, Utc::now(), None)
                .await
            {
                Ok(record) => {
                    let db = self.write_db().await?;
                    let empty = crate::normalizer::payload_is_explicitly_empty(&record.raw.payload);
                    let report = Self::persist_record(&db, record)?.report;
                    // Fetched but not decodable (quarantined) is not a success. Before, success was recorded before storage, so this detail
                    // no longer counted as pending, the next sync showed the stream green ("nothing pending"), and this
                    // run's track and samples were lost. Now the parse result is recorded, and the failure count backs off as usual.
                    let parsed = matches!(report.status, StreamStatus::Success);
                    db.record_workout_detail_fetch_result(&item.workout_id, &item.source, parsed)?;
                    if !item.supplement {
                        db.record_detail_request(
                            &item.workout_id,
                            if empty {
                                "empty_from_cloud"
                            } else if parsed {
                                "persisted"
                            } else {
                                "parse_failed"
                            },
                            None,
                        )?;
                    }
                    reports.push(report);
                }
                Err(error) if error.is_cancelled() => return Err(error),
                Err(error) if error.needs_reauth() => return Err(error),
                Err(error) => {
                    tracing::warn!("Failed to fetch workout detail {}: {}", item.workout_id, error);
                    if let Ok(db) = self.write_db().await {
                        if !item.supplement {
                            db.record_detail_request(
                                &item.workout_id,
                                super::details::detail_failure_status(&error),
                                Some(error.code()),
                            )?;
                        }
                        let _ = db.record_workout_detail_fetch_result(
                            &item.workout_id,
                            &item.source,
                            false,
                        );
                    }
                    last_error = Some(error);
                }
            }
        }
        // A deadline that stops this round is not a failure of the whole stream: the rest waits for next time. Raise last_error
        // only when the whole limited queue was tried and nothing arrived.
        let error = if reports.is_empty() && attempted >= queue_len {
            last_error
        } else {
            None
        };
        pending_details_outcome(reports, error)
    }

    /// Write sync_state even when no details are pending. Otherwise a failed state from the last round stays.
    /// `records_written` keeps the last count; this path writes nothing new.
    pub(super) async fn persist_empty_pending_details(&self) -> Result<StreamReport> {
        let previous = self.previous_records_written("workout_detail").await?;
        let still_pending = {
            let db = self.db.lock().await;
            !db.pending_history_details(40, false)?.is_empty()
        };
        let message = if still_pending {
            "Pending run details continue at the next sync"
        } else {
            "No pending run details"
        };
        let report = StreamReport {
            stream: "workout_detail".into(),
            status: StreamStatus::Success,
            records_written: 0,
            raw_records: 0,
            capability: CapabilityStatus::Verified,
            needs_reauth: false,
            message: Some(message.into()),
        };
        let db = self.write_db().await?;
        db.update_sync_state_details(
            "workout_detail",
            None,
            status_name(&report.status),
            report.message.as_deref(),
            report.needs_reauth,
            previous,
            report.capability.clone(),
            report.message.clone(),
        )?;
        Ok(report)
    }

    pub(super) async fn persist_records(
        &self,
        stream: &str,
        records: Vec<FetchedRecord>,
    ) -> Result<StreamReport> {
        self.persist_records_with(stream, records, false).await
    }

    /// Store data only, without the user-visible per-stream sync state (for history backfill, see `Database::quiet_sync_state`).
    pub(super) async fn persist_records_quietly(
        &self,
        stream: &str,
        records: Vec<FetchedRecord>,
    ) -> Result<StreamReport> {
        self.persist_records_with(stream, records, true).await
    }

    async fn persist_records_with(
        &self,
        stream: &str,
        records: Vec<FetchedRecord>,
        quiet: bool,
    ) -> Result<StreamReport> {
        let workout_evidence = if stream == "workouts" {
            crate::storage::training_coverage::workout_day_evidence(&records)
        } else {
            Vec::new()
        };
        let incomplete = records.iter().any(|record| record.incomplete);
        let reasons: std::collections::BTreeSet<_> = records
            .iter()
            .filter(|record| record.incomplete)
            .filter_map(|record| record.incomplete_reason.as_deref())
            .collect();
        let incomplete_message = if reasons.is_empty() {
            partial_window_reason().1
        } else {
            reasons.into_iter().collect::<Vec<_>>().join("; ")
        };
        // All payloads of one stream are stored in one write-lock section. Writing data already fetched takes milliseconds
        // to seconds, so other writers need not wait for the whole network phase.
        let db = self.write_db().await?;
        let _quiet = quiet.then(|| db.quiet_sync_state());
        let mut reports = Vec::with_capacity(records.len());
        for record in records {
            reports.push(Self::persist_record(&db, record)?.report);
        }
        if !incomplete
            && !reports
                .iter()
                .any(|r| matches!(r.status, StreamStatus::Failed | StreamStatus::Unverified))
        {
            db.record_workout_day_evidence(&workout_evidence)?;
        }
        let incomplete = incomplete.then_some(incomplete_message);
        Self::finish_stream(&db, stream, &reports, incomplete)
    }

    /// Finish a stream: merge the per-item reports into one, and record the phases and the sync state.
    ///
    /// When `incomplete` has a reason, the whole stream counts as incomplete: written data stays, but it must not report success.
    pub(super) fn finish_stream(
        db: &Database,
        stream: &str,
        reports: &[StreamReport],
        incomplete: Option<String>,
    ) -> Result<StreamReport> {
        let mut aggregate = aggregate_stream_reports(stream, reports);
        if let Some(incomplete_message) = incomplete {
            if aggregate.status != StreamStatus::Failed {
                aggregate.status = StreamStatus::Failed;
                aggregate.message = Some(incomplete_message.clone());
            }
            db.record_stream_stage(
                stream,
                Stage::Fetch,
                &StageOutcome::Failed {
                    kind: StageErrorKind::Unknown,
                    message: Some(incomplete_message),
                },
            )?;
        }
        db.record_stream_written(stream, aggregate.records_written)?;
        db.update_sync_state_details(
            stream,
            None,
            status_name(&aggregate.status),
            aggregate.message.as_deref(),
            aggregate.needs_reauth,
            aggregate.records_written,
            aggregate.capability.clone(),
            aggregate.message.clone(),
        )?;
        Ok(aggregate)
    }

    pub(super) fn persist_record(db: &Database, record: FetchedRecord) -> Result<PersistResult> {
        let stream = record.raw.stream.clone();
        let capability = record.raw.capability.clone();
        let mut report = StreamReport {
            stream: stream.clone(),
            status: StreamStatus::Success,
            records_written: 0,
            raw_records: 1,
            capability: capability.clone(),
            needs_reauth: false,
            message: None,
        };
        // The payload is already here, so the fetch phase succeeded whatever happens with parsing and writing.
        // Record the three phases separately so the UI can say "not fetched", "not understood" or "not written".
        db.record_stream_stage(&stream, Stage::Fetch, &StageOutcome::Ok)?;
        match db.persist_fetched_record(&record.raw) {
            Ok((_, value)) => {
                report.records_written = value.primary_records;
                db.record_stream_stage(&stream, Stage::Parse, &StageOutcome::Ok)?;
                db.record_stream_stage(&stream, Stage::Write, &StageOutcome::Ok)?;
            }
            // The cloud clearly returned an empty collection. Even if this stream's structure is not verified field by field, "empty" is not "not understood".
            // Charge also uses this path: a known "no score that day" is already Ok(empty). The remaining
            // DataUnavailable means the payload had content we did not understand. It must not silently become "the cloud has none" (#157).
            Err(error)
                if error.is_unavailable()
                    && (capability == CapabilityStatus::Unverified
                        || record.raw.source_key.starts_with("events:Charge:"))
                    && !crate::normalizer::payload_is_explicitly_empty(&record.raw.payload) =>
            {
                report.status = StreamStatus::Unverified;
                report.capability = CapabilityStatus::Unverified;
                report.message = Some(error.user_message());
                // A payload arrived, but the current normalizer does not know its structure. The raw payload is already stored,
                // and a normalization failure does not delete it. This is a parse-phase failure, not a network failure.
                db.record_stream_stage(
                    &stream,
                    Stage::Parse,
                    &StageOutcome::Failed {
                        kind: StageErrorKind::UnrecognizedPayload,
                        message: report.message.clone(),
                    },
                )?;
            }
            Err(error) if error.is_unavailable() => {
                report.status = StreamStatus::Unavailable;
                report.capability = CapabilityStatus::Unavailable;
                report.message = Some(error.user_message());
                db.record_stream_stage(
                    &stream,
                    Stage::Parse,
                    &StageOutcome::Failed {
                        kind: StageErrorKind::NotAvailable,
                        message: report.message.clone(),
                    },
                )?;
            }
            Err(error) => {
                report.status = StreamStatus::Failed;
                report.capability = CapabilityStatus::Unavailable;
                report.needs_reauth = error.needs_reauth();
                report.message = Some(error.user_message());
                let kind = StageErrorKind::classify(&error);
                let stage = if kind == StageErrorKind::Storage {
                    Stage::Write
                } else {
                    Stage::Parse
                };
                db.record_stream_stage(
                    &stream,
                    stage,
                    &StageOutcome::Failed {
                        kind,
                        message: report.message.clone(),
                    },
                )?;
            }
        }
        db.update_sync_state_details(
            &stream,
            None,
            status_name(&report.status),
            report.message.as_deref(),
            report.needs_reauth,
            report.records_written,
            report.capability.clone(),
            report.message.clone(),
        )?;
        Ok(PersistResult { report })
    }

    /// A stream that this round had no time for because the time budget ran out. It counts as failed (this stream really did not update, and
    /// the next scheduled sync uses the full window because of it), but the code is `err.core.timed_out`, not a configuration error,
    /// and the last written count stays.
    pub(super) async fn timed_out_report(&self, stream: &str) -> Result<StreamReport> {
        self.failure_report(
            stream,
            &ZeppBridgeError::TimedOut("This sync round ran out of time; this stream continues next time".into()),
        )
        .await
    }

    pub(super) async fn failure_report(
        &self,
        stream: &str,
        error: &ZeppBridgeError,
    ) -> Result<StreamReport> {
        let previous = self.previous_records_written(stream).await?;
        let report = StreamReport {
            stream: stream.into(),
            status: StreamStatus::Failed,
            records_written: previous,
            raw_records: 0,
            capability: CapabilityStatus::Unavailable,
            needs_reauth: error.needs_reauth(),
            message: Some(error.user_message()),
        };
        let db = self.write_db().await?;
        db.record_stream_stage(
            stream,
            Stage::Fetch,
            &StageOutcome::Failed {
                kind: StageErrorKind::classify(error),
                message: report.message.clone(),
            },
        )?;
        db.update_sync_state_details(
            stream,
            None,
            "failed",
            report.message.as_deref(),
            report.needs_reauth,
            previous,
            CapabilityStatus::Unavailable,
            report.message.clone(),
        )?;
        Ok(report)
    }

    pub(super) async fn heart_rate_fetch_error(
        &self,
        error: &ZeppBridgeError,
    ) -> Result<StreamReport> {
        // The independent HR endpoint can be absent while wellness supplies HR.
        // Keep that absence visible, but never turn network/auth failures neutral.
        if error.is_unavailable() {
            self.unavailable_report("heart_rate", error).await
        } else {
            self.failure_report("heart_rate", error).await
        }
    }

    pub(super) async fn unavailable_report(
        &self,
        stream: &str,
        error: &ZeppBridgeError,
    ) -> Result<StreamReport> {
        let previous = self.previous_records_written(stream).await?;
        let report = StreamReport {
            stream: stream.into(),
            status: StreamStatus::Unavailable,
            records_written: previous,
            raw_records: 0,
            capability: CapabilityStatus::Unavailable,
            needs_reauth: error.needs_reauth(),
            message: Some(error.user_message()),
        };
        let db = self.write_db().await?;
        db.update_sync_state_details(
            stream,
            None,
            "unavailable",
            report.message.as_deref(),
            report.needs_reauth,
            previous,
            CapabilityStatus::Unavailable,
            report.message.clone(),
        )?;
        Ok(report)
    }

    /// A failed or unavailable stream must not reset its persisted
    /// `records_written` counter; the UI reads that value as "N synced",
    /// so a transient failure would otherwise make the stored data look wiped.
    pub(super) async fn previous_records_written(&self, stream: &str) -> Result<i64> {
        let db = self.db.lock().await;
        Ok(db
            .get_sync_state(stream)?
            .map(|state| state.records_written)
            .unwrap_or(0))
    }
}
