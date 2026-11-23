//! The export subcommand: argument parsing, writing to disk, multi-file FIT (split out of main.rs, same logic).

use super::*;

#[derive(Debug)]
pub(super) struct ExportOptions {
    pub(super) format: String,
    pub(super) out: Option<String>,
    pub(super) selection: ExportSelection,
}

/// Arguments must pass validation before we open the database or produce output.
pub(super) fn parse_export_args(args: &[String]) -> Result<ExportOptions, String> {
    let flags = Flags::parse(args)?;
    flags.reject_unknown(&[
        "json", "format", "from", "to", "workout", "types", "detail", "out",
    ])?;
    let mut seen = BTreeSet::new();
    for (name, value) in &flags.values {
        if !seen.insert(name.as_str()) {
            return Err(format!("--{name} must not be repeated"));
        }
        if name == "json" {
            if value.is_some() {
                return Err("--json does not take a value".into());
            }
        } else if value.as_deref().is_none_or(|value| value.trim().is_empty()) {
            return Err(format!("--{name} needs a non-empty value"));
        }
    }

    let format = flags.get("format").unwrap_or("json");
    if !matches!(format, "json" | "csv" | "gpx" | "fit") {
        return Err("--format must be json, csv, gpx or fit".into());
    }
    if format == "fit" && !flags.has("out") {
        return Err(
            "--format fit needs --out to point at a directory: FIT is binary and one file per workout, so it cannot go to stdout"
                .into(),
        );
    }

    // Ranges are exclusive: a date range plus a single workout is a contradictory request. We set no priority and return an error.
    let scope = match (flags.get("from"), flags.get("to"), flags.get("workout")) {
        (Some(_), _, Some(_)) | (_, Some(_), Some(_)) => {
            return Err(
                "--from/--to and --workout are mutually exclusive ranges; give only one".into(),
            )
        }
        (Some(from), Some(to), None) => ExportScope::date_range(from, to),
        (Some(_), None, None) | (None, Some(_), None) => {
            return Err("--from and --to must be given together".into())
        }
        (None, None, Some(workout)) => ExportScope::Workout {
            workout_id: workout.to_string(),
        },
        (None, None, None) => {
            return Err("An export range is required: --from/--to or --workout".into())
        }
    }
    .validated()
    .map_err(translate_export_scope_error)?;

    let types: Vec<String> = flags
        .get("types")
        .unwrap_or("workouts,daily_activity,sleep")
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_ascii_lowercase)
        .collect();
    if types.is_empty() {
        return Err("--types must select at least one data type".into());
    }
    let unknown: Vec<&str> = types
        .iter()
        .map(String::as_str)
        .filter(|value| !EXPORT_DATA_TYPES.contains(value))
        .collect();
    if !unknown.is_empty() {
        return Err(format!(
            "--types does not accept {}; valid types are {}",
            unknown.join(", "),
            EXPORT_DATA_TYPES.join(", ")
        ));
    }
    let detail = match flags.get("detail").unwrap_or("summary") {
        "summary" => ExportDetail::Summary,
        "full" => ExportDetail::Full,
        _ => return Err("--detail must be summary or full".into()),
    };
    // Summary aggregates per-point metrics and leaves out workout tracks. File export needs the raw detail.
    let detail = if matches!(format, "csv" | "gpx" | "fit") {
        ExportDetail::Full
    } else {
        detail
    };

    Ok(ExportOptions {
        format: format.to_string(),
        out: flags.get("out").map(str::to_string),
        selection: ExportSelection {
            scope: Some(scope),
            start_date: None,
            end_date: None,
            data_types: types,
            detail,
        },
    })
}

pub(super) fn cmd_export(args: &[String]) -> u8 {
    // Parsing can also fail, so we cannot wait for Flags to build before we choose the error output format.
    let json_mode = scan_json_flag(args);
    let options = match parse_export_args(args) {
        Ok(options) => options,
        Err(message) => return fail(json_mode, EXIT_USAGE, "usage", &message),
    };
    let format = options.format.as_str();

    let db = match open_read_only() {
        Ok(db) => db,
        Err((code, message)) => return fail(json_mode, code, error_kind_for(code), &message),
    };
    // The export is derived data. If an old parser produced it, the user has a right to know before the file
    // is written. The notice goes to stderr, so the file from `export > a.csv` stays clean.
    if let Some(notice) = db
        .pending_replay_plan()
        .ok()
        .flatten()
        .as_ref()
        .and_then(|plan| pending_replay_notice(Some(plan)))
    {
        eprintln!("Note: {notice}");
    }
    // JSON uses `query_records`: the same entry point as the local REST `/v1/records` and the post-sync push.
    // The output is a superset of the old export (it adds records_version and a source per record), so old callers can still read it.
    // Other formats still use the export builder directly: they convert fixed columns and need no source label.
    let built = if format == "json" {
        records::query_records(&db, &records_query(&options.selection)).and_then(|set| {
            serde_json::to_string_pretty(&set.value)
                .map(|text| (text, set.record_count))
                .map_err(|error| ZeppBridgeError::ParseError(error.to_string()))
        })
    } else {
        db.build_ai_export(&options.selection)
    };
    // The same builder as the GUI: export semantics are implemented only once, in core.
    let (json_text, records) = match built {
        Ok(value) => value,
        Err(error) => {
            let (code, kind) = exit_code_for(&error);
            return fail(json_mode, code, kind, &user_text(&error));
        }
    };

    if format == "fit" {
        return export_fit_files(json_mode, &json_text, options.out.as_deref());
    }

    let (body, count) = match format {
        "json" => (json_text, records),
        other => {
            let parsed: serde_json::Value = match serde_json::from_str(&json_text) {
                Ok(value) => value,
                Err(error) => {
                    return fail(
                        json_mode,
                        EXIT_FAILED,
                        "failed",
                        &format!("The export result could not be parsed: {error}"),
                    )
                }
            };
            let converted = if other == "csv" {
                export_formats::to_csv(&parsed)
            } else {
                export_formats::to_gpx(&parsed)
            };
            match converted {
                Ok(value) => value,
                Err(message) => return fail(json_mode, EXIT_FAILED, "failed", &message),
            }
        }
    };

    match options.out.as_deref() {
        Some(path) => {
            if let Err(error) = paths::write_file_atomically(Path::new(path), body.as_bytes()) {
                return fail(
                    json_mode,
                    EXIT_FAILED,
                    "failed",
                    &format!("Could not write the file: {error}"),
                );
            }
            // Echo only the path the user gave. Do not resolve it to an absolute path and print that.
            emit(
                json_mode,
                serde_json::json!({ "ok": true, "format": format, "records": count, "out": path }),
                &format!("Exported {count} records to {path}"),
            );
        }
        None => {
            // Without --out the body has stdout to itself, and the count notice goes to stderr,
            // so `zeppbridge-cli export > a.csv` gives a clean file.
            print!("{body}");
            eprintln!("Exported {count} records ({format})");
        }
    }
    EXIT_OK
}

/// CLI export always included full tracks (with full detail), so GPS stays on here and the output does not change.
pub(super) fn records_query(selection: &ExportSelection) -> records::RecordQuery {
    records::RecordQuery {
        types: selection.data_types.clone(),
        scope: selection
            .scope
            .clone()
            .unwrap_or_else(|| ExportScope::date_range("", "")),
        since: None,
        detail: selection.detail,
        include_gps: true,
    }
}

/// Lets `WriteLockError` also map to the busy code.
pub(super) fn write_lock_exit(error: &WriteLockError) -> u8 {
    match error {
        WriteLockError::Busy { .. } => EXIT_BUSY,
        WriteLockError::Unavailable(_) => EXIT_FAILED,
    }
}

/// FIT export: one file per workout, all written to the directory that `out` points to.
///
/// Why no stdout: FIT is binary, and one export is usually many files. If we join them
/// into one stream, no one on either end can split them again. So we require `--out` and do not quietly write something
/// that nobody can use.
pub(super) fn export_fit_files(json_mode: bool, json_text: &str, out: Option<&str>) -> u8 {
    let Some(directory) = out else {
        return fail(
            json_mode,
            EXIT_USAGE,
            "usage",
            "--format fit needs --out to point at a directory: FIT is binary and one file per workout, so it cannot go to stdout",
        );
    };

    let parsed: serde_json::Value = match serde_json::from_str(json_text) {
        Ok(value) => value,
        Err(error) => {
            return fail(
                json_mode,
                EXIT_FAILED,
                "failed",
                &format!("The export result could not be parsed: {error}"),
            )
        }
    };

    let (files, points) = match export_fit::to_fit(&parsed) {
        Ok(value) => value,
        Err(message) => return fail(json_mode, EXIT_FAILED, "failed", &message),
    };

    if let Err(error) = std::fs::create_dir_all(directory) {
        return fail(
            json_mode,
            EXIT_FAILED,
            "failed",
            &format!("Could not create the directory: {error}"),
        );
    }
    for (name, bytes) in &files {
        let target = std::path::Path::new(directory).join(name);
        if let Err(error) = paths::write_file_atomically(&target, bytes) {
            return fail(
                json_mode,
                EXIT_FAILED,
                "failed",
                &format!("Could not write the file: {error}"),
            );
        }
    }

    emit(
        json_mode,
        serde_json::json!({
            "ok": true,
            "format": "fit",
            "files": files.len(),
            "records": points,
            "out": directory
        }),
        &format!(
            "Exported {} FIT files ({points} sample points in total) to {directory}",
            files.len()
        ),
    );
    EXIT_OK
}
