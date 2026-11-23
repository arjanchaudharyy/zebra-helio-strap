//! Writes local workouts into a zip that an unstrap backup (the open-source companion app for Helio Strap) can import (Beta lab, plan C3).
//!
//! The unstrap `CsvImporter` finds tables by file name and reads by column name. It takes the tables it knows, and missing tables are not an error.
//! Only two are written here: `workout.csv` (one row per workout) and `workout_hr.csv` (per-point heart rate in workouts).
//!
//! Only values that exist in the database are written: missing cells stay empty, never filled with 0 (unstrap reads an empty cell as "none" and 0 as "0").
//! Zepp's own training load, VO2max and training effect are not written. In unstrap those columns are the strap's own algorithm results,
//! and mixing them in would pollute its trends. Workout types use unstrap numbers (Zepp OS protocol). Unknown types become its
//! generic "activity" (223).
//!
//! The file name is intentionally not `unstrap-backup-*.zip`: unstrap auto-backup rotates and deletes old files by that name.

use std::io::{Cursor, Write};

use chrono::{DateTime, SecondsFormat, Utc};

use crate::import::unstrap::unstrap_sport;
use crate::models::error::{Result, ZeppBridgeError};
use crate::storage::Database;

#[cfg(test)]
mod tests;

const WORKOUT_HEADER: &str = "start,start_epoch,end_epoch,sport_type,exercise_seconds,calories,avg_hr,max_hr,min_hr,aerobic_te,anaerobic_te,training_load,vo2max,steps,subtype";
const WORKOUT_HR_HEADER: &str = "workout_start_epoch,epoch,bpm";

/// The unstrap generic "activity".
const GENERIC_ACTIVITY: i64 = 223;

/// How many workouts to read per page.
const PAGE: usize = 200;

#[derive(Debug, Clone, PartialEq)]
pub struct UnstrapExport {
    pub bytes: Vec<u8>,
    pub workouts: usize,
    pub heart_rate_points: usize,
}

/// Workout catalog key -> unstrap number. When one key has several numbers, use the one the strap uses now
/// (soccer 191, padel 216, same as unstrap's own `STRAP_IDS`).
pub fn unstrap_code(key: &str) -> i64 {
    const PREFERRED: [i64; 2] = [191, 216];
    PREFERRED
        .into_iter()
        .chain(0..300)
        .find(|code| unstrap_sport(*code) == key && key != "activity")
        .unwrap_or(GENERIC_ACTIVITY)
}

fn cell<T: ToString>(value: Option<T>) -> String {
    value.map(|v| v.to_string()).unwrap_or_default()
}

fn zip_error(error: zip::result::ZipError) -> ZeppBridgeError {
    ZeppBridgeError::IoError(std::io::Error::other(error))
}

/// Reads all visible workouts that start at or after `since` and writes them into a zip. Empty `since` means all.
pub fn build_unstrap_export(db: &Database, since: Option<DateTime<Utc>>) -> Result<UnstrapExport> {
    let mut workouts_csv = String::from(WORKOUT_HEADER);
    workouts_csv.push('\n');
    let mut hr_csv = String::from(WORKOUT_HR_HEADER);
    hr_csv.push('\n');
    let mut workouts = 0;
    let mut heart_rate_points = 0;
    let mut offset = 0;
    'pages: loop {
        let page = db.workouts_page(PAGE, offset)?;
        if page.is_empty() {
            break;
        }
        offset += page.len();
        for summary in page {
            if since.is_some_and(|since| summary.start_time < since) {
                // Newest first: everything after this is older.
                break 'pages;
            }
            let Some(workout) = db.get_workout_detail(&summary.workout_id)? else {
                continue;
            };
            let start = workout.start_time.timestamp();
            let end = workout.end_time.timestamp();
            if end <= start {
                continue;
            }
            workouts += 1;
            let row = [
                workout
                    .start_time
                    .to_rfc3339_opts(SecondsFormat::Secs, true),
                start.to_string(),
                end.to_string(),
                unstrap_code(&workout.effective_type).to_string(),
                cell(workout.moving_seconds.filter(|s| *s > 0)),
                cell(workout.calories.filter(|c| *c > 0)),
                cell(workout.avg_hr.filter(|hr| *hr > 0)),
                cell(workout.max_hr.filter(|hr| *hr > 0)),
                cell(workout.min_hr.filter(|hr| *hr > 0)),
                String::new(),
                String::new(),
                String::new(),
                String::new(),
                cell(workout.total_steps.filter(|s| *s > 0)),
                String::new(),
            ];
            workouts_csv.push_str(&row.join(","));
            workouts_csv.push('\n');
            let series = db.get_workout_series(&workout.workout_id)?;
            let mut last_second = None;
            for sample in series.samples {
                let (Some(bpm), Ok(time)) = (
                    sample.heart_rate.filter(|bpm| (25..=250).contains(bpm)),
                    DateTime::parse_from_rfc3339(&sample.timestamp),
                ) else {
                    continue;
                };
                // unstrap keys heart rate by second: keep only the first one in the same second.
                let second = time.timestamp();
                if last_second == Some(second) {
                    continue;
                }
                last_second = Some(second);
                heart_rate_points += 1;
                hr_csv.push_str(&format!("{start},{second},{bpm}\n"));
            }
        }
    }

    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (name, text) in [("workout.csv", &workouts_csv), ("workout_hr.csv", &hr_csv)] {
        zip.start_file(name, options).map_err(zip_error)?;
        zip.write_all(text.as_bytes())?;
    }
    let bytes = zip.finish().map_err(zip_error)?.into_inner();
    Ok(UnstrapExport {
        bytes,
        workouts,
        heart_rate_points,
    })
}

/// Export file name: `zeppbridge-for-unstrap-<local date>.zip`.
pub fn export_file_name(today: chrono::NaiveDate) -> String {
    format!("zeppbridge-for-unstrap-{today}.zip")
}
