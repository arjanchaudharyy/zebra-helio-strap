//! Export commands: write FIT files from workout details, build the AI handoff package, and check the export folder.
//!
//! Only the old Explore page used the JSON / CSV / GPX write commands, so they were removed with it.
//! The CLI `export` calls core directly and does not use this file.

use super::*;

/// Uses the same converter as the CLI, so the desktop workout details can copy GPX.
#[tauri::command]
pub async fn copy_workout_gpx(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    workout_id: String,
) -> std::result::Result<(), AppError> {
    use tauri_plugin_clipboard_manager::ClipboardExt as _;
    let (data, _) = spawn_independent_read(state.data_dir.clone(), move |db| {
        db.build_ai_export_value(&ExportSelection {
            scope: Some(zeppbridge_core::models::ExportScope::Workout { workout_id }),
            start_date: None,
            end_date: None,
            data_types: vec!["workouts".to_owned()],
            detail: ExportDetail::Full,
        })
    })
    .await?;
    let (text, _) = zeppbridge_core::export_formats::to_gpx(&data)
        .map_err(|message| AppError::new("err.workout.gpx_no_route", message))?;
    app.clipboard()
        .write_text(text)
        .map_err(|_| AppError::new("err.workout.gpx_clipboard", "Could not copy GPX to the clipboard"))
}

/// Writes the selected workouts as FIT files, one file for each workout, all in `directory`.
///
/// Why a folder and not one file: by convention a FIT activity file holds one activity, and
/// Garmin Connect and Strava do not accept chained FIT files with many activities in the same way.
/// The request was to "get all past workouts at once", and a folder gives exactly that.
///
/// `record_count` is the total number of per-second samples in all files. A workout with no detail series
/// makes no file. If no file can be made, that is an error, not an empty folder.
#[tauri::command]
pub async fn save_fit_export(
    state: tauri::State<'_, AppState>,
    selection: ExportSelection,
    directory: String,
) -> std::result::Result<ExportResult, AppError> {
    let directory = validate_export_directory(&directory)?;
    // `mut` cannot be in a `#[tauri::command]` parameter pattern: the macro then cannot infer the parameter
    // type, and the compiler reports a never-type fallback. Rebind it in the function body.
    let mut selection = selection;

    // The per-second series is the whole point of an archive format like FIT, so this reads the full
    // payload, whatever the UI selection is.
    selection.detail = ExportDetail::Full;
    // Read only: use the separate read-only connection, so this does not wait behind the command-side write lock.
    let (export, record_count) = spawn_independent_read(state.data_dir.clone(), move |db| {
        db.build_ai_export_value(&selection)
    })
    .await?;
    if record_count == 0 {
        return Err(AppError::new(
            "err.export.empty_range",
            "No records to export in this time range",
        ));
    }

    let (files, point_count) = export_fit::to_fit(&export).map_err(|message| {
        AppError::new("err.export.convert_failed", message)
            .with_params(serde_json::json!({ "format": "FIT" }))
    })?;

    std::fs::create_dir_all(&directory).map_err(|error| {
        AppError::new(
            "err.export.write_failed",
            format!("Could not create the FIT export folder: {error}"),
        )
        .with_params(serde_json::json!({ "format": "FIT" }))
    })?;

    let mut bytes = 0usize;
    for (name, content) in &files {
        let target = directory.join(name);
        write_file_atomically(&target, content).map_err(|error| {
            AppError::new(
                "err.export.write_failed",
                format!("Could not write the FIT export: {error}"),
            )
            .with_params(serde_json::json!({ "format": "FIT" }))
        })?;
        bytes += content.len();
    }

    Ok(ExportResult {
        path: directory.to_string_lossy().into_owned(),
        record_count: point_count,
        bytes,
        generated_at: Utc::now().to_rfc3339(),
        file_count: Some(files.len()),
    })
}

/// Prepare a privacy-preserving payload for an external AI provider.
///
/// This deliberately calls the same database export builder as the FIT export
/// and the CLI, then applies a second, recursive redaction pass.
#[tauri::command]
pub async fn prepare_ai_handoff(
    state: tauri::State<'_, AppState>,
    selection: ExportSelection,
    prompt: String,
    include_precise_route: Option<bool>,
) -> std::result::Result<AiHandoffResult, AppError> {
    let prompt = sanitize_clipboard_text(prompt.trim());
    if prompt.is_empty() {
        return Err(AppError::new(
            "err.handoff.prompt_required",
            "Enter a prompt first",
        ));
    }

    // Metrics the user excludes (metric_prefs) do not go to AI. The user's own exports (FIT / CLI) are not affected.
    let (export, record_count) = spawn_independent_read(state.data_dir.clone(), move |db| {
        let (mut export, count) = db.build_ai_export_value(&selection)?;
        zeppbridge_core::storage::metric_prefs::apply_exclusions(
            &mut export,
            &db.excluded_metrics()?,
        );
        Ok((export, count))
    })
    .await?;
    if record_count == 0 {
        return Err(AppError::new(
            "err.handoff.empty_range",
            "No records to hand off in this time range",
        ));
    }

    let include_precise_route = include_precise_route.unwrap_or(false);
    let (redacted, redactions) = redact_ai_export(export, include_precise_route)?;
    let bytes = redacted.len();
    let mode = ai_handoff_mode_for_bytes(bytes);
    let (clipboard_text, file_path) = if mode == "inline" {
        (
            format!("{prompt}\n\nRedacted health data (JSON) follows:\n{redacted}"),
            None,
        )
    } else {
        let target_dir = directories::UserDirs::new()
            .and_then(|u| u.desktop_dir().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| state.data_dir.join("exports"));
        std::fs::create_dir_all(&target_dir).map_err(|error| {
            AppError::new(
                "err.handoff.mkdir_failed",
                format!("Could not create the data package export folder: {error}"),
            )
        })?;
        let path = target_dir.join("zeppbridge-ai-handoff.json");
        write_file_atomically(&path, redacted.as_bytes()).map_err(|error| {
            AppError::new(
                "err.handoff.write_failed",
                format!("Could not write the redacted AI data to the desktop: {error}"),
            )
        })?;
        (
            format!("{prompt}\n\nThe data package is on the desktop (zeppbridge-ai-handoff.json). Drag it into the AI chat."),
            Some(path.to_string_lossy().into_owned()),
        )
    };

    Ok(AiHandoffResult {
        mode: mode.to_string(),
        clipboard_text,
        file_path,
        bytes,
        records: record_count,
        redactions,
        metadata: AiHandoffMetadata {
            precise_route_included: include_precise_route,
            authentication_fields_removed: true,
            identity_fields_removed: true,
        },
    })
}

pub(crate) fn ai_handoff_mode_for_bytes(bytes: usize) -> &'static str {
    if bytes <= AI_HANDOFF_INLINE_LIMIT_BYTES {
        "inline"
    } else {
        "attachment"
    }
}

pub(super) fn write_file_atomically(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    crate::paths::write_file_atomically(path, bytes)
}

/// The target folder for the FIT export.
///
/// Does only clear checks: not empty, an absolute path, not a file. It intentionally
/// does not require the folder to exist. Users often make a new folder in the save dialog, and the write
/// creates the folder.
pub(super) fn validate_export_directory(value: &str) -> std::result::Result<PathBuf, AppError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(
            AppError::new("err.export.path_required", "Choose a folder to save the FIT files")
                .with_params(serde_json::json!({ "format": "FIT" })),
        );
    }
    let path = PathBuf::from(trimmed);
    if !path.is_absolute() {
        return Err(AppError::new(
            "err.export.path_not_absolute",
            "The save location must be an absolute path",
        ));
    }
    if path.is_file() {
        return Err(AppError::new(
            "err.export.not_a_directory",
            "The FIT export needs a folder, but a file is selected",
        ));
    }
    Ok(path)
}
