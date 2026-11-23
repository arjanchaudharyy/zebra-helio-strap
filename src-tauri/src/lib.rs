mod app_state;
mod auto_sync;
mod clipboard_files;
mod commands;
mod diagnostics;
mod exit_gate;
mod external_open;
mod ipc_error;
mod ipc_types;
#[cfg(target_os = "linux")]
mod linux_rendering;
mod local_api;
mod main_window;
mod tray_i18n;
mod uninstall;
pub use uninstall::{purge_credentials_for_uninstall, PURGE_CREDENTIALS_ARG};
pub mod feedback_toast;
mod store_import;
#[cfg(feature = "github-updater")]
mod updates;
#[cfg(windows)]
mod web_composer;

// The desktop shell is an adapter over the shared core: models, storage,
// migrations, normalization, queries, export and write coordination all live in
// `zeppbridge-core` so the CLI, MCP server and local REST API answer from the
// same semantics instead of re-implementing them.
pub use zeppbridge_core::{
    auth, connectors, decoder, device_catalog, export_fit, export_formats, fetcher, insight,
    models, normalizer, paths, sport_catalog, storage, sync,
};

use app_state::AppState;
use commands::{
    ai_coach_ask, ai_coach_configure, ai_coach_key_status, ai_coach_set_key, ai_coach_status, strap_alerts, strap_daily, strap_key_fetch, daily_goals, strap_detail, strap_live_recent, strap_live_start, strap_set, strap_buzz, strap_settings, strap_live_stop, strap_sync_now, zebra_start_route,
    ai_exchange_delete, ai_exchange_list, ai_exchange_rename, ai_handoff_autopaste,
    ai_handoff_cancel, ai_handoff_copy_file, ai_profile_save, ai_prompt_export, ai_prompt_import,
    ai_prompt_versions, ai_task_attachment_stat, ai_task_day_strip, ai_task_delete,
    ai_task_delete_many, ai_task_get, ai_task_list, ai_task_prepare, ai_task_preview, ai_task_save,
    ai_task_set_pinned, ai_template_delete, ai_template_list, ai_template_save, annual_rings,
    cancel_official_login, cancel_pending_restore, cancel_sync, cancel_web_login,
    card_collection_get, card_collection_set, check_official_login_now, cleanup_old_data,
    clear_auth, cloud_companion_open, cloud_companion_save_config, cloud_companion_setup,
    cloud_companion_state, cloud_companion_upload, coach_note_add, coach_note_decide,
    coach_note_delete, coach_note_edit, coach_notes_export, coach_notes_list, compact_raw_payloads,
    copy_workout_gpx, create_manual_backup, data_letters_mail_status, data_letters_open_capsule,
    data_letters_seal_capsule, data_letters_state, delete_life_event, disconnect_official,
    edit_workouts, export_for_unstrap, export_my_records, get_app_status, get_capability_overview,
    get_coverage_ledger, get_daily_heart_rate_extremes, get_data_health, get_device_profile,
    get_device_profiles, get_health_overview, get_heart_rate_series, get_heart_rate_zones,
    get_hidden_workout_page, get_hourly_steps, get_login_status, get_mcp_sidecar,
    get_metric_baselines, get_metric_series, get_official_status, get_pending_restore,
    get_recent_sleep, get_recent_workouts, get_restore_preview, get_sleep_detail,
    get_sleep_nap_stages, get_sleep_page, get_storage_estimate, get_strength_catalog,
    get_stress_series, get_training_balance, get_unknown_workout_codes, get_user_prefs,
    get_weekly_report, get_workout_detail, get_workout_detail_coverage, get_workout_fit_override,
    get_workout_insight, get_workout_page, get_workout_series, get_workout_type_options,
    import_batches, import_workout_fit, import_workouts, injuries_list, injury_delete, injury_save,
    list_backups, list_life_events, manual_auth,
    mcp_write_undo, mcp_writes_list, metric_exclusions, open_data_folder, open_external_url,
    personal_records, prepare_ai_handoff, probe_data_capabilities, push_import_companion,
    push_remove, push_run, push_save, push_set_enabled, push_state, race_delete, race_save,
    races_list, recover_advanced_credentials, recover_official_credentials, remove_workout_fit,
    reprocess_local_data, reset_coverage_ledger, retry_failed_backfill_chunks, rings_save_image,
    run_database_integrity_check, save_fit_export, save_life_event, save_mcp_bundle,
    set_backup_pinned, set_device_hidden, set_device_model_override,
    set_heart_rate_zone_preference, set_mcp_task_only, set_metric_excluded, set_user_prefs,
    set_workout_code_label, set_workout_type_override, shoe_delete, shoe_save, shoes_overview,
    stage_restore, start_fresh_library, start_history_backfill, start_history_sync,
    start_incremental_sync, start_official_login, start_web_login, start_workout_detail_backfill,
    system_region_locale, training_plan_adherence, training_plan_discard,
    training_plan_history, training_plan_preview, training_plan_publish, training_plan_save_draft,
    training_plan_set_ai_publish, training_plan_state, training_plan_update_draft,
    undo_import_batch, undo_workout_edit, verify_auth, verify_backup, workout_feedback_adjusted,
    workout_feedback_dismiss, workout_feedback_get, workout_feedback_prompt, workout_feedback_save,
    workout_feedback_snooze, workout_feedback_toast_due, workout_feedback_toast_show,
    workout_feel_get, workout_feel_save, workout_import_sources,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager};
use zeppbridge_core::models::RawPayloadCompaction;
use zeppbridge_core::storage::write_lock::{self, WritePurpose};

static EXIT_GATE: exit_gate::ExitGate = exit_gate::ExitGate::new();
// Retain the write barrier until process termination, including the gap between
// the waiter's success and the event loop processing its final exit request.
static EXIT_WRITE_GUARD: std::sync::Mutex<Option<write_lock::ExclusiveWriteGuard>> =
    std::sync::Mutex::new(None);
struct ExitDataDir(std::path::PathBuf);

/// Whether the backend (AppState) is ready.
///
/// `AppState::new` takes a full snapshot of a large database before migration, which takes several seconds for a database of hundreds of MB.
/// This work moved to the `app-init` worker thread, so the event loop draws the window first. In the frontend,
/// every `invoke` first passes the `whenBackendReady` gate (it listens to `app://ready` first,
/// then asks this command once; see `src/lib/bridge/tauri.ts`), so no other
/// command needs its own readiness check.
static APP_READY: AtomicBool = AtomicBool::new(false);

#[tauri::command]
fn app_is_ready() -> bool {
    APP_READY.load(Ordering::SeqCst)
}

/// The two numbers for `minWidth` / `minHeight` in `tauri.conf.json`.
///
/// The computed initial size must not be below them: if it is, the window is too small to click, and Tauri enforces the minimum size
/// only when the user drags. `set_size` takes whatever value it gets.
const MIN_WINDOW_WIDTH: f64 = 520.0;
const MIN_WINDOW_HEIGHT: f64 = 560.0;

/// Desktop startup may prepare the Store's LocalState path before the user
/// answers the import prompt. CLI and MCP go through `resolve_data_dir`, which
/// intentionally blocks until that choice has been recorded.
#[cfg(feature = "store")]
fn desktop_data_dir() -> std::io::Result<std::path::PathBuf> {
    match paths::prepare_package_local_data_dir()? {
        Some(dir) => Ok(dir),
        None => paths::resolve_data_dir(),
    }
}

#[cfg(not(feature = "store"))]
fn desktop_data_dir() -> std::io::Result<std::path::PathBuf> {
    paths::resolve_data_dir()
}

/// The initial size of the main window, computed from the monitor work area.
///
/// This used to be three lines of arithmetic inline in `setup()`, with **no lower bound**:
///
/// ```text
/// let work_w = (work.size.width as f64 / scale) - 24.0;
/// let width  = (work_w * 0.88).max(1280.0_f64.min(work_w));
/// ```
///
/// A zero work area makes the old algorithm compute a negative number, so the input check stays. But the user's logs did not
/// record this branch, so we cannot treat it as the confirmed cause of that user's window not opening.
///
/// Returns `None` when the monitor data cannot be trusted. Then change nothing, and let
/// the 1280x800 in `tauri.conf.json` apply as is.
fn main_window_size(work_width: u32, work_height: u32, scale: f64) -> Option<(f64, f64)> {
    if work_width == 0 || work_height == 0 || !scale.is_finite() || scale <= 0.0 {
        return None;
    }
    let work_w = (f64::from(work_width) / scale) - 24.0;
    let work_h = (f64::from(work_height) / scale) - 32.0;
    if !work_w.is_finite() || !work_h.is_finite() || work_w <= 0.0 || work_h <= 0.0 {
        return None;
    }
    let width = (work_w * 0.88).max(1280.0_f64.min(work_w));
    let height = (work_h * 0.88).max(800.0_f64.min(work_h));
    Some((width.max(MIN_WINDOW_WIDTH), height.max(MIN_WINDOW_HEIGHT)))
}

// The tray label table is in `tray_i18n.rs` (ten languages). This file keeps only the call sites.
// `tray_i18n::tray_labels` normalizes the language code itself and accepts `pt-BR`/`de-AT`/env style values.

fn handle_exit_requested(app: &AppHandle, code: Option<i32>, api: &tauri::ExitRequestApi) {
    match EXIT_GATE.request() {
        exit_gate::ExitAction::Allow => return,
        exit_gate::ExitAction::KeepWaiting => {
            api.prevent_exit();
            return;
        }
        exit_gate::ExitAction::CheckWriters => {}
    }
    let Some(data_dir) = app.try_state::<ExitDataDir>().map(|dir| dir.0.clone()) else {
        EXIT_GATE.writers_finished();
        return;
    };
    match write_lock::try_acquire(&data_dir, WritePurpose::Metadata) {
        Ok(guard) => {
            *EXIT_WRITE_GUARD.lock().unwrap_or_else(|p| p.into_inner()) = Some(guard);
            EXIT_GATE.writers_finished();
        }
        Err(write_lock::WriteLockError::Busy { .. }) => {
            api.prevent_exit();
            // Replay/compaction stops at batch boundaries when it sees this flag, and gives the write lock back soon. The exit
            // wait thread does not wait for the whole maintenance window (which can take minutes on a large database).
            storage::request_background_write_abort();
            if let Some(state) = app.try_state::<AppState>() {
                if let Ok(sync) = state.sync.try_read() {
                    if let Some(manager) = sync.as_ref() {
                        manager.request_cancel();
                    }
                }
            }
            let app = app.clone();
            let spawned = std::thread::Builder::new()
                .name("exit-writer-wait".into())
                .spawn(move || {
                    match write_lock::acquire_with_timeout(
                        &data_dir,
                        WritePurpose::Metadata,
                        Duration::from_secs(300),
                    ) {
                        Ok(guard) => {
                            *EXIT_WRITE_GUARD.lock().unwrap_or_else(|p| p.into_inner()) =
                                Some(guard);
                            EXIT_GATE.writers_finished();
                            diagnostics::log("Exit: writes finished, continuing to exit");
                            app.exit(code.unwrap_or(0));
                        }
                        Err(error) => {
                            diagnostics::log(&format!(
                                "Exit cancelled: waiting for writes to finish failed, can retry: {error}"
                            ));
                            EXIT_GATE.retry_later();
                        }
                    }
                });
            if let Err(error) = spawned {
                diagnostics::log(&format!("Exit cancelled: could not start the wait thread: {error}"));
                EXIT_GATE.retry_later();
            }
        }
        Err(error) => {
            api.prevent_exit();
            EXIT_GATE.retry_later();
            diagnostics::log(&format!("Exit: could not check the write lock: {error}"));
        }
    }
}

/// The tray labels must also change after the tray is built: when the user changes the language in settings, the tray must not keep the old language.
struct TrayMenuItems {
    show: MenuItem<tauri::Wry>,
    sync: MenuItem<tauri::Wry>,
    quit: MenuItem<tauri::Wry>,
}

/// The system UI language code, like `zh-CN` / `de_DE.UTF-8`. Returns `None` if it cannot be read.
///
/// Used only once, before the frontend tells us the language (first tray build). `tray_i18n::tray_labels`
/// handles normalization and fallback. Here we pass on the raw language code as is.
fn system_ui_locale() -> Option<String> {
    for key in ["LC_ALL", "LC_MESSAGES", "LANG", "LANGUAGE"] {
        if let Some(value) = std::env::var_os(key) {
            let value = value.to_string_lossy();
            // `LANGUAGE` can be a preference list like `zh_CN:en`. Take only the first item.
            // The other variables have no colon, so split does nothing to them.
            let first = value.split(':').next().unwrap_or_default().trim();
            if !first.is_empty() {
                return Some(first.to_string());
            }
        }
    }
    #[cfg(windows)]
    {
        // Windows does not set these environment variables. Ask the system for the user's UI language.
        //
        // This used to spawn a `powershell -NoProfile -Command "(Get-Culture).Name"`
        // process. It was on the startup path, and a PowerShell cold start on machines with antivirus or strict group policy
        // takes 0.5 to 2 seconds. The main UI waited that long, with no feedback
        // during the wait. `GetUserDefaultLocaleName` is the native way to do the same thing, in microseconds.
        if let Some(name) = windows_ui_locale() {
            return Some(name);
        }
    }
    None
}

/// The user's UI language, like `zh-CN` / `en-US`. Returns `None` if it cannot be read.
#[cfg(windows)]
pub(crate) fn windows_ui_locale() -> Option<String> {
    use windows_sys::Win32::Globalization::GetUserDefaultLocaleName;

    // `LOCALE_NAME_MAX_LENGTH` (winnls.h, 85 wchars, including the final NUL).
    // windows-sys does not export this constant, so we state the source here instead of picking a number.
    const LOCALE_NAME_MAX_LENGTH: usize = 85;

    let mut buffer = [0u16; LOCALE_NAME_MAX_LENGTH];
    // The return value is the number of characters written, **including** the final NUL. 0 means failure.
    let written = unsafe { GetUserDefaultLocaleName(buffer.as_mut_ptr(), buffer.len() as i32) };
    if written <= 1 {
        return None;
    }
    Some(String::from_utf16_lossy(&buffer[..(written - 1) as usize]))
}

/// Correct the tray labels after the frontend sets the UI language.
#[tauri::command]
fn set_tray_locale(app: AppHandle, locale: String) -> std::result::Result<(), ipc_error::AppError> {
    let Some(items) = app.try_state::<TrayMenuItems>() else {
        return Ok(());
    };
    let labels = tray_i18n::tray_labels(&locale);
    let _ = items.show.set_text(labels.show);
    let _ = items.sync.set_text(labels.sync);
    let _ = items.quit.set_text(labels.quit);
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // The portable build just swapped the exe and the old process started this one: wait for the old process to exit fully, then set up the single instance.
    // If not, the single instance would merge this launch into the old process that has not exited yet, and the new version would vanish at once. A normal launch only
    // cleans up the `*.old` left by the last update (updates/portable.rs).
    #[cfg(feature = "github-updater")]
    if let Ok(exe) = std::env::current_exe() {
        if std::env::args().any(|arg| arg == updates::portable::AFTER_UPDATE_ARG) {
            updates::portable::wait_for_previous(&exe, std::time::Duration::from_secs(20));
        }
        updates::portable::sweep(&exe);
    }

    // The rename queued by "start fresh with another account" must run before logs, WebView and the database open the data directory
    // (commands/auth.rs::start_fresh_library).
    if let Ok(data_dir) = paths::resolve_data_dir() {
        let moved = paths::apply_pending_fresh_start(&data_dir);
        if !matches!(moved, Ok(None)) {
            diagnostics::init(Some(&data_dir));
            match moved {
                Ok(Some(aside)) => {
                    diagnostics::log(&format!("Start fresh: old data moved to {}", aside.display()))
                }
                Err(error) => diagnostics::log(&format!(
                    "Start fresh: could not move the data folder ({error}), will retry at next launch"
                )),
                Ok(None) => {}
            }
        }
    }

    // The WebView2 user data directory must be set before `Builder`: WebView2 reads the environment variable
    // when it initializes, so setting it in `setup()` is too late.
    //
    // Set this variable only when the directory **was really created**. Before, this was
    // `let _ = std::fs::create_dir_all(...)` followed by an unconditional `set_var`: controlled folder
    // access, antivirus folder protection and OneDrive locks can all make creation fail, but the variable was set anyway.
    // Then WebView2 got a path it could not use and failed to initialize: the window and the tray were there,
    // but the window was blank. A bad path is worse than no path: without it, WebView2 uses the system default directory
    // and can at least open.
    #[cfg(target_os = "windows")]
    {
        let resolved = desktop_data_dir();
        // Set up logging first, or all the `log` calls below are lost. They are the
        // only evidence we can get for "blank window" problems. `init` is idempotent.
        diagnostics::init(resolved.as_deref().ok());
        match resolved {
            Ok(data_dir) => {
                let webview_dir = paths::webview_user_data_dir(&data_dir);
                match std::fs::create_dir_all(&webview_dir) {
                    Ok(()) => std::env::set_var("WEBVIEW2_USER_DATA_FOLDER", &webview_dir),
                    Err(error) => diagnostics::log(&format!(
                        "Could not create WebView2 data directory {} ({error}), using the system default directory",
                        webview_dir.display()
                    )),
                }
            }
            Err(error) => {
                diagnostics::log(&format!("Could not resolve the data directory ({error}); startup will report the error later"));
            }
        }
    }
    // Record the selected Linux graphics policy before creating GTK/WebKit.
    // Previously configure() ran before log initialization, losing this line in
    // release builds precisely when it was needed to diagnose rendering issues.
    #[cfg(target_os = "linux")]
    let rendering_script = {
        let resolved = paths::resolve_data_dir();
        diagnostics::init(resolved.as_deref().ok());
        linux_rendering::configure()
    };
    #[cfg(not(target_os = "linux"))]
    let rendering_script = "";
    diagnostics::install_panic_hook();
    diagnostics::log("Startup: building Tauri runtime");
    let builder = tauri::Builder::default()
        .append_invoke_initialization_script(rendering_script)
        .manage(main_window::MainWindowState::default())
        .manage(auto_sync::AutoSyncAlarm::default())
        // Single-instance must be registered first so a second launch never
        // reaches tray setup and creates a duplicate icon.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            main_window::request_show(app, "second-instance");
        }))
        // Main-window AI handoff links call the opener API explicitly.  Do
        // not inject its `_blank` click interceptor into the Zepp login
        // webview: OAuth must stay inside the cookie-polling login session.
        .plugin(
            tauri_plugin_opener::Builder::new()
                .open_js_links_on_click(false)
                .build(),
        )
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_drag::init())
        .plugin(tauri_plugin_process::init())
        .on_window_event(main_window::on_window_event)
        .on_page_load(|webview, payload| {
            if webview.label() == "main" {
                // Do not log URLs: the login webview can contain OAuth credentials.
                diagnostics::log(&format!("Main webview page load: {:?}", payload.event()));
            }
        })
        .setup(|app| {
            diagnostics::log("Startup: entered application setup");
            // These three steps used to be bare `?`. If any step failed, `setup` returned Err
            // and the process was gone. On Windows there is no terminal and no log, so the user only saw
            // a dead icon in the notification area. Now each one writes a log and shows a dialog box
            // with the exact path and system error. See `diagnostics.rs`.
            let data_dir = match desktop_data_dir() {
                Ok(dir) => {
                    diagnostics::init(Some(&dir));
                    dir
                }
                Err(error) => {
                    diagnostics::init(None);
                    return Err(diagnostics::fatal_startup("Cannot use the data folder", error).into());
                }
            };
            let store_import_gate = store_import::StoreImportGate::new(data_dir.clone());
            app.manage(store_import_gate.clone());
            // Available before the slow initializer publishes AppState, so a
            // quit during migration/restoration still waits for its writer.
            app.manage(ExitDataDir(data_dir.clone()));
            // Check whether this machine has a second database. If so, the user sees "my data is gone",
            // but the truth is that this launch resolved a different directory (NSIS installs to %LOCALAPPDATA%, MSI installs to
            // Program Files, and the two locations differ). We only log it and do not move anything. See
            // the comment on `paths::other_libraries_on_this_machine`.
            #[cfg(not(feature = "store"))]
            for other in paths::other_libraries_on_this_machine(&data_dir) {
                diagnostics::log(&format!(
                    "Note: {} also has a zepp.db. This launch uses {}. To choose one, set {}",
                    other.display(),
                    data_dir.display(),
                    paths::DATA_DIR_ENV
                ));
            }
            let webview_dir = paths::webview_user_data_dir(&data_dir);
            if let Err(error) = std::fs::create_dir_all(&webview_dir) {
                return Err(diagnostics::fatal_startup(
                    &format!("Cannot create WebView data directory {}", webview_dir.display()),
                    error,
                )
                .into());
            }
            std::env::set_var("WEBVIEW2_USER_DATA_FOLDER", &webview_dir);

            // The rest of the setup can be slow: a queued restore copies the whole database, and `AppState::new`
            // takes a full snapshot before the schema migration. A database of hundreds of MB takes several seconds,
            // and all of that used to happen in setup(). setup runs on the thread of the event pump.
            // Until it returns, webview resource requests cannot go out, and the window stays a
            // black screen with no explanation (a first launch measured more than 5 seconds).
            //
            // Now it runs on a worker thread: setup returns at once, and the event loop and page start first.
            // The frontend shows "Preparing local data". When ready, we broadcast `app://ready`,
            // the frontend invoke gate (`src/lib/bridge/tauri.ts::whenBackendReady`)
            // lets calls through, and the real queries start.
            let app_handle = app.handle().clone();
            let init_data_dir = data_dir.clone();
            let init_store_import_gate = store_import_gate.clone();
            let init_spawned =
                std::thread::Builder::new()
                    .name("app-init".into())
                    .spawn(move || {
                        if EXIT_GATE.is_exiting() {
                            return;
                        }
                        init_store_import_gate.wait_for_decision(|| EXIT_GATE.is_exiting());
                        if EXIT_GATE.is_exiting() {
                            return;
                        }
                        let stage_started = std::time::Instant::now();
                        // A queued restore must run before AppState is built: after that, desktop commands,
                        // the sync thread and the local API each hold connections, and swapping the file would conflict.
                        let restore_notice =
                            zeppbridge_core::storage::backup::apply_pending_restore(&init_data_dir)
                                .map(|outcome| outcome.message);
                        if stage_started.elapsed() >= Duration::from_secs(1) {
                            diagnostics::log(&format!(
                                "Startup: pending restore applied in {:.1}s",
                                stage_started.elapsed().as_secs_f64()
                            ));
                        }
                        let stage_started = std::time::Instant::now();
                        let state = match AppState::new(init_data_dir.clone()) {
                            Ok(state) => state,
                            Err(error) => {
                                // The window is drawn by now, but the backend cannot start, so the app
                                // still cannot work. Same fatal path: write startup-error.log,
                                // show the native dialog, then exit.
                                let _ = diagnostics::fatal_startup(
                                    &format!(
                                        "Cannot open the local database {}",
                                        init_data_dir.join("zepp.db").display()
                                    ),
                                    error,
                                );
                                app_handle.exit(1);
                                return;
                            }
                        };
                        diagnostics::log(&format!(
                            "Startup: database and application state ready in {:.1}s",
                            stage_started.elapsed().as_secs_f64()
                        ));
                        if EXIT_GATE.is_exiting() {
                            return;
                        }
                        if let Some(notice) = restore_notice {
                            state.push_startup_warning(notice);
                        }
                        // The local API is off by default on first install. `restore` only restores an enabled state that the user saved
                        // explicitly. A port conflict only puts the API in an error state. It does not block the desktop app from starting.
                        let local_api = std::sync::Arc::new(
                            zeppbridge_core::local_api::LocalApiController::new(
                                init_data_dir.clone(),
                            ),
                        );
                        if let Some(error) = local_api.restore().error {
                            diagnostics::log(&error);
                        }
                        app_handle.manage(local_api::LocalApi(local_api.clone()));
                        app_handle.manage(state);
                        // Raise the replay flag before broadcasting "ready" too. Before, it was raised only after the background thread started
                        // and counted all responses once, so the first automatic sync after launch could start before it,
                        // hit the replay writes halfway through the download, and waste the trip.
                        let replay_db = storage::Database::open_without_migration(
                            init_data_dir.join("zepp.db"),
                        )
                        .ok();
                        let replay_plan = replay_db
                            .as_ref()
                            .and_then(|db| db.pending_replay_plan().ok().flatten());
                        // Log why we replay: a full replay takes over a minute and sync yields meanwhile, so the user just sees "sync does nothing".
                        // A replay with an unchanged revision means some responses were not stamped. Log the count so we can find what was missed later.
                        if let (Some(db), Some(plan)) = (replay_db.as_ref(), replay_plan.as_ref()) {
                            diagnostics::log(&format!(
                                "Startup: replay planned (stored={}, target={}, raw_records={}, unstamped={})",
                                plan.stored_revision.as_deref().unwrap_or("none"),
                                plan.target_revision,
                                plan.raw_records,
                                db.unstamped_raw_count()
                                    .map(|count| count.to_string())
                                    .unwrap_or_else(|_| "?".into()),
                            ));
                        }
                        let replay_flag = replay_plan.map(|_| storage::ReplayGuard::enter());
                        // Set the flag first, then broadcast: the frontend registers the listener first, then asks `app_is_ready` once.
                        // The order must not be reversed. If reversed, there is a window where "the event was sent but the flag is not set",
                        // and that page waits until the 60-second timeout.
                        APP_READY.store(true, Ordering::SeqCst);
                        let _ = app_handle.emit("app://ready", ());

                        // After the parser revision changes, a background job replays the local raw responses once to correct derived
                        // data (workout types, sleep stages, and so on). A separate connection + background thread, so it does not block
                        // window creation.
                        //
                        // After the replay, also compact the existing raw responses. Both need the write lock, so they run in
                        // the same thread and do not fight each other. Neither may block window creation.
                        let compaction_handle = app_handle.clone();
                        let compaction_data_dir = init_data_dir.clone();
                        std::thread::spawn(move || {
                            if EXIT_GATE.is_exiting() {
                                return;
                            }
                            let Some(db) = replay_db else {
                                return;
                            };
                            // The write lock is taken per batch and released between batches (reprocess_raw_records_if_needed_batched):
                            // during a full replay of over ten minutes, short writes like settings changes and life events still get in.
                            if let Some(replay_flag) = replay_flag {
                                match db
                                    .reprocess_raw_records_if_needed_batched(&compaction_data_dir)
                                {
                                    Ok(Some(counts)) => {
                                        let total: i64 = counts.values().sum();
                                        diagnostics::log(&format!(
                                        "Normalizer upgraded, replayed local raw responses ({total} derived records)"
                                    ));
                                    }
                                    Ok(None) => {}
                                    Err(error) => {
                                        diagnostics::log(&format!("Local response replay failed: {error}"))
                                    }
                                }
                                drop(replay_flag);
                                // The sync that yielded during the replay waits for this: the frontend retries at once and does not wait a full minute.
                                let _ = compaction_handle.emit("replay://finished", ());
                            }

                            if EXIT_GATE.is_exiting() {
                                return;
                            }

                            // Compaction of existing responses: on by default, done once at the first launch after a new version is installed.
                            // Raw responses take the most space in the database (JSON text, compressed to about one fifth).
                            // If each person had to click it in advanced settings, most people would never compact.
                            //
                            // The UI shows "Compacting" through `compaction_in_progress()`, and it goes away when done.
                            // During this time, sync yields and retries automatically, as it does for a replay.
                            match db.pending_raw_payload_count() {
                                Ok(0) | Err(_) => {}
                                Ok(pending) => {
                                    let _compaction_flag = storage::CompactionGuard::enter();
                                    let _ = compaction_handle.emit("compaction://started", pending);
                                    // `let _write_guard = acquire_with_timeout(...)` binds
                                    // the whole `Result` to the variable: the `Err` returned when the lock does not come within 30 seconds
                                    // is also a value, so compaction would run anyway and the write lock would do nothing.
                                    // Only a matched `Ok` means we really got the lock.
                                    let Ok(_write_guard) =
                                        storage::write_lock::acquire_with_timeout(
                                            &compaction_data_dir,
                                            storage::write_lock::WritePurpose::Compaction,
                                            std::time::Duration::from_secs(30),
                                        )
                                    else {
                                        diagnostics::log("Skipped this response compaction: could not get the write lock");
                                        let _ = compaction_handle.emit(
                                            "compaction://finished",
                                            RawPayloadCompaction::default(),
                                        );
                                        return;
                                    };
                                    match db.compact_raw_payloads() {
                                        Ok(report) => {
                                            diagnostics::log(&format!(
                                                "Compacted {} historical responses, {} -> {} bytes",
                                                report.compacted,
                                                report.bytes_before,
                                                report.bytes_after
                                            ));
                                            let _ = compaction_handle
                                                .emit("compaction://finished", report);
                                        }
                                        Err(error) => {
                                            diagnostics::log(&format!("Historical response compaction failed: {error}"));
                                            let _ = compaction_handle.emit(
                                                "compaction://finished",
                                                RawPayloadCompaction::default(),
                                            );
                                        }
                                    }
                                }
                            }
                        });
                    });
            if let Err(error) = init_spawned {
                // If even the init thread cannot be spawned, the frontend stays on "Preparing" forever.
                return Err(diagnostics::fatal_startup("Cannot start the init thread", error).into());
            }

            main_window::initialize(app.handle());

            // The tray menu is native, and the UI i18n does not reach it, but English users
            // always right-click it. The tray must be built before the frontend loads, so we first
            // use the system language, and after the frontend sets the language we correct it with `set_tray_locale`.
            let labels = tray_i18n::tray_labels(&system_ui_locale().unwrap_or_default());
            let show = MenuItem::with_id(app, "show", labels.show, true, None::<&str>)?;
            let sync = MenuItem::with_id(app, "sync", labels.sync, true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", labels.quit, true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &sync, &quit])?;
            app.manage(TrayMenuItems {
                show: show.clone(),
                sync: sync.clone(),
                quit: quit.clone(),
            });
            let mut tray = TrayIconBuilder::new()
                .menu(&menu)
                .tooltip("ZeppBridge")
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => main_window::request_show(app, "tray-menu"),
                    "sync" => {
                        let _ = app.emit("tray://sync", ());
                    }
                    "quit" => {
                        diagnostics::log("Exit: tray menu \"Quit\"");
                        app.exit(0);
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let tauri::tray::TrayIconEvent::Click {
                        button: tauri::tray::MouseButton::Left,
                        button_state: tauri::tray::MouseButtonState::Up,
                        ..
                    } = event
                    {
                        main_window::request_show(tray.app_handle(), "tray-click");
                    }
                });
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }

            // If the tray cannot be built, the whole app must still start.
            //
            // On Linux the desktop environment provides the tray icon: Tauri uses libayatana-appindicator3,
            // but the GNOME Flatpak runtime does not have this library, and some distros do not install it on the desktop.
            // That C library calls `panic!` directly when it cannot find the .so (see the
            // `Failed to load ayatana-appindicator3 or appindicator3 dynamic library`
            // backtrace in issue #11), so here we must catch not only `Err` but also an unwind. If not, the user sees
            // "installed, and it crashes at launch", when the only missing piece is a decorative icon.
            let built = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| tray.build(app)));
            match built {
                Ok(Ok(_)) => {
                    app.state::<main_window::MainWindowState>()
                        .set_tray_present();
                    diagnostics::log("Startup: tray ready");
                }
                Ok(Err(error)) => {
                    diagnostics::log(&format!(
                        "Tray icon creation failed; the app keeps running (closing the window will exit): {error}"
                    ));
                }
                Err(_) => {
                    diagnostics::log(
                        "Tray icon creation crashed; the app keeps running (closing the window will exit).
                         On Linux this usually means libayatana-appindicator3 is missing:
                         Debian/Ubuntu: sudo apt install libayatana-appindicator3-1
                         Fedora: sudo dnf install libayatana-appindicator3
                         openSUSE: sudo zypper install libayatana-appindicator3-1",
                    );
                }
            }
            app.state::<main_window::MainWindowState>().set_ready();
            // Upstream auto-update is off in this fork: it would replace the redesign.
            #[cfg(feature = "github-updater")]
            let _ = updates::silent::spawn_periodic_recheck;
            auto_sync::spawn(app.handle().clone());
            diagnostics::log("Startup: application setup complete");
            Ok(())
        });
    #[cfg(feature = "github-updater")]
    let builder = builder
        .manage(updates::SilentUpdater::default())
        .plugin(tauri_plugin_updater::Builder::new().build());
    let app = builder
        .invoke_handler(tauri::generate_handler![
            ai_coach_ask,
            ai_coach_key_status,
            ai_coach_set_key,
            ai_coach_configure,
            ai_coach_status,
            strap_alerts,
            strap_daily,
            strap_key_fetch,
            strap_detail,
            daily_goals,
            strap_live_recent,
            strap_set,
            strap_buzz,
            strap_settings,
            strap_live_start,
            strap_live_stop,
            strap_sync_now,
            zebra_start_route,
            app_is_ready,
            store_import::store_import_status,
            store_import::store_import_decide,
            auto_sync::set_auto_sync_prefs,
            system_region_locale,
            list_life_events,
            save_life_event,
            delete_life_event,
            verify_auth,
            clear_auth,
            start_fresh_library,
            manual_auth,
            start_official_login,
            cancel_official_login,
            check_official_login_now,
            get_official_status,
            recover_official_credentials,
            disconnect_official,
            start_web_login,
            cancel_web_login,
            get_login_status,
            recover_advanced_credentials,
            start_history_sync,
            start_incremental_sync,
            cancel_sync,
            probe_data_capabilities,
            get_app_status,
            get_capability_overview,
            get_health_overview,
            get_heart_rate_series,
            get_stress_series,
            get_metric_baselines,
            metric_exclusions,
            set_metric_excluded,
            get_metric_series,
            get_hourly_steps,
            get_training_balance,
            get_heart_rate_zones,
            set_heart_rate_zone_preference,
            get_recent_sleep,
            get_recent_workouts,
            get_sleep_page,
            get_workout_page,
            get_daily_heart_rate_extremes,
            get_sleep_detail,
            get_sleep_nap_stages,
            get_workout_detail,
            get_workout_series,
            get_strength_catalog,
            get_device_profile,
            get_device_profiles,
            data_letters_state,
            data_letters_mail_status,
            data_letters_seal_capsule,
            data_letters_open_capsule,
            import_workouts,
            import_batches,
            undo_import_batch,
            workout_import_sources,
            export_for_unstrap,
            export_my_records,
            get_workout_fit_override,
            import_workout_fit,
            remove_workout_fit,
            set_workout_type_override,
            edit_workouts,
            undo_workout_edit,
            get_hidden_workout_page,
            get_workout_type_options,
            get_unknown_workout_codes,
            set_workout_code_label,
            set_device_hidden,
            set_device_model_override,
            get_data_health,
            run_database_integrity_check,
            get_workout_insight,
            get_weekly_report,
            reprocess_local_data,
            save_fit_export,
            copy_workout_gpx,
            prepare_ai_handoff,
            ai_task_day_strip,
            ai_exchange_list,
            ai_exchange_rename,
            ai_exchange_delete,
            ai_profile_save,
            training_plan_adherence,
            training_plan_update_draft,
            training_plan_history,
            workout_feedback_prompt,
            workout_feedback_get,
            workout_feedback_save,
            workout_feedback_dismiss,
            workout_feedback_snooze,
            workout_feedback_toast_due,
            workout_feedback_toast_show,
            workout_feedback_adjusted,
            coach_notes_list,
            coach_note_add,
            coach_note_decide,
            coach_note_edit,
            coach_note_delete,
            coach_notes_export,
            shoes_overview,
            shoe_save,
            shoe_delete,
            workout_feel_get,
            workout_feel_save,
            injuries_list,
            injury_save,
            injury_delete,
            races_list,
            race_save,
            race_delete,
            personal_records,
            annual_rings,
            rings_save_image,
            ai_task_list,
            ai_task_get,
            ai_task_save,
            ai_task_delete,
            ai_task_delete_many,
            ai_task_set_pinned,
            ai_prompt_versions,
            ai_prompt_export,
            ai_prompt_import,
            card_collection_get,
            card_collection_set,
            ai_template_list,
            ai_template_save,
            ai_template_delete,
            ai_task_preview,
            ai_task_prepare,
            ai_task_attachment_stat,
            ai_handoff_copy_file,
            ai_handoff_autopaste,
            ai_handoff_cancel,
            set_user_prefs,
            get_user_prefs,
            get_storage_estimate,
            cleanup_old_data,
            compact_raw_payloads,
            open_data_folder,
            open_external_url,
            #[cfg(feature = "github-updater")]
            updates::silent::update_start,
            #[cfg(feature = "github-updater")]
            updates::silent::update_view,
            #[cfg(feature = "github-updater")]
            updates::silent::update_set_auto,
            #[cfg(feature = "github-updater")]
            updates::silent::update_apply,
            list_backups,
            create_manual_backup,
            verify_backup,
            set_backup_pinned,
            get_restore_preview,
            stage_restore,
            get_pending_restore,
            cancel_pending_restore,
            start_history_backfill,
            get_workout_detail_coverage,
            start_workout_detail_backfill,
            get_coverage_ledger,
            reset_coverage_ledger,
            retry_failed_backfill_chunks,
            training_plan_state,
            training_plan_save_draft,
            training_plan_preview,
            training_plan_discard,
            training_plan_set_ai_publish,
            training_plan_publish,
            get_mcp_sidecar,
            cloud_companion_state,
            cloud_companion_save_config,
            cloud_companion_upload,
            cloud_companion_setup,
            cloud_companion_open,
            push_state,
            push_save,
            push_set_enabled,
            push_import_companion,
            push_run,
            push_remove,
            mcp_writes_list,
            mcp_write_undo,
            set_mcp_task_only,
            save_mcp_bundle,
            set_tray_locale,
            local_api::get_local_api_status,
            local_api::set_local_api_enabled,
            local_api::reveal_local_api_token,
            local_api::rotate_local_api_token,
        ])
        .build(tauri::generate_context!());
    match app {
        Ok(app) => app.run(|app, event| match event {
            tauri::RunEvent::Ready => {
                diagnostics::log("Startup: event loop ready");
                main_window::request_show(app, "startup");
            }
            // Every exit path writes one log line (#143): tao panics only after the event loop ends, so the log shows only
            // the panic, not who made it exit. `code` empty = the last window closed / the system asked to exit.
            // A value = code called `exit` (the tray "Quit" and the pre-update package swap each log a line first).
            tauri::RunEvent::ExitRequested { code, api, .. } => {
                diagnostics::log(&format!("Exit: exit requested (code {code:?})"));
                handle_exit_requested(app, code, &api);
            }
            tauri::RunEvent::Exit => {
                if let Some(api) = app.try_state::<local_api::LocalApi>() {
                    api.0.shutdown();
                }
                diagnostics::log("Exit: event loop ended");
            }
            _ => {}
        }),
        Err(error) => {
            // Builder/plugin failures otherwise disappear in Windows releases.
            // Configured-window failures inside run() are logged by the panic hook.
            let _ = diagnostics::fatal_startup("Tauri initialization failed", error);
        }
    }
}

#[cfg(all(test, windows))]
mod locale_tests {
    /// The native path to read the language must really work.
    ///
    /// The point of replacing `powershell (Get-Culture).Name` is to stop spawning a process. But if
    /// the buffer length or return value semantics are wrong, it quietly falls back to `None`, and the UI language detection
    /// goes wrong with it. No build turns red from this. So we run it once on the CI Windows runner.
    #[test]
    fn the_native_call_returns_a_locale_shaped_name() {
        let name = super::windows_ui_locale().expect("must read the UI language on Windows");
        assert!(
            name.len() >= 2 && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'),
            "the returned value does not look like a language tag: {name:?}"
        );
        assert!(!name.contains('\0'), "the final NUL was not removed: {name:?}");
    }
}

#[cfg(test)]
mod window_size_tests {
    use super::{main_window_size, MIN_WINDOW_HEIGHT, MIN_WINDOW_WIDTH};

    /// A normal 1080p monitor: the size fits inside the work area.
    #[test]
    fn a_normal_monitor_gets_a_window_that_fits_inside_it() {
        let (width, height) = main_window_size(1920, 1040, 1.0).expect("a normal monitor must give a size");
        assert!(width <= 1920.0 - 24.0, "width must not exceed the work area: {width}");
        assert!(height <= 1040.0 - 32.0, "height must not exceed the work area: {height}");
        assert!(width >= MIN_WINDOW_WIDTH && height >= MIN_WINDOW_HEIGHT);
    }

    /// This case is the whole reason for this fix.
    ///
    /// When `current_monitor()` gives `0x0`, the old code computed a width of -21.1,
    /// and `set_size` accepted a negative number. The window and tray were there, but clicking did nothing.
    #[test]
    fn a_zero_sized_work_area_falls_back_to_the_configured_size() {
        assert_eq!(main_window_size(0, 0, 1.0), None);
        assert_eq!(main_window_size(1920, 0, 1.0), None);
        assert_eq!(main_window_size(0, 1040, 1.0), None);
    }

    /// A broken scale factor must also not become a negative number or NaN.
    #[test]
    fn a_broken_scale_factor_falls_back_to_the_configured_size() {
        assert_eq!(main_window_size(1920, 1040, 0.0), None);
        assert_eq!(main_window_size(1920, 1040, -1.0), None);
        assert_eq!(main_window_size(1920, 1040, f64::NAN), None);
    }

    /// A work area narrower than the margins must also not give a negative number.
    #[test]
    fn a_work_area_smaller_than_the_margins_falls_back() {
        assert_eq!(main_window_size(20, 20, 1.0), None);
    }

    /// Small screen / high DPI: the computed value must not be below the minimum size in `tauri.conf.json`,
    /// or the window is too small to click, which is the same as "does not open".
    #[test]
    fn a_tiny_work_area_never_goes_below_the_configured_minimum() {
        let (width, height) = main_window_size(800, 600, 2.0).expect("this is still a usable monitor");
        assert!(width >= MIN_WINDOW_WIDTH, "width below the minimum: {width}");
        assert!(height >= MIN_WINDOW_HEIGHT, "height below the minimum: {height}");
    }
}
