//! Speak up when the app cannot start.
//!
//! Before this file existed, ZeppBridge failed **in total silence** on Windows:
//! `main.rs` sets `windows_subsystem = "windows"`, so no `eprintln!` has a terminal to write to;
//! the project had no log file at all; and `setup()` in `lib.rs` used `?` for data directory
//! resolution and `AppState::new()`. If any step failed, the process exited. The user saw
//! a black window flash (that was something else), a dead icon left in the notification area,
//! no response to "Open ZeppBridge", and no fix from a reinstall. The user had nothing
//! to send us, and we had nothing to look at.
//!
//! This module gives two things:
//!
//! * [`init`] / [`log`]: a rotated log file next to the data directory. The user can send
//!   it to us directly.
//! * [`fatal_startup`]: the exit path for fatal startup errors. It writes `startup-error.log`,
//!   shows a native dialog on Windows with the **exact path and system error**,
//!   and only then lets the process exit.

use std::fmt::Display;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

/// Size limit for one log file. It holds many startups, but stays small enough to send.
const MAX_LOG_BYTES: u64 = 2 * 1024 * 1024;
/// Number of rotated history files to keep (`zeppbridge.log.1` ... `.3`).
const KEPT_LOGS: usize = 3;

static LOG_FILE: OnceLock<Option<PathBuf>> = OnceLock::new();
/// Whether the startup banner was written. See [`init`].
static BANNER_WRITTEN: AtomicBool = AtomicBool::new(false);
/// Many threads write the log (startup, background replay, commands). Rotation must keep
/// "check size" and "rename" atomic. An in-process lock is enough. The cross-process lock is
/// `storage::write_lock`, and the log does not need that much.
static WRITE_LOCK: Mutex<()> = Mutex::new(());

/// Tauri can panic while creating configured windows, before application setup.
/// Release builds on Windows have no stderr, so retain the panic in our log.
pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log(&format!("Application panic: {info}"));
        previous(info);
    }));
}

/// Log directory: `logs/` next to the data directory.
pub fn log_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("logs")
}

/// Fallback when the data directory could not be resolved.
///
/// This is the moment when the log is needed most, so it must not depend on the step before.
fn fallback_log_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("LOCALAPPDATA")
            .map(|base| PathBuf::from(base).join("ZeppBridge").join("logs"))
    }
    #[cfg(not(windows))]
    {
        Some(std::env::temp_dir().join("zeppbridge-logs"))
    }
}

/// Sets up the log file. `data_dir` is `None` when the data directory step already failed.
///
/// Only the first call takes effect (`OnceLock`). Startup first sets it up with the fallback.
/// A second call after the data directory is known is harmless, but it does **not** change the directory.
/// A change would split one startup into two files, and diagnosis needs one continuous record.
pub fn init(data_dir: Option<&Path>) {
    let _ = LOG_FILE.get_or_init(|| {
        let dir = data_dir.map(log_dir).or_else(fallback_log_dir)?;
        fs::create_dir_all(&dir).ok()?;
        let path = dir.join("zeppbridge.log");
        rotate_if_needed(&path);
        Some(path)
    });
    // On Windows, two places call `init`: the start of `run()` (so the WebView2 directory
    // log lines have a place to go) and `setup()`. Write the banner once only. Otherwise one startup shows
    // two "Starting ZeppBridge" lines, and the reader thinks the app restarted.
    if BANNER_WRITTEN.swap(true, Ordering::Relaxed) {
        return;
    }
    log(&format!(
        "Starting ZeppBridge {} / {} / exe={} / data_dir={}",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::current_exe()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| "(unknown)".into()),
        data_dir
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "(unresolved)".into()),
    ));
}

/// Appends one line. It never panics, and a failed write never affects the caller.
pub fn log(message: &str) {
    // Dev builds have a terminal, so keep the stderr copy too.
    #[cfg(debug_assertions)]
    eprintln!("{message}");

    let Some(Some(path)) = LOG_FILE.get() else {
        return;
    };
    let stamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
    let Ok(_guard) = WRITE_LOCK.lock() else {
        return;
    };
    rotate_if_needed(path);
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "[{stamp}] {message}");
    }
}

/// Above the limit, moves `.log` to `.log.1`, shifts older files back, and drops the oldest.
fn rotate_if_needed(path: &Path) {
    let Ok(meta) = fs::metadata(path) else {
        return;
    };
    if meta.len() < MAX_LOG_BYTES {
        return;
    }
    let _ = fs::remove_file(path.with_extension(format!("log.{KEPT_LOGS}")));
    for index in (1..KEPT_LOGS).rev() {
        let from = path.with_extension(format!("log.{index}"));
        let to = path.with_extension(format!("log.{}", index + 1));
        let _ = fs::rename(from, to);
    }
    let _ = fs::rename(path, path.with_extension("log.1"));
}

/// Fatal startup error: write a file and show a dialog. The caller decides when to exit.
///
/// Returns the error that `setup()` propagates, so the call site stays one line.
pub fn fatal_startup(context: &str, error: impl Display) -> anyhow::Error {
    let detail = format!("{context}: {error}");
    log(&format!("Startup failed: {detail}"));

    let target = LOG_FILE
        .get()
        .and_then(|slot| slot.clone())
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .or_else(fallback_log_dir);
    let mut where_to_look = String::new();
    if let Some(dir) = target {
        let _ = fs::create_dir_all(&dir);
        let file = dir.join("startup-error.log");
        let stamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
        let body = format!(
            "[{stamp}] ZeppBridge {} failed to start\r\n{detail}\r\n",
            env!("CARGO_PKG_VERSION")
        );
        if fs::write(&file, body).is_ok() {
            where_to_look = format!("\r\n\r\nDetails written to:\r\n{}", file.display());
        }
    }

    show_native_error(&format!(
        "ZeppBridge failed to start\r\n\r\n{detail}{where_to_look}"
    ));
    anyhow::anyhow!(detail)
}

/// No dialog in tests.
///
/// This is not cosmetic: `MessageBoxW` takes foreground focus and blocks until someone clicks OK,
/// and unit tests and CI have nobody to click.
#[cfg(test)]
fn show_native_error(_text: &str) {}

#[cfg(all(windows, not(test)))]
fn show_native_error(text: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        MessageBoxW, MB_ICONERROR, MB_OK, MB_SETFOREGROUND, MB_TOPMOST,
    };

    let wide = |value: &str| {
        value
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect::<Vec<u16>>()
    };
    let body = wide(text);
    let caption = wide("ZeppBridge");
    // No window exists yet, so the owner is null. TOPMOST | SETFOREGROUND keep it
    // from hiding behind other windows. At this point the user sees only "nothing happened".
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            body.as_ptr(),
            caption.as_ptr(),
            MB_OK | MB_ICONERROR | MB_TOPMOST | MB_SETFOREGROUND,
        );
    }
}

#[cfg(all(not(windows), not(test)))]
fn show_native_error(text: &str) {
    // The macOS .app and the Linux desktop entry have no terminal either, but we do not spawn osascript /
    // zenity here: startup already failed, and an external process only adds one more way to fail. The log file is
    // the reliable exit on these two platforms, and the log line above already has its path.
    eprintln!("{text}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotation_keeps_the_newest_and_drops_the_oldest() {
        let dir = std::env::temp_dir().join("zeppbridge-log-rotation-test");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("zeppbridge.log");
        fs::write(&path, vec![b'x'; MAX_LOG_BYTES as usize + 1]).unwrap();
        rotate_if_needed(&path);
        assert!(!path.exists(), "the oversized file must be moved away");
        assert!(path.with_extension("log.1").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    /// A fatal startup error must produce a file that the user can send directly.
    ///
    /// The dialog half is a no-op in tests (it takes focus). This test checks the file.
    /// Before, this file did not exist, and the user had nothing at all.
    #[test]
    fn a_fatal_startup_error_lands_in_a_file_the_user_can_send() {
        let dir = std::env::temp_dir().join("zeppbridge-fatal-startup-test");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        // Set the log location directly: `init` is a `OnceLock`, and tests must not depend on order.
        let _ = LOG_FILE.set(Some(dir.join("zeppbridge.log")));

        let error = fatal_startup(
            r"Cannot use data folder C:\Program Files\ZeppBridge\data",
            std::io::Error::new(std::io::ErrorKind::PermissionDenied, "access is denied"),
        );
        // The returned message must include the path and the system error, not only "startup failed".
        let text = error.to_string();
        assert!(text.contains(r"ZeppBridge\data"), "{text}");
        assert!(text.contains("access is denied"), "{text}");

        let written = fs::read_to_string(dir.join("startup-error.log")).expect("the file must be written");
        assert!(written.contains("access is denied"), "{written}");
        assert!(written.contains(env!("CARGO_PKG_VERSION")), "{written}");
        // The same event must also go to the normal log, or the timeline breaks here.
        let log = fs::read_to_string(dir.join("zeppbridge.log")).expect("the normal log must have a line too");
        assert!(log.contains("Startup failed"), "{log}");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rotation_leaves_a_small_file_alone() {
        let dir = std::env::temp_dir().join("zeppbridge-log-small-test");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("zeppbridge.log");
        fs::write(&path, b"one line").unwrap();
        rotate_if_needed(&path);
        assert!(path.exists());
        assert!(!path.with_extension("log.1").exists());
        let _ = fs::remove_dir_all(&dir);
    }
}
