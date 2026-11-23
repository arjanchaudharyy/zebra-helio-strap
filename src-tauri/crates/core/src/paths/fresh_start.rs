//! "Switch accounts and start fresh".
//!
//! The database binds to one Zepp account on the first write (`storage/owner.rs`).
//! After that, a different account is refused. Advanced data and the official
//! authorization must also use the same account. Before, a user who wanted to
//! switch accounts had to quit the app and rename the `data` folder in the
//! install folder by hand. Many users could not find that step.
//!
//! The running process holds the database, logs and WebView folder, and on Windows
//! the folder cannot be renamed while in use. So there are two steps: the command
//! deletes the token, puts a marker in the data folder, and restarts. The new
//! process renames the whole folder to `data-before-<time>` before logs or WebView
//! open any file. **No old data is deleted.** To go back, rename the folder back.

use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub(super) const MARKER: &str = ".fresh-start";
/// On restart, the old process can still hold some handles.
const RENAME_WAIT: Duration = Duration::from_secs(15);

/// Queue a request: on the next start, move this whole data folder aside.
pub fn request_fresh_start(data_dir: &Path) -> io::Result<()> {
    std::fs::write(data_dir.join(MARKER), b"1")
}

/// Call this at the very start (before logs, WebView and the database open). If a
/// request is queued, rename the folder, create an empty one, and return where the old data is now.
pub fn apply_pending_fresh_start(data_dir: &Path) -> io::Result<Option<PathBuf>> {
    apply_with_wait(data_dir, RENAME_WAIT)
}

pub(super) fn apply_with_wait(data_dir: &Path, wait: Duration) -> io::Result<Option<PathBuf>> {
    if !data_dir.join(MARKER).is_file() {
        return Ok(None);
    }
    let target = aside_path(
        data_dir,
        &chrono::Local::now().format("%Y%m%d-%H%M%S").to_string(),
    )?;
    let deadline = Instant::now() + wait;
    loop {
        match std::fs::rename(data_dir, &target) {
            Ok(()) => break,
            Err(_) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(300)),
            Err(error) => return Err(error),
        }
    }
    std::fs::create_dir_all(data_dir)?;
    // The marker moved with the old folder. Remove it, or renaming the folder back would move it aside again.
    let _ = std::fs::remove_file(target.join(MARKER));
    Ok(Some(target))
}

pub(super) fn aside_path(data_dir: &Path, stamp: &str) -> io::Result<PathBuf> {
    let parent = data_dir
        .parent()
        .ok_or_else(|| io::Error::other("data folder has no parent"))?;
    let name = data_dir
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "data".to_string());
    let mut candidate = parent.join(format!("{name}-before-{stamp}"));
    let mut n = 2;
    while candidate.exists() {
        candidate = parent.join(format!("{name}-before-{stamp}-{n}"));
        n += 1;
    }
    Ok(candidate)
}
