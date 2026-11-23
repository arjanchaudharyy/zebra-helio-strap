//! Silent update: check in the background at launch. If a new version exists, download it (the updater plugin verifies the signature against the built-in public key during download).
//! When the download is done, only a green ring lights up around the top-left logo. The swap happens only when the user clicks "Update".
//!
//! - There is one copy of the state, here. When the UI is open, [`EVENT`] pushes it to the frontend (the logo progress ring and the settings row).
//!   A page that just mounted calls [`update_view`] once to catch up.
//! - Before the swap, take the sync lease and the write lock: if sync / backfill / replay runs, wait for it to end. After that, nothing new starts,
//!   so the process never exits with a half-written database.
//! - Installed builds (NSIS / MSI, the macOS .app) use the plugin installer (on Windows, the installer in passive mode).
//!   The portable build swaps files in place, see [`super::portable`].
//!
//! All failures are quiet: the logo stays the same, and only the settings row gives the reason.

use super::portable;
use crate::app_state::AppState;
use crate::ipc_error::AppError;
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};
use zeppbridge_core::storage::write_lock::{self, WritePurpose};

/// Event name for state changes pushed to the frontend.
pub(crate) const EVENT: &str = "update://state";
/// Platform entry for the portable update package in `latest.json` (written by CI package-portable.ps1).
///
/// Read the copy that includes the CLI. The older `windows-x86_64-portable` still ships, with only the main program and MCP:
/// older apps read it. Their allowlist has no CLI, so they reject the whole package if it includes the CLI.
const PORTABLE_TARGET: &str = "windows-x86_64-portable-cli";
/// Changes only the check URL, not the public key: wherever it points, the downloaded package must pass the built-in public key signature check.
/// Use it to run the full chain locally against a latest.json that you serve.
const ENDPOINT_ENV: &str = "ZEPPBRIDGE_UPDATE_ENDPOINT";
/// How often to check again while the app stays in the tray. Releases come out daily. With only one check at launch, a user who stays in the tray
/// misses several releases in a row.
const RECHECK_EVERY: Duration = Duration::from_secs(6 * 60 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Phase {
    /// This launch has not checked yet.
    Idle,
    Checking,
    Downloading,
    /// Downloaded and signature-verified. Waits for the user to click "Update".
    Ready,
    /// The user clicked "Update". It waits for sync to end, or the swap is in progress.
    Applying,
    UpToDate,
    Failed,
    /// This build does not update itself (Linux package managers, x86_64 mac).
    Unsupported,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct UpdateView {
    pub phase: Phase,
    pub version: Option<String>,
    /// Original release notes (the tag annotation at release time).
    pub notes: Option<String>,
    pub date: Option<String>,
    pub downloaded: u64,
    pub total: Option<u64>,
    /// After the user clicked "Update", it waits for sync / backfill to end.
    pub waiting_for_sync: bool,
    pub portable: bool,
    /// Failure reason code (`err.update.*`). The UI picks the text by code.
    pub error_code: Option<String>,
}

impl Default for UpdateView {
    fn default() -> Self {
        Self {
            phase: Phase::Idle,
            version: None,
            notes: None,
            date: None,
            downloaded: 0,
            total: None,
            waiting_for_sync: false,
            portable: false,
            error_code: None,
        }
    }
}

#[derive(Default)]
struct Inner {
    view: UpdateView,
    staged: Option<(Update, Vec<u8>)>,
}

/// Update state for this run (`app.manage`).
#[derive(Default)]
pub(crate) struct SilentUpdater {
    inner: Mutex<Inner>,
    /// The user turned off auto update in settings: skip scheduled rechecks. Only a manual "Check for updates" checks.
    manual_only: AtomicBool,
}

impl SilentUpdater {
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn view(&self) -> UpdateView {
        self.lock().view.clone()
    }
}

fn publish(app: &AppHandle, change: impl FnOnce(&mut Inner)) -> UpdateView {
    let updater = app.state::<SilentUpdater>();
    let view = {
        let mut inner = updater.lock();
        change(&mut inner);
        inner.view.clone()
    };
    let _ = app.emit(EVENT, &view);
    view
}

fn fail(app: &AppHandle, code: &str, detail: impl std::fmt::Display) {
    crate::diagnostics::log(&format!("Silent update: {code} ({detail})"));
    publish(app, |inner| {
        inner.staged = None;
        inner.view.phase = Phase::Failed;
        inner.view.waiting_for_sync = false;
        inner.view.error_code = Some(code.to_string());
    });
}

fn is_portable() -> bool {
    super::is_portable_update().unwrap_or(false)
}

/// The "Auto update" switch in settings (the UI stores it locally and tells the backend once at each launch). Off stops only the scheduled rechecks.
/// A version that is downloading or downloaded is not cancelled. The UI decides from the switch whether to send the launch check.
#[tauri::command]
pub(crate) fn update_set_auto(updater: tauri::State<'_, SilentUpdater>, enabled: bool) {
    updater.manual_only.store(!enabled, Ordering::Relaxed);
}

#[tauri::command]
pub(crate) fn update_view(updater: tauri::State<'_, SilentUpdater>) -> UpdateView {
    updater.view()
}

/// The UI starts the launch check. After that, check again every [`RECHECK_EVERY`]. While a download is running, done, or installing,
/// `update_start` returns as is and does not interrupt it. If this build does not update itself, stop checking.
pub(crate) fn spawn_periodic_recheck(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(RECHECK_EVERY).await;
            if crate::EXIT_GATE.is_exiting() {
                break;
            }
            if app
                .state::<SilentUpdater>()
                .manual_only
                .load(Ordering::Relaxed)
            {
                continue;
            }
            if update_start(app.clone()).phase == Phase::Unsupported {
                break;
            }
        }
    });
}

/// Starts a background check (at app launch, or from the manual "Check for updates" on the settings page). If a check, download, finished download,
/// or install is already in progress, return the current state as is. Do not start a second one.
#[tauri::command]
pub(crate) fn update_start(app: AppHandle) -> UpdateView {
    if crate::EXIT_GATE.is_exiting() {
        return app.state::<SilentUpdater>().view();
    }
    let supported = super::self_update_supported();
    let mut start = false;
    let view = publish(&app, |inner| {
        if matches!(
            inner.view.phase,
            Phase::Checking | Phase::Downloading | Phase::Ready | Phase::Applying
        ) {
            return;
        }
        inner.view = UpdateView {
            portable: is_portable(),
            ..UpdateView::default()
        };
        if supported {
            inner.view.phase = Phase::Checking;
            start = true;
        } else {
            inner.view.phase = Phase::Unsupported;
        }
    });
    if start {
        tauri::async_runtime::spawn(check_and_download(app));
    }
    view
}

async fn check_and_download(app: AppHandle) {
    let portable = is_portable();
    let mut builder = app.updater_builder().timeout(Duration::from_secs(60));
    if portable {
        builder = builder.target(PORTABLE_TARGET);
    }
    if let Some(endpoint) = std::env::var(ENDPOINT_ENV)
        .ok()
        .filter(|value| !value.trim().is_empty())
    {
        builder = match endpoint.trim().parse() {
            Ok(url) => match builder.endpoints(vec![url]) {
                Ok(builder) => builder,
                Err(error) => return fail(&app, "err.update.check_failed", error),
            },
            Err(error) => return fail(&app, "err.update.check_failed", error),
        };
    }
    let checked = match builder.build() {
        Ok(updater) => updater.check().await,
        Err(error) => return fail(&app, "err.update.check_failed", error),
    };
    let update = match checked {
        Ok(Some(update)) => update,
        Ok(None) => {
            // Log "up to date" too: before, only failures and finished downloads were logged. A user who read the log could not tell if a check found
            // no new version or never ran.
            crate::diagnostics::log(&format!(
                "Silent update: up to date ({})",
                app.package_info().version
            ));
            publish(&app, |inner| inner.view.phase = Phase::UpToDate);
            return;
        }
        // The plugin finds the platform entry before it compares versions: if the remote latest.json has no portable entry, it reports this error
        // even when the remote version is older. Every release since 3.0 has a portable entry, so a release without it is an older one.
        // Treat it as "no new version". Do not show alarming red text on the settings page.
        Err(tauri_plugin_updater::Error::TargetNotFound(target)) => {
            crate::diagnostics::log(&format!("Silent update: remote has no package for {target}, treated as no new version"));
            publish(&app, |inner| inner.view.phase = Phase::UpToDate);
            return;
        }
        Err(error) => return fail(&app, "err.update.check_failed", error),
    };

    let size = update
        .raw_json
        .pointer(&format!("/platforms/{}/size", update.target))
        .and_then(serde_json::Value::as_u64);
    publish(&app, |inner| {
        inner.view.phase = Phase::Downloading;
        inner.view.version = Some(update.version.clone());
        inner.view.notes = update.body.clone();
        inner.view.date = update.date.map(|date| date.to_string());
        inner.view.downloaded = 0;
        inner.view.total = size;
    });

    // Push progress once per 1% change, not once per network chunk.
    let mut downloaded = 0u64;
    let mut last_percent = 0u64;
    let progress_app = app.clone();
    let result = update
        .download(
            move |chunk, total| {
                downloaded += chunk as u64;
                let total = total.or(size);
                let percent = total.map_or(0, |total| downloaded * 100 / total.max(1));
                if percent != last_percent {
                    last_percent = percent;
                    publish(&progress_app, |inner| {
                        inner.view.downloaded = downloaded;
                        inner.view.total = total;
                    });
                }
            },
            || {},
        )
        .await;
    match result {
        Ok(bytes) => {
            crate::diagnostics::log(&format!(
                "Silent update: {} downloaded and signature verified ({} bytes)",
                update.version,
                bytes.len()
            ));
            publish(&app, |inner| {
                inner.view.phase = Phase::Ready;
                inner.view.downloaded = bytes.len() as u64;
                inner.view.total = Some(bytes.len() as u64);
                inner.staged = Some((update, bytes));
            });
        }
        Err(error) => fail(&app, "err.update.download_failed", error),
    }
}

/// Waits until the sync lease and the write lock are both free (if sync, backfill or replay runs, wait). After that, holds them until the process exits.
async fn hold_the_library(
    app: &AppHandle,
    data_dir: std::path::PathBuf,
) -> Result<
    (
        write_lock::ExclusiveWriteGuard,
        write_lock::ExclusiveWriteGuard,
    ),
    AppError,
> {
    let mut announced = false;
    loop {
        let dir = data_dir.clone();
        let attempt = tokio::task::spawn_blocking(move || {
            let lease = write_lock::acquire_sync_lease(&dir, WritePurpose::Update, Duration::ZERO)?;
            let write = write_lock::try_acquire(&dir, WritePurpose::Update)?;
            Ok::<_, write_lock::WriteLockError>((lease, write))
        })
        .await
        .map_err(|_| AppError::new("err.update.apply_failed", "Task was interrupted while waiting for sync to end"))?;
        match attempt {
            Ok(guards) => return Ok(guards),
            Err(write_lock::WriteLockError::Busy { .. }) => {
                if !announced {
                    announced = true;
                    publish(app, |inner| inner.view.waiting_for_sync = true);
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
            Err(error) => {
                return Err(AppError::new(
                    "err.update.apply_failed",
                    format!("Cannot get the data directory lock: {error}"),
                ))
            }
        }
    }
}

/// The user clicked "Update": wait for sync to end, check the data location, then swap. On success, the process exits (portable build and Windows
/// installed build) or restarts (macOS), so this command does not return in normal cases.
#[tauri::command]
pub(crate) async fn update_apply(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<(), AppError> {
    let staged = {
        let updater = app.state::<SilentUpdater>();
        let mut inner = updater.lock();
        if inner.view.phase != Phase::Ready {
            return Err(AppError::new("err.update.not_ready", "The new version has not finished downloading"));
        }
        inner.view.phase = Phase::Applying;
        inner.staged.take()
    };
    let _ = app.emit(EVENT, app.state::<SilentUpdater>().view());
    let Some((update, bytes)) = staged else {
        fail(&app, "err.update.not_ready", "The downloaded package is missing");
        return Err(AppError::new("err.update.not_ready", "The new version has not finished downloading"));
    };

    let result: Result<(), AppError> = async {
        let _guards = hold_the_library(&app, state.data_dir.clone()).await?;
        super::validate_data_location(&state.data_dir)?;
        crate::diagnostics::log(&format!("Silent update: start swap to {}", update.version));
        if is_portable() {
            let exe = std::env::current_exe()?;
            let staged = portable::stage(&exe, &bytes).map_err(|error| {
                AppError::new("err.update.package_invalid", format!("Invalid update package: {error}"))
            })?;
            portable::swap(staged).map_err(|error| {
                AppError::new(
                    "err.update.swap_failed",
                    format!("Program file swap failed. Original files restored: {error}"),
                )
            })?;
            std::process::Command::new(&exe)
                .arg(portable::AFTER_UPDATE_ARG)
                .spawn()
                .map_err(|error| {
                    AppError::new(
                        "err.update.launch_failed",
                        format!("The new version is in place, but it failed to launch: {error}"),
                    )
                })?;
            app.exit(0);
            return Ok(());
        }
        update.install(&bytes).map_err(|error| {
            AppError::new("err.update.install_failed", format!("Install failed: {error}"))
        })?;
        // On Windows, install exits the process itself. Only macOS gets here, and it must restart after the .app swap.
        // restart() never returns on a worker thread, retaining _guards forever.
        // ExitRequested then waits on this command's own write lock. The request
        // variant returns so those guards are released before the exit waiter.
        app.request_restart();
        Ok(())
    }
    .await;
    if let Err(error) = &result {
        // Swap did not happen: the package was used up. Go back to failed, and download again at next launch.
        fail(&app, &error.code, &error.message);
    }
    result
}
