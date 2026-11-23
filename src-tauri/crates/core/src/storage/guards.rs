//! In-process flags for replay, compaction and background writes (split out of storage/mod.rs by domain, logic unchanged).

use super::*;

/// True while the startup replay is rewriting derived rows from stored raw
/// payloads.
///
/// The replay writes in bulk on its own connection; an automatic sync landing
/// in the middle of it used to lose the race for the write lock and surface as
/// a red "local database temporarily unavailable". A sync that knows the replay is running can
/// stand aside and come back instead, which is the honest answer: nothing
/// failed, the library is busy healing itself.
/// True while old raw payloads are being compacted in the background.
///
/// Same approach as the replay: the UI must be able to say "compacting", and sync must know someone is writing to the library now.
pub(super) static COMPACTION_IN_PROGRESS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

pub(super) type PayloadStatsMap = HashMap<String, (u64, i64)>;

pub(super) static PAYLOAD_STATS_MEM: std::sync::Mutex<Option<(String, PayloadStatsMap)>> =
    std::sync::Mutex::new(None);

pub fn compaction_in_progress() -> bool {
    COMPACTION_IN_PROGRESS.load(std::sync::atomic::Ordering::SeqCst) > 0
}

/// A counter, not a bool: the startup thread raises the flag before it takes the lock, and the compaction function goes one level deeper inside.
/// The inner Drop must not erase the fact that the outer level still waits for the lock.
pub struct CompactionGuard {
    pub(super) _private: (),
}

impl CompactionGuard {
    pub fn enter() -> Self {
        COMPACTION_IN_PROGRESS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Self { _private: () }
    }
}

impl Drop for CompactionGuard {
    fn drop(&mut self) {
        COMPACTION_IN_PROGRESS.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

/// How many times in this process "the health data in the library really changed": rewritten derived rows (normalization stored), rows deleted by cleanup,
/// changed training-day coverage evidence all move it forward one step. Fetching the same raw payload again unchanged (`touch_unchanged_raw`) does not.
///
/// The sync command reads it at the start and when each stream finishes, and tells the UI "did this stream / this round change anything":
/// if not, the pages do not need to drop their read caches and query the whole page again. It can over-report but never under-report:
/// a normalization that failed and rolled back also counts as one change.
static CONTENT_EPOCH: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub fn content_epoch() -> u64 {
    CONTENT_EPOCH.load(std::sync::atomic::Ordering::SeqCst)
}

pub(crate) fn note_content_change() {
    CONTENT_EPOCH.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
}

pub(super) static REPLAY_IN_PROGRESS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

/// Whether a raw-payload replay is running right now.
pub fn replay_in_progress() -> bool {
    REPLAY_IN_PROGRESS.load(std::sync::atomic::Ordering::SeqCst) > 0
}

/// Flag raised by an exit request.
///
/// The desktop app waits for the write lock to clear before it exits, and a replay or compaction can run for tens of seconds to minutes.
/// Without this flag, a user who clicks "Quit" waits for the whole maintenance window. Both loops check it at each batch
/// boundary: when it is set, they commit the current batch and stop, release the write lock, and the remaining payloads
/// continue at the next start (the revision does not advance, so committed batches are never redone).
pub(super) static BACKGROUND_WRITE_ABORT: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Called by the exit path. It affects only scheduling after the batch boundary and does not interrupt a transaction that is committing.
pub fn request_background_write_abort() {
    BACKGROUND_WRITE_ABORT.store(true, std::sync::atomic::Ordering::SeqCst);
}

pub(super) fn background_write_abort_requested() -> bool {
    BACKGROUND_WRITE_ABORT.load(std::sync::atomic::Ordering::SeqCst)
}

/// Clears the replay flag however the replay ends, including on an early
/// return or a panic. Nested enters are counted so an inner Drop cannot hide
/// an outer wait-for-lock.
pub struct ReplayGuard {
    pub(super) _private: (),
}

impl ReplayGuard {
    pub fn enter() -> Self {
        REPLAY_IN_PROGRESS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Self { _private: () }
    }
}

impl Drop for ReplayGuard {
    fn drop(&mut self) {
        REPLAY_IN_PROGRESS.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

/// Maximum number of pending run details one sync fetches. The rest wait for the next round, so one sync does not
/// spend its deadline on hundreds of permanent 404s.
pub const PENDING_WORKOUT_DETAIL_LIMIT: usize = 40;

/// After this many consecutive failures for one detail, it leaves the automatic queue for a while.
pub const MAX_WORKOUT_DETAIL_ATTEMPTS: i64 = 3;

/// After this many days the failure count decays back to 0, so a later cloud fix gets another try.
pub(super) const WORKOUT_DETAIL_ATTEMPT_DECAY: chrono::Duration = chrono::Duration::days(7);

/// Transaction boundary for one replay batch.
///
/// A commit must call `commit()` explicitly. An early `?` return and a panic both go through `Drop` and roll back.
/// Written by hand instead of with `rusqlite::Transaction` because the whole `Database` only has
/// `&self.conn`, and `Transaction` needs to borrow `&mut Connection`.
pub(super) struct ReplayBatch<'a> {
    pub(super) conn: &'a Connection,
    pub(super) open: bool,
}

impl<'a> ReplayBatch<'a> {
    pub(super) fn begin(conn: &'a Connection) -> Result<Self> {
        conn.execute("BEGIN IMMEDIATE", [])?;
        Ok(Self { conn, open: true })
    }

    pub(super) fn commit(mut self) -> Result<()> {
        self.open = false;
        self.conn.execute("COMMIT", [])?;
        Ok(())
    }
}

impl Drop for ReplayBatch<'_> {
    fn drop(&mut self) {
        if self.open {
            let _ = self.conn.execute("ROLLBACK", []);
        }
    }
}
