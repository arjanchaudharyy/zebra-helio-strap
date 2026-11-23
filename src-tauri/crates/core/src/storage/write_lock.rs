//! Cross-process single-writer lock.
//!
//! The desktop app, the CLI and future scheduled jobs can all try to write the same `zepp.db` at once. An in-process
//! `Mutex` cannot stop a second process. When two syncs run at once, the mildest result is duplicate requests and duplicate cleanup.
//! The worst result is a migration or restore that another writer interrupts halfway.
//!
//! Every action that writes the database must get this lock first: sync, history backfill, schema migration, restore,
//! reparse, cleanup. Read-only queries do not take the write lock. A read-only connection cannot write anyway, and a queue
//! would only make all MCP queries hang during a long sync.
//!
//! The design uses a lock that "the operating system holds", not a custom lock file with a timestamp:
//!
//! * Windows opens the file in exclusive share mode. When the handle closes (normal exit, crash, Task Manager
//!   kills the process), the lock goes away at once.
//! * Unix-like systems use `flock`. The kernel also releases it when the process exits.
//!
//! So there is no failure mode like "the last process crashed, the lock file remains, and the next start cannot open the library",
//! which needs a person to delete a file.
//!
//! Network sync also has a **sync lease** ([`acquire_sync_lease`]). See the notes there.

use std::fs::{File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const LOCK_FILE: &str = "zepp.db.write-lock";
/// Records the purpose and pid of the current holder. Used only to tell the user "who is writing".
const HOLDER_FILE: &str = "zepp.db.write-lock.holder";
const SYNC_LOCK_FILE: &str = "zepp.db.sync-lock";
const SYNC_HOLDER_FILE: &str = "zepp.db.sync-lock.holder";
const POLL_INTERVAL: Duration = Duration::from_millis(120);

/// The purpose of one write action. A wait timeout shows it to the user, so the wording is for users.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WritePurpose {
    LifeEvent,
    Sync,
    HistoryBackfill,
    Migration,
    Restore,
    Backup,
    Reprocess,
    Cleanup,
    /// Compact old raw payloads.
    ///
    /// It once reused `Cleanup`, so a waiting sync told the user "another write operation is
    /// in progress (Clean up old data)". Compaction deletes no bytes, so that text was false. The purpose is shown
    /// to the user as is, so it must say what really happens.
    Compaction,
    /// Short writes: preferences, coverage ledger, sync metadata, integrity check results.
    Metadata,
    /// Install an update: hold the sync lease and the write lock until the process exits, so no library is left half-written at exit.
    Update,
}

impl WritePurpose {
    pub fn as_str(self) -> &'static str {
        match self {
            WritePurpose::LifeEvent => "life_event",
            WritePurpose::Sync => "sync",
            WritePurpose::HistoryBackfill => "history_backfill",
            WritePurpose::Migration => "migration",
            WritePurpose::Restore => "restore",
            WritePurpose::Backup => "backup",
            WritePurpose::Reprocess => "reprocess",
            WritePurpose::Cleanup => "cleanup",
            WritePurpose::Compaction => "compaction",
            WritePurpose::Metadata => "metadata",
            WritePurpose::Update => "update",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            WritePurpose::LifeEvent => "Edit life event",
            WritePurpose::Sync => "Cloud sync",
            WritePurpose::HistoryBackfill => "History backfill",
            WritePurpose::Migration => "Database upgrade",
            WritePurpose::Restore => "Restore from backup",
            WritePurpose::Backup => "Create backup",
            WritePurpose::Reprocess => "Reparse local payloads",
            WritePurpose::Cleanup => "Clean up old data",
            WritePurpose::Compaction => "Compact old payloads",
            WritePurpose::Metadata => "Save local settings",
            WritePurpose::Update => "Install update",
        }
    }
}

#[derive(Debug)]
pub enum WriteLockError {
    /// Another writer is in progress. `holder` is the purpose it reports. It may be unreadable.
    Busy { holder: Option<String> },
    /// The lock file itself cannot be opened (for example, the directory is not writable).
    Unavailable(io::Error),
}

impl std::fmt::Display for WriteLockError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WriteLockError::Busy { holder: Some(who) } => {
                write!(f, "Another ZeppBridge write operation is in progress ({who}). Wait for it to finish.")
            }
            WriteLockError::Busy { holder: None } => {
                write!(f, "Another ZeppBridge write operation is in progress. Wait for it to finish.")
            }
            WriteLockError::Unavailable(error) => {
                write!(f, "Cannot create the write lock: {error}")
            }
        }
    }
}

impl std::error::Error for WriteLockError {}

/// While it is held, no process on this machine can get the write lock of the same data directory.
///
/// Released on `Drop`: normal return, early `?` return and panic all reach it, so there is
/// no "forgot to unlock" path.
#[derive(Debug)]
pub struct ExclusiveWriteGuard {
    file: Option<File>,
    holder_path: PathBuf,
}

impl Drop for ExclusiveWriteGuard {
    fn drop(&mut self) {
        // Clear the holder info first, then release the handle, so the next holder does not read the previous purpose.
        let _ = std::fs::remove_file(&self.holder_path);
        if let Some(file) = self.file.take() {
            #[cfg(unix)]
            unlock_unix(&file);
            drop(file);
        }
    }
}

/// Try to get the lock now. If it is taken, return `Busy` without waiting.
pub fn try_acquire(
    data_dir: &Path,
    purpose: WritePurpose,
) -> Result<ExclusiveWriteGuard, WriteLockError> {
    acquire_with_timeout(data_dir, purpose, Duration::ZERO)
}

/// Try to get the lock again and again until the timeout.
///
/// Poll instead of blocking in a kernel call, so the caller can set a limit: if a GUI sync
/// waits on the lock for more than a few seconds, it must show "another operation is in progress" and not freeze.
pub fn acquire_with_timeout(
    data_dir: &Path,
    purpose: WritePurpose,
    timeout: Duration,
) -> Result<ExclusiveWriteGuard, WriteLockError> {
    acquire_file(data_dir, LOCK_FILE, HOLDER_FILE, purpose.label(), timeout)
}

/// Lease for network sync and history backfill: only one runs at a time per data directory.
///
/// It and the write lock are **two locks**. Before, sync held the write lock from before the network call until the end. A history
/// backfill often takes minutes, and auto sync every 15 minutes also takes ten seconds or more. During that time, a settings change or a life
/// event was rejected with "another write operation is in progress", while sync was only waiting on the network
/// and wrote no bytes. Now sync holds only this lease for its full run (it stops desktop and CLI from running one each at once),
/// and takes the write lock only for the parts that really write the database.
pub fn acquire_sync_lease(
    data_dir: &Path,
    purpose: WritePurpose,
    timeout: Duration,
) -> Result<ExclusiveWriteGuard, WriteLockError> {
    acquire_file(
        data_dir,
        SYNC_LOCK_FILE,
        SYNC_HOLDER_FILE,
        purpose.label(),
        timeout,
    )
}

/// The two cross-process locks for official authorization state (see `official/store.rs`).
///
/// Desktop, CLI and MCP all read and write the same official token: the CLI can be refreshing while the desktop disconnects, and
/// their in-process locks cannot see each other. They do not touch `zepp.db`, so they are separate from the write lock and the sync lease.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OfficialLock {
    /// The short step that writes credentials / `official.json` (compare-and-swap, disconnect, saving the login).
    Commit,
    /// The full round trip of one refresh: if two processes refresh with the same refresh token, the later one is rejected.
    Refresh,
}

pub fn acquire_official(
    data_dir: &Path,
    which: OfficialLock,
    timeout: Duration,
) -> Result<ExclusiveWriteGuard, WriteLockError> {
    let (lock_file, holder_file, label) = match which {
        OfficialLock::Commit => (
            "official.commit-lock",
            "official.commit-lock.holder",
            "Update official authorization",
        ),
        OfficialLock::Refresh => (
            "official.refresh-lock",
            "official.refresh-lock.holder",
            "Refresh official token",
        ),
    };
    acquire_file(data_dir, lock_file, holder_file, label, timeout)
}

fn acquire_file(
    data_dir: &Path,
    lock_file: &str,
    holder_file: &str,
    label: &str,
    timeout: Duration,
) -> Result<ExclusiveWriteGuard, WriteLockError> {
    std::fs::create_dir_all(data_dir).map_err(WriteLockError::Unavailable)?;
    let lock_path = data_dir.join(lock_file);
    let holder_path = data_dir.join(holder_file);
    let deadline = Instant::now() + timeout;

    loop {
        match open_exclusive(&lock_path) {
            Ok(file) => {
                // Report the purpose, so another process can show it while it waits. A write failure does not affect the lock itself.
                let _ = std::fs::write(&holder_path, format!("{label}|{}", std::process::id()));
                return Ok(ExclusiveWriteGuard {
                    file: Some(file),
                    holder_path,
                });
            }
            Err(error) if is_contention(&error) => {
                if Instant::now() >= deadline {
                    let holder = std::fs::read_to_string(&holder_path)
                        .ok()
                        .and_then(|text| text.split('|').next().map(str::to_string))
                        .filter(|value| !value.trim().is_empty());
                    return Err(WriteLockError::Busy { holder });
                }
                std::thread::sleep(POLL_INTERVAL);
            }
            Err(error) => return Err(WriteLockError::Unavailable(error)),
        }
    }
}

pub(crate) fn try_open_exclusive(path: &Path) -> io::Result<Option<File>> {
    match open_exclusive(path) {
        Ok(file) => Ok(Some(file)),
        Err(error) if is_contention(&error) => Ok(None),
        Err(error) => Err(error),
    }
}

#[cfg(windows)]
fn open_exclusive(path: &Path) -> io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt;
    // share_mode(0): no other handle can open this file at the same time. A second process (and
    // a second thread in the same process) gets a sharing violation. That is the behavior we want.
    OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .share_mode(0)
        .open(path)
}

#[cfg(windows)]
fn is_contention(error: &io::Error) -> bool {
    // ERROR_SHARING_VIOLATION (32) / ERROR_LOCK_VIOLATION (33)
    matches!(error.raw_os_error(), Some(32) | Some(33))
}

#[cfg(unix)]
fn open_exclusive(path: &Path) -> io::Result<File> {
    use std::os::unix::io::AsRawFd;
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)?;
    // LOCK_NB: return at once if the lock is taken. The loop above decides whether to keep waiting.
    let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(file)
}

#[cfg(unix)]
fn is_contention(error: &io::Error) -> bool {
    // Compare values, do not use match arms.
    //
    // On macOS `EWOULDBLOCK` and `EAGAIN` are the same number. With two pattern arms,
    // the second one is unreachable, and clippy's `unreachable_patterns` makes it an error
    // (`-D warnings` on CI). On Linux they are two different values, and both must match.
    // A `||` comparison has no such platform difference: the same value is only compared twice,
    // and different values each match.
    let Some(code) = error.raw_os_error() else {
        return false;
    };
    code == libc::EWOULDBLOCK || code == libc::EAGAIN || code == libc::EACCES
}

#[cfg(unix)]
fn unlock_unix(file: &File) {
    use std::os::unix::io::AsRawFd;
    unsafe {
        libc::flock(file.as_raw_fd(), libc::LOCK_UN);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("zeppbridge-write-lock-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_second_writer_is_refused_while_the_first_still_holds_the_lock() {
        let dir = temp_dir("contended");
        let first = try_acquire(&dir, WritePurpose::Sync).unwrap();

        let second = try_acquire(&dir, WritePurpose::HistoryBackfill);
        match second {
            Err(WriteLockError::Busy { holder }) => {
                assert_eq!(holder.as_deref(), Some("Cloud sync"), "must say who is writing");
            }
            other => panic!("the second writer must not get the lock: {other:?}"),
        }

        drop(first);
        // After release, the lock can be taken again at once. Nobody needs to delete the lock file.
        assert!(try_acquire(&dir, WritePurpose::HistoryBackfill).is_ok());
    }

    #[test]
    fn waiting_reports_who_holds_the_lock_instead_of_hanging() {
        let dir = temp_dir("timeout");
        let _held = try_acquire(&dir, WritePurpose::Restore).unwrap();
        let started = Instant::now();
        let error = acquire_with_timeout(&dir, WritePurpose::Sync, Duration::from_millis(300))
            .expect_err("cannot get the lock within the timeout");
        assert!(
            started.elapsed() >= Duration::from_millis(250),
            "must really have waited"
        );
        assert!(started.elapsed() < Duration::from_secs(5), "must not wait forever");
        let message = error.to_string();
        assert!(message.contains("Restore from backup"), "{message}");
    }

    #[test]
    fn a_dropped_guard_leaves_nothing_a_human_has_to_clean_up() {
        let dir = temp_dir("no-stale");
        {
            let _guard = try_acquire(&dir, WritePurpose::Migration).unwrap();
            assert!(dir.join(HOLDER_FILE).exists());
        }
        assert!(
            !dir.join(HOLDER_FILE).exists(),
            "holder info must go away with the lock"
        );
        // The lock file itself can stay (it is reused next time), but it must never block the next writer.
        assert!(try_acquire(&dir, WritePurpose::Sync).is_ok());
    }

    /// The sync lease blocks a second sync but not the write lock: while sync waits on the network, settings can still be written.
    #[test]
    fn a_running_sync_lease_blocks_another_sync_but_not_a_quick_write() {
        let dir = temp_dir("sync-lease");
        let lease = acquire_sync_lease(&dir, WritePurpose::Sync, Duration::ZERO).unwrap();
        match acquire_sync_lease(&dir, WritePurpose::HistoryBackfill, Duration::ZERO) {
            Err(WriteLockError::Busy { holder }) => assert_eq!(holder.as_deref(), Some("Cloud sync")),
            other => panic!("the second sync must not get the lease: {other:?}"),
        }
        assert!(try_acquire(&dir, WritePurpose::Metadata).is_ok());
        drop(lease);
        assert!(acquire_sync_lease(&dir, WritePurpose::HistoryBackfill, Duration::ZERO).is_ok());
    }

    #[test]
    fn different_data_directories_do_not_block_each_other() {
        let one = temp_dir("dir-one");
        let two = temp_dir("dir-two");
        let _first = try_acquire(&one, WritePurpose::Sync).unwrap();
        assert!(
            try_acquire(&two, WritePurpose::Sync).is_ok(),
            "the lock scope is the data directory, not the whole machine"
        );
    }
}
