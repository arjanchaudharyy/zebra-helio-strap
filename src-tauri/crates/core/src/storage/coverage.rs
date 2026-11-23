//! History coverage ledger.
//!
//! "Requested" and "received" are two different things, and "received" and "written" are two more.
//! Recording only "synced up to month X of year X" makes three very different states look the same:
//! we requested but the cloud returned nothing, the cloud returned data but parsing failed, and the data really was written to the local library.
//!
//! The ledger records the state of each block by (stream, month), so:
//!
//! * After an interruption, work continues from the unfinished block instead of from the start;
//! * Running again does not write again (a block already `persisted` is skipped);
//! * The UI can show "requested", "received", "written" and "cloud returned nothing" separately;
//! * Only when both the ledger and the facts in the library hold can we say this part of history is complete.

use super::Database;
use crate::models::error::Result;
use chrono::{DateTime, Datelike, NaiveDate, TimeZone, Utc};
use serde::{Deserialize, Serialize};

/// State of one block. The strings are a contract: the UI and the CLI both branch on them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChunkStatus {
    /// Planned, not requested yet.
    Pending,
    /// Requested, and written to the local library.
    Persisted,
    /// Requested, and the cloud clearly has no data for this period. **This is not a failure** and must not be retried.
    EmptyFromCloud,
    /// Part of the window was written, but the whole block is not complete yet (sub-slice 404, parse failure and so on).
    ///
    /// Data already stored stays. `needs_work()` is true, so it does not count as a "complete local copy".
    Partial,
    /// The request or the write failed. It can be retried.
    Failed,
}

impl ChunkStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            ChunkStatus::Pending => "pending",
            ChunkStatus::Persisted => "persisted",
            ChunkStatus::EmptyFromCloud => "empty_from_cloud",
            ChunkStatus::Partial => "partial",
            ChunkStatus::Failed => "failed",
        }
    }

    fn parse(value: &str) -> Self {
        match value {
            "persisted" => ChunkStatus::Persisted,
            "empty_from_cloud" => ChunkStatus::EmptyFromCloud,
            "partial" => ChunkStatus::Partial,
            "failed" => ChunkStatus::Failed,
            _ => ChunkStatus::Pending,
        }
    }

    /// Does it still need work? Written blocks and blocks the cloud confirmed empty are not requested again.
    /// A partial write is not a complete result and must be fetched again.
    pub fn needs_work(self) -> bool {
        matches!(
            self,
            ChunkStatus::Pending | ChunkStatus::Failed | ChunkStatus::Partial
        )
    }
}

/// After this many automatic retries, a block stops and waits for the user.
///
/// Transient faults (network jitter, rate limits) usually pass after one or two tries. If it still fails the third time,
/// the block itself most likely has a problem, and more automatic retries only spin and block the blocks after it.
pub const MAX_AUTO_ATTEMPTS: i64 = 3;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoverageChunk {
    pub stream: String,
    /// Month this block covers, `YYYY-MM-01`.
    pub chunk_start: String,
    /// Exclusive. The start of the next block.
    pub chunk_end: String,
    pub status: String,
    pub requested_at: Option<String>,
    pub fetched_at: Option<String>,
    pub persisted_at: Option<String>,
    pub records: i64,
    pub error: Option<String>,
    /// Number of attempts so far. Only failures add to it.
    pub attempts: i64,
}

/// Public detail of one failed block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FailedChunk {
    pub stream: String,
    /// `YYYY-MM-01`. The UI shows only the month.
    pub chunk_start: String,
    /// Redacted failure reason (source text). The UI uses `error_code` first and shows this only when there is no code.
    pub error: Option<String>,
    /// Stable code for the failure reason. Empty for rows in old libraries.
    pub error_code: Option<String>,
    pub attempts: i64,
    /// Automatic retries are used up. It moves again only when the user retries explicitly.
    pub exhausted: bool,
}

/// Coverage summary for one stream, for direct use by the UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamCoverage {
    pub stream: String,
    pub requested_chunks: i64,
    pub persisted_chunks: i64,
    pub empty_chunks: i64,
    pub failed_chunks: i64,
    pub pending_chunks: i64,
    /// Earliest / latest month actually written to the local library.
    pub persisted_from: Option<String>,
    pub persisted_to: Option<String>,
    /// Months requested where the cloud returned nothing, at most 120 (ten years): the board uses it to mark a stack as "not in the cloud" (3.0.4).
    pub empty_months: Vec<String>,
    pub records: i64,
}

/// Ledger view of a whole backfill.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoverageLedger {
    /// Range the user asked to cover.
    pub requested_from: Option<String>,
    pub requested_to: Option<String>,
    pub streams: Vec<StreamCoverage>,
    pub total_chunks: i64,
    pub completed_chunks: i64,
    /// True only when the ledger proves every block has a result (written or confirmed empty by the cloud).
    /// The README phrase "complete copy" may appear only when this is true.
    pub complete: bool,
    /// Month and reason for each failed block. The UI uses it to show "which month, and why",
    /// instead of only "N blocks failed".
    pub failed_chunks_detail: Vec<FailedChunk>,
    /// Failed blocks remain whose automatic retries are used up. The user must retry explicitly.
    pub needs_manual_retry: bool,
    /// Per-month view for the "kintsugi" calendar (3.1): which months are on this machine, which were filled by backfill,
    /// which the cloud does not have, and which are still cracks. Lists only months in the ledger or with local data.
    /// Months with neither are treated by the UI as cracks "not asked of the cloud yet".
    #[serde(default)]
    pub months: Vec<MonthSeam>,
}

/// The result for one month, reduced to one word from newest to oldest. The strings are a contract: the UI branches on them.
///
/// * `crack`: a stream failed or was only partly written, so it must be fetched again (still a crack even if other streams were written);
/// * `gold`: backfill wrote records, so the crack is filled with a gold line;
/// * `local`: this machine has data (from normal sync), and the ledger wrote no new records;
/// * `pending`: in the backfill plan, not asked of the cloud yet;
/// * `empty`: asked, and every stream in the cloud says it has nothing for this month. Not a failure, and not drawn as a crack.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MonthSeam {
    /// `YYYY-MM`.
    pub month: String,
    pub state: String,
}

/// Streams the history backfill covers. Per-workout detail (`workout_detail`) follows the workout summary
/// and gets no blocks of its own.
pub const BACKFILL_STREAMS: [&str; 6] = [
    "heart_rate",
    "daily_summary",
    "workouts",
    "sleep",
    "hrv",
    "wellness",
];

/// Split into blocks by calendar month.
///
/// Months instead of a fixed number of days, because the Zepp daily APIs already align to calendar months,
/// and "the 2026-03 block was not received" is easier to explain to a user than "block 47 was not received".
pub fn month_chunks(from: NaiveDate, to: NaiveDate) -> Vec<(NaiveDate, NaiveDate)> {
    let mut chunks = Vec::new();
    if to < from {
        return chunks;
    }
    let mut cursor = NaiveDate::from_ymd_opt(from.year(), from.month(), 1).unwrap_or(from);
    let limit = next_month(NaiveDate::from_ymd_opt(to.year(), to.month(), 1).unwrap_or(to));
    while cursor < limit {
        let next = next_month(cursor);
        chunks.push((cursor, next));
        cursor = next;
    }
    chunks
}

fn next_month(date: NaiveDate) -> NaiveDate {
    if date.month() == 12 {
        NaiveDate::from_ymd_opt(date.year() + 1, 1, 1).unwrap_or(date)
    } else {
        NaiveDate::from_ymd_opt(date.year(), date.month() + 1, 1).unwrap_or(date)
    }
}

pub fn to_utc(date: NaiveDate) -> DateTime<Utc> {
    Utc.from_utc_datetime(&date.and_hms_opt(0, 0, 0).unwrap_or_default())
}

impl Database {
    /// Add to a backfill plan.
    ///
    /// Finished blocks usually stay as they are. When a new collection API is added, only the affected streams are checked again, once.
    /// After that, a repeat request for the same history still skips finished blocks.
    pub fn plan_backfill(&self, from: NaiveDate, to: NaiveDate) -> Result<i64> {
        // A completed legacy wellness month did not include blood pressure.
        // Parser replay cannot fetch a response that was never requested.
        // Re-check those months once when the user next requests backfill;
        // retain all raw/canonical data and other streams' conclusions.
        const FETCH_REVISION: &str = "blood-pressure-date-v1";
        const REVISION_KEY: &str = "wellness_backfill_fetch_revision";
        let now = Utc::now().to_rfc3339();
        if self.get_app_meta(REVISION_KEY)?.as_deref() != Some(FETCH_REVISION) {
            self.conn.execute(
                "UPDATE coverage_ledger SET status = 'pending', attempts = 0,
                    error = NULL, error_code = NULL, updated_at = ?1
                 WHERE stream = 'wellness' AND status IN ('persisted', 'empty_from_cloud')",
                [&now],
            )?;
            // Write the marker after the idempotent update: interruption can
            // repeat requests, but cannot leave old completed chunks skipped.
            self.set_app_meta(REVISION_KEY, FETCH_REVISION)?;
        }
        let mut planned = 0i64;
        for stream in BACKFILL_STREAMS {
            for (start, end) in month_chunks(from, to) {
                let changed = self.conn.execute(
                    "INSERT INTO coverage_ledger
                        (stream, chunk_start, chunk_end, status, requested_at, records, updated_at)
                     VALUES(?1, ?2, ?3, 'pending', ?4, 0, ?4)
                     ON CONFLICT(stream, chunk_start) DO NOTHING",
                    rusqlite::params![stream, start.to_string(), end.to_string(), now],
                )?;
                planned += changed as i64;
            }
        }
        Ok(planned)
    }

    /// Blocks that still need work, from newest to oldest.
    ///
    /// Start from the latest month: a backfill can be cancelled at any time, so the first data back should be the months the user
    /// most likely wants to see now.
    ///
    /// Failed blocks that already reached [`MAX_AUTO_ATTEMPTS`] automatic retries are **no longer returned**. Otherwise a
    /// block that can never be fixed (for example, payloads arrive but not one canonical row parses) stays
    /// at the head of the queue and blocks all the blocks after it. They stay in the ledger and still count as unfinished, but wait for
    /// the user to release them explicitly with "Retry failed items".
    pub fn pending_backfill_chunks(&self, limit: usize) -> Result<Vec<CoverageChunk>> {
        let mut stmt = self.conn.prepare(
            "SELECT stream, chunk_start, chunk_end, status, requested_at, fetched_at,
                    persisted_at, records, error, attempts
             FROM coverage_ledger
             WHERE status = 'pending'
                OR (status IN ('failed', 'partial') AND attempts < ?2)
             ORDER BY chunk_start DESC, stream ASC
             LIMIT ?1",
        )?;
        let rows = stmt.query_map(
            rusqlite::params![limit as i64, MAX_AUTO_ATTEMPTS],
            map_chunk,
        )?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    /// Detail of failed blocks, so the UI can show "which month, and why" directly.
    ///
    /// The error text was always in the library but was never passed up, so the UI could only show "N blocks failed",
    /// and the user did not know which month it was or whether to retry.
    pub fn failed_backfill_chunks(&self) -> Result<Vec<FailedChunk>> {
        let mut stmt = self.conn.prepare(
            "SELECT stream, chunk_start, error, attempts, error_code
             FROM coverage_ledger
             WHERE status IN ('failed', 'partial')
             ORDER BY chunk_start DESC, stream ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(FailedChunk {
                stream: row.get(0)?,
                chunk_start: row.get(1)?,
                error: row.get::<_, Option<String>>(2)?,
                error_code: row.get::<_, Option<String>>(4)?,
                attempts: row.get(3)?,
                exhausted: row.get::<_, i64>(3)? >= MAX_AUTO_ATTEMPTS,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    /// Reset the attempt count of failed blocks to zero, so they go back into the automatic backfill queue.
    ///
    /// Touches only `failed` / `partial`: written blocks and blocks the cloud confirmed empty must not be sent back by this action,
    /// or "Retry failed items" quietly becomes "fetch everything again".
    pub fn reset_failed_backfill_chunks(&self) -> Result<usize> {
        let changed = self.conn.execute(
            "UPDATE coverage_ledger
                SET attempts = 0, updated_at = ?1
              WHERE status IN ('failed', 'partial')",
            [Utc::now().to_rfc3339()],
        )?;
        Ok(changed)
    }

    /// Set months back to pending backfill when "the ledger says written, but the library has no payload". Returns how many blocks changed.
    ///
    /// Retention cleanup before 3.0 deleted derived rows and payloads but did not reset the ledger: the ledger stayed
    /// `persisted`, so backfill always skipped those months. A real library was missing a full year of workouts from 2024-08,
    /// and 19 months of sleep, while the Zepp app still had them (found while checking a shoe distance mismatch, #143).
    /// The current `cleanup_old_data` also updates the ledger. This only repairs what was left before that fix.
    ///
    /// Looks only at raw payloads: a derived row always points to a payload, and a payload is deleted only when no derived row points to it. So
    /// if the window has no payload for this stream, nothing is left for this month. A wrong call costs only
    /// one more fetch (backfill is an idempotent upsert). After the fetch there are payloads, so it does not trigger again.
    pub fn reopen_chunks_without_raw(&self) -> Result<usize> {
        let changed = self.conn.execute(
            "UPDATE coverage_ledger
                SET status = 'pending', records = 0, persisted_at = NULL,
                    error = NULL, error_code = NULL, attempts = 0, updated_at = ?1
              WHERE status IN ('persisted', 'partial')
                AND NOT EXISTS (
                    SELECT 1 FROM raw_records r
                     WHERE r.stream = coverage_ledger.stream
                       AND r.start_utc < coverage_ledger.chunk_end
                       AND COALESCE(r.end_utc, r.start_utc) > coverage_ledger.chunk_start)",
            [Utc::now().to_rfc3339()],
        )?;
        Ok(changed)
    }

    /// Record the result of one block.
    ///
    /// A failure adds to `attempts`. A success (written or confirmed empty by the cloud) resets it to zero: a block that once
    /// failed because of network jitter must not carry that history after it later succeeds.
    pub fn record_backfill_chunk(
        &self,
        stream: &str,
        chunk_start: &str,
        status: ChunkStatus,
        records: i64,
        error: Option<&str>,
        error_code: Option<&str>,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let fetched_at = matches!(
            status,
            ChunkStatus::Persisted | ChunkStatus::EmptyFromCloud | ChunkStatus::Partial
        )
        .then(|| now.clone());
        let is_persisted = status == ChunkStatus::Persisted;
        let retryable = matches!(status, ChunkStatus::Failed | ChunkStatus::Partial);
        self.conn.execute(
            // records can only go up: a block can first be Partial(12) and then fail completely on retry
            // (0). A retry that wrote no new data does not mean those 12 rows vanished: they are still
            // in tables stored by natural-key upsert, and the ledger must not erase their count.
            "UPDATE coverage_ledger
                SET status = ?3,
                    records = MAX(records, ?4),
                    error = ?5,
                    fetched_at = COALESCE(?6, fetched_at),
                    persisted_at = CASE
                        WHEN ?8 THEN ?7
                        WHEN ?9 THEN NULL
                        ELSE persisted_at
                    END,
                    error_code = ?10,
                    attempts = CASE WHEN ?9 THEN attempts + 1 ELSE 0 END,
                    last_attempt_at = ?7,
                    updated_at = ?7
              WHERE stream = ?1 AND chunk_start = ?2",
            rusqlite::params![
                stream,
                chunk_start,
                status.as_str(),
                records,
                error,
                fetched_at,
                now,
                is_persisted,
                retryable,
                error_code
            ],
        )?;
        Ok(())
    }

    /// Ledger summary.
    pub fn coverage_ledger(&self) -> Result<CoverageLedger> {
        let mut stmt = self.conn.prepare(
            "SELECT stream, chunk_start, chunk_end, status, requested_at, fetched_at,
                    persisted_at, records, error, attempts
             FROM coverage_ledger
             ORDER BY stream ASC, chunk_start ASC",
        )?;
        let rows = stmt.query_map([], map_chunk)?;
        let mut by_stream: std::collections::BTreeMap<String, Vec<CoverageChunk>> =
            std::collections::BTreeMap::new();
        for row in rows {
            let chunk = row?;
            by_stream
                .entry(chunk.stream.clone())
                .or_default()
                .push(chunk);
        }

        let mut streams = Vec::new();
        let mut total = 0i64;
        let mut completed = 0i64;
        let mut requested_from: Option<String> = None;
        let mut requested_to: Option<String> = None;
        let mut month_chunks_seen: Vec<(String, String, i64)> = Vec::new();

        for (stream, chunks) in by_stream {
            let mut coverage = StreamCoverage {
                stream,
                requested_chunks: chunks.len() as i64,
                persisted_chunks: 0,
                empty_chunks: 0,
                failed_chunks: 0,
                pending_chunks: 0,
                persisted_from: None,
                persisted_to: None,
                empty_months: Vec::new(),
                records: 0,
            };
            for chunk in &chunks {
                month_chunks_seen.push((
                    chunk.chunk_start.clone(),
                    chunk.status.clone(),
                    chunk.records,
                ));
                total += 1;
                requested_from = min_option(requested_from.take(), Some(chunk.chunk_start.clone()));
                requested_to = max_option(requested_to.take(), Some(chunk.chunk_end.clone()));
                match ChunkStatus::parse(&chunk.status) {
                    ChunkStatus::Persisted => {
                        completed += 1;
                        coverage.persisted_chunks += 1;
                        coverage.records += chunk.records;
                        coverage.persisted_from = min_option(
                            coverage.persisted_from.take(),
                            Some(chunk.chunk_start.clone()),
                        );
                        coverage.persisted_to = max_option(
                            coverage.persisted_to.take(),
                            Some(chunk.chunk_start.clone()),
                        );
                    }
                    ChunkStatus::EmptyFromCloud => {
                        completed += 1;
                        coverage.empty_chunks += 1;
                        if coverage.empty_months.len() < 120 {
                            coverage.empty_months.push(chunk.chunk_start.clone());
                        }
                    }
                    ChunkStatus::Failed | ChunkStatus::Partial => {
                        coverage.failed_chunks += 1;
                        coverage.records += chunk.records;
                    }
                    ChunkStatus::Pending => coverage.pending_chunks += 1,
                }
            }
            streams.push(coverage);
        }

        let failed_chunks_detail = self.failed_backfill_chunks()?;
        let needs_manual_retry = failed_chunks_detail.iter().any(|chunk| chunk.exhausted);
        let months = self.month_seams(&month_chunks_seen)?;

        Ok(CoverageLedger {
            requested_from,
            requested_to,
            streams,
            total_chunks: total,
            completed_chunks: completed,
            // Complete only when every block has a result. No blocks at all also does not count as complete:
            // "nothing done" is not "already done".
            complete: total > 0 && completed == total,
            failed_chunks_detail,
            needs_manual_retry,
            months,
        })
    }

    /// Combine the ledger blocks and the months with local data into a per-month result (`MonthSeam`).
    ///
    /// Local months look only at three per-day / per-session tables: daily overview, sleep, workouts. Per-minute heart rate samples
    /// are too large to scan just to learn "does this month have data". Months with heart rate almost always have a daily overview.
    fn month_seams(&self, chunks: &[(String, String, i64)]) -> Result<Vec<MonthSeam>> {
        #[derive(Default)]
        struct Month {
            gold: bool,
            crack: bool,
            pending: bool,
            asked: bool,
            local: bool,
        }
        let mut months: std::collections::BTreeMap<String, Month> =
            std::collections::BTreeMap::new();
        for (start, status, records) in chunks {
            let Some(key) = start.get(..7) else { continue };
            let month = months.entry(key.to_string()).or_default();
            match ChunkStatus::parse(status) {
                ChunkStatus::Persisted if *records > 0 => month.gold = true,
                ChunkStatus::Persisted | ChunkStatus::EmptyFromCloud => {}
                ChunkStatus::Failed | ChunkStatus::Partial => month.crack = true,
                ChunkStatus::Pending => month.pending = true,
            }
            month.asked = true;
        }
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT substr(date, 1, 7) FROM daily_metrics
             UNION SELECT substr(date(end_time, 'localtime'), 1, 7) FROM sleep_sessions
             UNION SELECT substr(date(start_time, 'localtime'), 1, 7) FROM workouts",
        )?;
        let rows = stmt.query_map([], |row| row.get::<_, Option<String>>(0))?;
        for row in rows {
            if let Some(key) = row? {
                if key.len() == 7 {
                    months.entry(key).or_default().local = true;
                }
            }
        }
        Ok(months
            .into_iter()
            .map(|(month, m)| {
                let state = if m.crack {
                    "crack"
                } else if m.gold {
                    "gold"
                } else if m.local {
                    "local"
                } else if m.pending {
                    "pending"
                } else if m.asked {
                    "empty"
                } else {
                    "local"
                };
                MonthSeam {
                    month,
                    state: state.to_string(),
                }
            })
            .collect())
    }

    /// Clear the whole ledger. Used when the user changes their mind and plans a full backfill again.
    pub fn reset_coverage_ledger(&self) -> Result<()> {
        self.conn.execute("DELETE FROM coverage_ledger", [])?;
        Ok(())
    }
}

fn map_chunk(row: &rusqlite::Row<'_>) -> rusqlite::Result<CoverageChunk> {
    Ok(CoverageChunk {
        stream: row.get(0)?,
        chunk_start: row.get(1)?,
        chunk_end: row.get(2)?,
        status: row.get(3)?,
        requested_at: row.get(4)?,
        fetched_at: row.get(5)?,
        persisted_at: row.get(6)?,
        records: row.get(7)?,
        error: row.get(8)?,
        attempts: row.get(9)?,
    })
}

fn min_option(current: Option<String>, candidate: Option<String>) -> Option<String> {
    match (current, candidate) {
        (Some(a), Some(b)) => Some(if a <= b { a } else { b }),
        (Some(a), None) => Some(a),
        (None, b) => b,
    }
}

fn max_option(current: Option<String>, candidate: Option<String>) -> Option<String> {
    match (current, candidate) {
        (Some(a), Some(b)) => Some(if a >= b { a } else { b }),
        (Some(a), None) => Some(a),
        (None, b) => b,
    }
}

#[cfg(test)]
mod tests;
