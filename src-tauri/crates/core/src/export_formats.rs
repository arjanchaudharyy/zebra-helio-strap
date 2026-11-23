//! CSV / GPX conversion.
//!
//! Input is always the normalized JSON from `Database::build_ai_export`. Both converters
//! only reorder. They do not query the database again and add no derived values.
//!
//! Two hard rules:
//! 1. Output only values that really exist in the local database. A missing field gives no row and no tag, never filled with 0.
//! 2. With nothing to output, return an error. Do not write a "header only" or "empty gpx" file,
//!    so the user does not think the export worked.

use chrono::{DateTime, FixedOffset};
use serde_json::Value;

/// CSV uses a long (tidy) table, not a wide table: the four record types have completely different field sets, so a wide table forces many
/// empty columns, while a long table naturally says "this record has no such metric".
/// The device_id column name stays for compatibility with existing CSV. Its value is the anonymous device_label from the builder.
const CSV_HEADER: &str =
    "record_type,record_id,start_time,end_time,metric,value,unit,source_scope,device_id";

/// Excel on Simplified Chinese Windows reads CSV with the local code page by default. Without a BOM, Chinese text is garbled.
const UTF8_BOM: &str = "\u{feff}";

/// Scalar sleep fields that can expand into metric rows, with their units.
const SLEEP_METRICS: [(&str, &str); 6] = [
    ("duration_minutes", "minutes"),
    ("score", "score"),
    ("deep_minutes", "minutes"),
    ("light_minutes", "minutes"),
    ("rem_minutes", "minutes"),
    ("awake_minutes", "minutes"),
];

/// Scalar workout fields that can expand into metric rows, with their units.
const WORKOUT_METRICS: [(&str, &str); 6] = [
    ("distance_meters", "meters"),
    ("calories", "kcal"),
    ("avg_hr", "bpm"),
    ("max_hr", "bpm"),
    ("training_load", ""),
    ("vo2max", "ml/kg/min"),
];

/// Converts the normalized export JSON into long-table CSV. Returns `(text, data row count)`.
///
/// Daily metrics use the per-entry values in the full payload. Per-second workout series and GPS tracks do not go into the CSV.
pub fn to_csv(export: &Value) -> Result<(String, usize), String> {
    let data = export
        .get("data")
        .ok_or_else(|| "export data structure is invalid: missing data section".to_string())?;

    let mut rows = String::new();
    let mut count = 0usize;

    for event in array(data, "life_events") {
        for field in ["title", "category", "notes"] {
            let value = text(event.get(field));
            push_row(
                &mut rows,
                &[
                    "life_event",
                    &event["id"].to_string(),
                    text(event.get("startDate")),
                    text(event.get("endDate")),
                    field,
                    value,
                    "",
                    "user_authored",
                    "",
                ],
            );
            count += 1;
        }
    }

    for sample in array(data, "metric_samples") {
        let Some(value) = number_text(sample.get("value")) else {
            continue;
        };
        push_numeric_row(
            &mut rows,
            &[
                "metric_sample",
                "",
                text(sample.get("timestamp")),
                "",
                text(sample.get("metric")),
                &value,
                text(sample.get("unit")),
                text(sample.get("source_scope")),
                text(sample.get("device_label")),
            ],
        );
        count += 1;
    }

    for daily in array(data, "daily_metrics") {
        let Some(value) = number_text(daily.get("value")) else {
            continue;
        };
        push_numeric_row(
            &mut rows,
            &[
                "daily_metric",
                "",
                text(daily.get("date")),
                "",
                text(daily.get("metric")),
                &value,
                text(daily.get("unit")),
                text(daily.get("source_scope")),
                text(daily.get("device_label")),
            ],
        );
        count += 1;
    }

    for session in array(data, "sleep_sessions") {
        for (metric, unit) in SLEEP_METRICS {
            let Some(value) = number_text(session.get(metric)) else {
                continue;
            };
            push_numeric_row(
                &mut rows,
                &[
                    "sleep_session",
                    text(session.get("sleep_id")),
                    text(session.get("start_time")),
                    text(session.get("end_time")),
                    metric,
                    &value,
                    unit,
                    text(session.get("source_scope")),
                    text(session.get("device_label")),
                ],
            );
            count += 1;
        }
    }

    for workout in array(data, "workouts") {
        let workout_id = text(workout.get("workout_id"));
        let start = text(workout.get("start_time"));
        let end = text(workout.get("end_time"));
        let scope = text(workout.get("source_scope"));
        let device = text(workout.get("device_label"));

        // Workout type is a string, not a number. But dropping it would make the CSV unable to tell running from cycling,
        // so it gets its own row in the value column.
        let workout_type = text(
            workout
                .get("effective_type")
                .or_else(|| workout.get("workout_type")),
        );
        if !workout_type.is_empty() {
            push_row(
                &mut rows,
                &[
                    "workout",
                    workout_id,
                    start,
                    end,
                    "workout_type",
                    workout_type,
                    "",
                    scope,
                    device,
                ],
            );
            count += 1;
        }

        for (metric, unit) in WORKOUT_METRICS {
            let Some(value) = number_text(workout.get(metric)) else {
                continue;
            };
            push_numeric_row(
                &mut rows,
                &[
                    "workout", workout_id, start, end, metric, &value, unit, scope, device,
                ],
            );
            count += 1;
        }

        if let Some(strength) = workout.get("strength").and_then(Value::as_object) {
            for (field, metric, unit) in [
                ("total_sets", "strength_total_sets", "sets"),
                ("total_reps", "strength_total_reps", "reps"),
                ("total_volume_kg", "strength_total_volume_kg", "kg"),
                ("rest_seconds", "strength_rest_seconds", "seconds"),
            ] {
                let Some(value) = number_text(strength.get(field)) else {
                    continue;
                };
                push_numeric_row(
                    &mut rows,
                    &[
                        "workout", workout_id, start, end, metric, &value, unit, scope, device,
                    ],
                );
                count += 1;
            }
            if let Some(sets) = strength.get("sets").and_then(Value::as_array) {
                for set in sets {
                    let set_idx = number_text(set.get("set_index")).unwrap_or_default();
                    let set_id = format!("{workout_id}#set{set_idx}");
                    // Exercise key (old exercises) and the English name in the Zepp catalog (new exercises from 1001 have only that).
                    for field in ["exercise_key", "exercise_name"] {
                        let value = text(set.get(field));
                        if value.is_empty() {
                            continue;
                        }
                        push_row(
                            &mut rows,
                            &[
                                "workout_strength_set",
                                &set_id,
                                start,
                                end,
                                field,
                                value,
                                "",
                                scope,
                                device,
                            ],
                        );
                        count += 1;
                    }
                    for (field, unit) in [
                        ("action_code", ""),
                        ("reps", "reps"),
                        ("weight_kg", "kg"),
                        ("duration_ms", "ms"),
                        ("avg_hr", "bpm"),
                        ("calories_kcal", "kcal"),
                    ] {
                        let Some(value) = number_text(set.get(field)) else {
                            continue;
                        };
                        push_numeric_row(
                            &mut rows,
                            &[
                                "workout_strength_set",
                                &set_id,
                                start,
                                end,
                                field,
                                &value,
                                unit,
                                scope,
                                device,
                            ],
                        );
                        count += 1;
                    }
                }
            }
        }
    }

    if count == 0 {
        return Err("no records to write to CSV for this period".to_string());
    }

    Ok((format!("{UTF8_BOM}{CSV_HEADER}\n{rows}"), count))
}

/// Converts the normalized export JSON into GPX 1.1. Returns `(XML, track point count)`.
///
/// Only workouts with real decoded GPS points produce a `<trk>`. With no point at all, return an error
/// and do not produce an empty track file. Heart rate is written into Garmin `TrackPointExtension` only when the timestamp matches the per-point sample **exactly**.
/// No nearest match or interpolation.
pub fn to_gpx(export: &Value) -> Result<(String, usize), String> {
    let data = export
        .get("data")
        .ok_or_else(|| "export data structure is invalid: missing data section".to_string())?;

    let mut tracks = String::new();
    let mut points = 0usize;

    for workout in array(data, "workouts") {
        let route = array(workout, "route");
        if route.is_empty() {
            continue;
        }

        let heart_rates = heart_rate_index(workout);
        let resumes = pause_resume_times(workout);

        let workout_type = text(
            workout
                .get("effective_type")
                .or_else(|| workout.get("workout_type")),
        );
        let start = text(workout.get("start_time"));
        let name = if start.is_empty() {
            workout_type.to_string()
        } else {
            format!("{workout_type} {start}").trim().to_string()
        };

        tracks.push_str("  <trk>\n");
        tracks.push_str(&format!("    <name>{}</name>\n", escape_xml(&name)));
        if !workout_type.is_empty() {
            tracks.push_str(&format!("    <type>{}</type>\n", escape_xml(workout_type)));
        }
        tracks.push_str(&format!(
            "    <desc>workout_id: {}</desc>\n",
            escape_xml(text(workout.get("workout_id")))
        ));
        tracks.push_str("    <trkseg>\n");

        let mut previous: Option<DateTime<FixedOffset>> = None;
        for point in route {
            let (Some(lat), Some(lon)) = (
                point.get("latitude").and_then(Value::as_f64),
                point.get("longitude").and_then(Value::as_f64),
            ) else {
                continue;
            };
            // Last boundary: out-of-range or non-finite coordinates are not written. The decoder already clamps them.
            // This is an independent check that does not trust the upstream to be clean.
            if !coordinates_in_domain(lat, lon) {
                continue;
            }
            let timestamp = text(point.get("timestamp"));
            let parsed = parse_time(timestamp);

            // Points recorded after a pause belong to a new trkseg. Otherwise the track would cross the rest time
            // as a false straight line.
            if let (Some(previous), Some(current)) = (previous, parsed) {
                if point.get("segment_start").and_then(Value::as_bool) == Some(true)
                    || resumes
                        .iter()
                        .any(|resume| *resume > previous && *resume <= current)
                {
                    tracks.push_str("    </trkseg>\n    <trkseg>\n");
                }
            }
            if parsed.is_some() {
                previous = parsed;
            }

            tracks.push_str(&format!(
                "      <trkpt lat=\"{lat:.6}\" lon=\"{lon:.6}\">\n"
            ));
            if let Some(altitude) = point.get("altitude_m").and_then(Value::as_f64) {
                tracks.push_str(&format!("        <ele>{altitude:.1}</ele>\n"));
            }
            if !timestamp.is_empty() {
                tracks.push_str(&format!("        <time>{}</time>\n", escape_xml(timestamp)));
            }
            if let Some(hr) = heart_rates
                .iter()
                .find(|(sample_time, _)| *sample_time == timestamp)
                .map(|(_, hr)| *hr)
            {
                tracks.push_str("        <extensions>\n          <gpxtpx:TrackPointExtension>\n");
                tracks.push_str(&format!("            <gpxtpx:hr>{hr}</gpxtpx:hr>\n"));
                tracks.push_str("          </gpxtpx:TrackPointExtension>\n        </extensions>\n");
            }
            tracks.push_str("      </trkpt>\n");
            points += 1;
        }

        tracks.push_str("    </trkseg>\n  </trk>\n");
    }

    if points == 0 {
        return Err("no GPS track to export for this period (only workouts with track points can produce GPX)".to_string());
    }

    let generated_at = text(export.get("generated_at"));
    let mut gpx = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    gpx.push_str(
        "<gpx version=\"1.1\" creator=\"ZeppBridge\" \
xmlns=\"http://www.topografix.com/GPX/1/1\" \
xmlns:gpxtpx=\"http://www.garmin.com/xmlschemas/TrackPointExtension/v1\">\n",
    );
    if !generated_at.is_empty() {
        gpx.push_str(&format!(
            "  <metadata>\n    <time>{}</time>\n  </metadata>\n",
            escape_xml(generated_at)
        ));
    }
    gpx.push_str(&tracks);
    gpx.push_str("</gpx>\n");

    Ok((gpx, points))
}

fn array<'a>(parent: &'a Value, key: &str) -> &'a [Value] {
    parent
        .get(key)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

/// String field: `null` and non-strings are all empty. The caller decides whether to skip.
fn text(value: Option<&Value>) -> &str {
    value.and_then(Value::as_str).unwrap_or("")
}

/// Number field: `null` / missing / non-number returns `None`, so the caller skips the whole row and does not write 0.
fn number_text(value: Option<&Value>) -> Option<String> {
    let number = value?.as_f64()?;
    if !number.is_finite() {
        return None;
    }
    if number.fract() == 0.0 && number.abs() < 1e15 {
        Some(format!("{}", number as i64))
    } else {
        Some(format!("{number}"))
    }
}

fn heart_rate_index(workout: &Value) -> Vec<(&str, i64)> {
    array(workout, "samples")
        .iter()
        .filter_map(|sample| {
            let timestamp = sample.get("timestamp")?.as_str()?;
            let heart_rate = sample.get("heart_rate")?.as_i64()?;
            Some((timestamp, heart_rate))
        })
        .collect()
}

fn pause_resume_times(workout: &Value) -> Vec<DateTime<FixedOffset>> {
    array(workout, "pauses")
        .iter()
        .filter_map(|pause| parse_time(text(pause.get("end_time"))))
        .collect()
}

fn parse_time(value: &str) -> Option<DateTime<FixedOffset>> {
    DateTime::parse_from_rfc3339(value).ok()
}

/// Coordinate range: latitude +-90, longitude +-180, and both must be finite.
fn coordinates_in_domain(latitude: f64, longitude: f64) -> bool {
    latitude.is_finite()
        && longitude.is_finite()
        && latitude.abs() <= 90.0
        && longitude.abs() <= 180.0
}

fn push_row(out: &mut String, fields: &[&str; 9]) {
    push_csv_row(out, fields, false);
}

// Only value cells produced by number_text may bypass text protection.
fn push_numeric_row(out: &mut String, fields: &[&str; 9]) {
    push_csv_row(out, fields, true);
}

fn push_csv_row(out: &mut String, fields: &[&str; 9], numeric_value: bool) {
    for (index, field) in fields.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        let protected;
        let value: &str = if !(numeric_value && index == 5)
            && (field.trim_start().starts_with(['=', '+', '-', '@'])
                || field.starts_with(['\t', '\r', '\n']))
        {
            protected = format!("'{field}");
            &protected
        } else {
            field
        };
        out.push_str(&escape_csv(value));
    }
    out.push('\n');
}

fn escape_csv(value: &str) -> String {
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ExportDetail, ExportScope, ExportSelection};
    use crate::storage::Database;
    use serde_json::json;

    fn export_with(data: Value) -> Value {
        json!({ "generated_at": "2026-08-24T10:00:00+08:00", "data": data })
    }

    #[test]
    fn regression_markdown_csv_protects_all_text_without_changing_numbers() {
        for dangerous in [
            "=1+1", "+1+1", "-1+1", "@SUM(A1)", "  =1+1", "\ttext", "\rtext", "\ntext", "=\"a,b\"",
        ] {
            let export = export_with(json!({
                "metric_samples": [{"timestamp": dangerous, "metric": dangerous, "value": -2.5, "unit": dangerous, "source_scope": dangerous, "device_label": dangerous}],
                "daily_metrics": [{"date": dangerous, "metric": dangerous, "value": -3, "unit": dangerous, "source_scope": dangerous, "device_label": dangerous}],
                "sleep_sessions": [{"sleep_id": dangerous, "start_time": dangerous, "end_time": dangerous, "score": -4, "source_scope": dangerous, "device_label": dangerous}],
                "workouts": [{"workout_id": dangerous, "start_time": dangerous, "end_time": dangerous, "effective_type": dangerous, "calories": -5, "source_scope": dangerous, "device_label": dangerous}],
                "life_events": [{"id": 1, "title": dangerous, "category": dangerous, "notes": dangerous}]
            }));
            let (csv, count) = to_csv(&export).unwrap();
            assert_eq!(count, 8);
            let safe = escape_csv(&format!("'{dangerous}"));
            assert!(csv.contains(&format!(
                "metric_sample,,{safe},,{safe},-2.5,{safe},{safe},{safe}\n"
            )));
            assert!(csv.contains(&format!(
                "daily_metric,,{safe},,{safe},-3,{safe},{safe},{safe}\n"
            )));
            assert!(csv.contains(&format!(
                "sleep_session,{safe},{safe},{safe},score,-4,score,{safe},{safe}\n"
            )));
            assert!(csv.contains(&format!(
                "workout,{safe},{safe},{safe},workout_type,{safe},,{safe},{safe}\n"
            )));
            assert!(csv.contains(&format!(
                "workout,{safe},{safe},{safe},calories,-5,kcal,{safe},{safe}\n"
            )));
            assert!(csv.contains(&format!("life_event,1,,,title,{safe},,user_authored,\n")));
        }
    }

    #[test]
    fn csv_skips_missing_values_instead_of_writing_zero() {
        let export = export_with(json!({
            "metric_samples": [
                { "metric": "heart_rate", "timestamp": "2026-08-24T09:00:00+08:00",
                  "value": 72, "unit": "bpm", "source_scope": "device", "device_id": null },
                { "metric": "heart_rate", "timestamp": "2026-08-24T09:01:00+08:00",
                  "value": null, "unit": "bpm", "source_scope": "device", "device_id": null }
            ],
            "sleep_sessions": [
                { "sleep_id": "s1", "start_time": "2026-08-23T23:00:00+08:00",
                  "end_time": "2026-08-24T07:00:00+08:00", "score": null,
                  "duration_minutes": 480, "rem_minutes": null, "source_scope": "device" }
            ]
        }));

        let (csv, rows) = to_csv(&export).unwrap();

        assert_eq!(rows, 2, "empty values must skip the whole row, never filled with 0");
        assert!(
            csv.starts_with(UTF8_BOM),
            "without a BOM, Excel garbles Chinese text"
        );
        assert!(csv.contains("metric_sample,,2026-08-24T09:00:00+08:00,,heart_rate,72,bpm,device,"));
        assert!(csv.contains("sleep_session,s1,"));
        assert!(!csv.contains(",score,"), "a missing score must not appear");
        assert!(!csv.contains(",rem_minutes,"), "REM that was not given must not appear");
    }

    #[test]
    fn csv_quotes_fields_containing_separators() {
        let export = export_with(json!({
            "workouts": [
                { "workout_id": "w,1", "workout_type": "Outdoor run \"morning\"",
                  "start_time": "2026-08-24T06:00:00+08:00", "end_time": "2026-08-24T07:00:00+08:00",
                  "distance_meters": 5012.5, "calories": null, "source_scope": "device" }
            ]
        }));

        let (csv, rows) = to_csv(&export).unwrap();

        assert_eq!(rows, 2, "type row + distance row. Missing calories give no row");
        assert!(csv.contains("\"w,1\""), "an ID with a comma must be quoted");
        assert!(
            csv.contains("\"Outdoor run \"\"morning\"\"\""),
            "quotes must be escaped in pairs"
        );
        assert!(csv.contains(",distance_meters,5012.5,meters,"));
    }

    #[test]
    fn csv_refuses_to_write_a_header_only_file() {
        let export = export_with(json!({ "metric_samples": [], "workouts": [] }));
        let error = to_csv(&export).unwrap_err();
        assert!(error.contains("no records to write to CSV"), "actual error: {error}");
    }

    #[test]
    fn csv_preserves_anonymous_device_labels_from_the_export_builder() {
        let db = Database::in_memory().unwrap();
        db.conn
            .execute_batch(
                "INSERT INTO device_identities (alias, serial, device_id, updated_at)
                 VALUES ('private-device-id', 'private-serial', 'private-device-id',
                         '2023-11-05T12:00:00+00:00');
                 INSERT INTO metric_samples
                     (metric, timestamp, value, unit, source_scope, device_id)
                 VALUES ('heart_rate', '2023-11-05T12:00:00+00:00', 72, 'bpm',
                         'device', 'private-device-id');
                 INSERT INTO daily_metrics
                     (date, metric, value, unit, source_scope, device_id)
                 VALUES ('2023-11-05', 'steps', 1234, 'steps', 'device', 'private-device-id');
                 INSERT INTO sleep_sessions
                     (sleep_id, start_time, end_time, duration_minutes, deep_minutes,
                      light_minutes, rem_minutes, awake_minutes, source_scope, device_id)
                 VALUES ('s1', '2023-11-05T00:00:00+00:00', '2023-11-05T07:00:00+00:00',
                         420, 60, 360, 0, 0, 'device', 'private-device-id');
                 INSERT INTO workouts
                     (workout_id, workout_type, start_time, end_time, source_scope, device_id)
                 VALUES ('w1', 'run', '2023-11-05T12:00:00+00:00',
                         '2023-11-05T13:00:00+00:00', 'device', 'private-device-id');",
            )
            .unwrap();
        let selection = ExportSelection {
            scope: Some(ExportScope::date_range("2023-11-01", "2023-11-30")),
            start_date: None,
            end_date: None,
            data_types: ["heart_rate", "steps", "sleep", "workouts"]
                .into_iter()
                .map(str::to_string)
                .collect(),
            detail: ExportDetail::Full,
        };
        let (encoded, _) = db.build_ai_export(&selection).unwrap();
        let export = serde_json::from_str(&encoded).unwrap();
        let (csv, _) = to_csv(&export).unwrap();

        // Keep the existing column contract, but only put the builder's anonymous
        // label in it. Raw device identifiers must never return via CSV.
        assert_eq!(
            csv.lines().next().unwrap(),
            format!("{UTF8_BOM}{CSV_HEADER}")
        );
        for record_type in ["metric_sample", "daily_metric", "sleep_session", "workout"] {
            let rows: Vec<_> = csv
                .lines()
                .filter(|line| line.starts_with(&format!("{record_type},")))
                .collect();
            assert!(!rows.is_empty(), "missing {record_type}");
            assert!(
                rows.iter().all(|line| line.ends_with(",device_1")),
                "missing device label for {record_type}: {rows:?}"
            );
        }
        assert!(!csv.contains("private-device-id"));
        assert!(!csv.contains("private-serial"));
    }

    #[test]
    fn gpx_emits_points_and_only_matches_heart_rate_on_exact_timestamps() {
        let export = export_with(json!({
            "workouts": [
                {
                    "workout_id": "w1", "workout_type": "outdoor_running",
                    "start_time": "2026-08-24T06:00:00+08:00",
                    "route": [
                        { "timestamp": "2026-08-24T06:00:00+08:00", "latitude": 31.2304,
                          "longitude": 121.4737, "altitude_m": 12.4 },
                        { "timestamp": "2026-08-24T06:00:01+08:00", "latitude": 31.2305,
                          "longitude": 121.4738, "altitude_m": null }
                    ],
                    "samples": [
                        { "timestamp": "2026-08-24T06:00:00+08:00", "heart_rate": 132 },
                        { "timestamp": "2026-08-24T06:00:09+08:00", "heart_rate": 150 }
                    ],
                    "pauses": []
                }
            ]
        }));

        let (gpx, points) = to_gpx(&export).unwrap();

        assert_eq!(points, 2);
        assert!(gpx.contains("<trkpt lat=\"31.230400\" lon=\"121.473700\">"));
        assert!(gpx.contains("<ele>12.4</ele>"));
        assert_eq!(
            gpx.matches("<gpxtpx:hr>").count(),
            1,
            "only points with an exact timestamp match carry heart rate"
        );
        assert!(gpx.contains("<gpxtpx:hr>132</gpxtpx:hr>"));
        assert!(!gpx.contains("150"), "heart rate from a nearby sample must not go on another point");
    }

    #[test]
    fn gpx_splits_segments_after_a_pause() {
        let export = export_with(json!({
            "workouts": [
                {
                    "workout_id": "w1", "workout_type": "outdoor_running",
                    "route": [
                        { "timestamp": "2026-08-24T06:00:00+08:00", "latitude": 31.0, "longitude": 121.0 },
                        { "timestamp": "2026-08-24T06:10:00+08:00", "latitude": 31.1, "longitude": 121.1 }
                    ],
                    "samples": [],
                    "pauses": [
                        { "start_time": "2026-08-24T06:02:00+08:00",
                          "end_time": "2026-08-24T06:05:00+08:00", "kind": "manual" }
                    ]
                }
            ]
        }));

        let (gpx, points) = to_gpx(&export).unwrap();

        assert_eq!(points, 2);
        assert_eq!(
            gpx.matches("<trkseg>").count(),
            2,
            "the two sides of a pause must not join as a false straight line"
        );
    }

    #[test]
    fn gpx_refuses_to_write_an_empty_track_and_escapes_xml() {
        let without_route = export_with(json!({
            "workouts": [
                { "workout_id": "w1", "workout_type": "indoor_cycling", "route": [], "samples": [] }
            ]
        }));
        let error = to_gpx(&without_route).unwrap_err();
        assert!(error.contains("no GPS track to export"), "actual error: {error}");

        let with_markup = export_with(json!({
            "workouts": [
                {
                    "workout_id": "a<b&c", "workout_type": "run & walk",
                    "start_time": "2026-08-24T06:00:00+08:00",
                    "route": [{ "timestamp": "2026-08-24T06:00:00+08:00",
                                "latitude": 31.0, "longitude": 121.0 }],
                    "samples": []
                }
            ]
        }));
        let (gpx, _) = to_gpx(&with_markup).unwrap();
        assert!(gpx.contains("run &amp; walk"));
        assert!(gpx.contains("a&lt;b&amp;c"));
        assert!(!gpx.contains("a<b&c"), "unescaped XML makes the track file unreadable");
    }

    /// Out-of-range coordinates are the last boundary: lat=999 is not a point on Earth and must not go into GPX.
    #[test]
    fn gpx_drops_points_outside_the_coordinate_domain() {
        let export = export_with(json!({
            "workouts": [
                {
                    "workout_id": "w1", "workout_type": "outdoor_running",
                    "route": [
                        { "timestamp": "2026-08-24T06:00:00+08:00", "latitude": 31.0,
                          "longitude": 121.0 },
                        { "timestamp": "2026-08-24T06:00:01+08:00", "latitude": 999.0,
                          "longitude": 121.0 }
                    ],
                    "samples": [],
                    "pauses": []
                }
            ]
        }));

        let (gpx, points) = to_gpx(&export).unwrap();

        assert_eq!(points, 1, "the point with lat=999 must be dropped");
        assert!(gpx.contains("<trkpt lat=\"31.000000\" lon=\"121.000000\">"));
        assert!(!gpx.contains("999.000000"));
    }
}
