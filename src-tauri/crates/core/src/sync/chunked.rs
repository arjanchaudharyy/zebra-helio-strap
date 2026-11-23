//! Sync one stream in time chunks: fetch from the newest chunk backward, and **store each chunk and notify the UI as it arrives**.
//!
//! Before, heart rate, daily summary and sleep fetched the whole time window before they wrote and sent `stream_completed`.
//! So the home page waited for the last chunk, and the newest data was in that last chunk. Now chunks run newest
//! to oldest with a callback after each stored chunk, so the home page shows the newest heart rate, today's steps and last night's sleep
//! after the first chunk.
//!
//! The failure rules match the old slice merge (`fetcher::pages::conclude_slices`): cancel and re-login
//! return at once; if no chunk arrived, the error goes back to the caller; if some arrived, the stream counts as
//! incomplete (written data stays, but it does not report success).

use super::*;
use std::future::Future;

/// Whether to keep fetching older chunks after one chunk fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum OnChunkError {
    /// Keep fetching older chunks (sleep, daily summary: one bad chunk must not affect other nights or days).
    Continue,
    /// Stop on any error other than "this chunk is unavailable" (heart rate: a network error always stopped at once,
    /// so it does not retry every chunk while offline).
    StopUnlessUnavailable,
}

impl SyncManager {
    pub(super) async fn sync_chunked_stream<F, Fut>(
        &self,
        stream: &str,
        window: FetchWindow,
        chunk_days: i64,
        on_error: OnChunkError,
        mut fetch: F,
        mut on_commit: impl FnMut(),
    ) -> Result<StreamReport>
    where
        F: FnMut(FetchWindow) -> Fut,
        Fut: Future<Output = Result<Vec<FetchedRecord>>>,
    {
        let mut reports = Vec::new();
        let mut reasons = std::collections::BTreeSet::new();
        let mut last_error: Option<ZeppBridgeError> = None;
        for chunk in window.chunks_newest_first(chunk_days) {
            self.abort_if_cancelled()?;
            match fetch(chunk).await {
                Ok(records) => {
                    for record in records.iter().filter(|record| record.incomplete) {
                        reasons.insert(
                            record
                                .incomplete_reason
                                .clone()
                                .unwrap_or_else(|| partial_window_reason().1),
                        );
                    }
                    let db = self.write_db().await?;
                    for record in records {
                        reports.push(Self::persist_record(&db, record)?.report);
                    }
                    drop(db);
                    on_commit();
                }
                Err(error) if error.is_cancelled() || error.needs_reauth() => return Err(error),
                Err(error) => {
                    reasons.insert(format!(
                        "{stream} ({}..{}): {}",
                        chunk.start_day(),
                        chunk.end_day(),
                        error.user_message()
                    ));
                    let stop =
                        on_error == OnChunkError::StopUnlessUnavailable && !error.is_unavailable();
                    // Do not let a later chunk's 404 downgrade an earlier real request failure to "unavailable".
                    if !error.is_unavailable() || last_error.is_none() {
                        last_error = Some(error);
                    }
                    if stop {
                        break;
                    }
                }
            }
        }
        if reports.is_empty() {
            return Err(last_error.unwrap_or_else(|| {
                ZeppBridgeError::Unavailable(format!("{stream} window has no recognizable records"))
            }));
        }
        if let Some(error) = last_error {
            reasons.insert(error.to_string());
        }
        let incomplete =
            (!reasons.is_empty()).then(|| reasons.into_iter().collect::<Vec<_>>().join("; "));
        let db = self.write_db().await?;
        Self::finish_stream(&db, stream, &reports, incomplete)
    }
}
