use chrono::{DateTime, FixedOffset};

use embedded_io_adapters::std::FromStd;

use rustyfit::{
    profile::{mesgdef, typedef},
    proto::{Field, Message, Value as FitValue, FIT},
    Encoder,
};

use serde_json::Value;

use std::collections::BTreeMap;

mod fields;
/// The reverse direction: read a .fit the user corrected (#140). It shares field conventions with the writer, so it lives here.
pub mod import;
mod laps;
mod records;

pub(crate) use import::decode_fit_activities;
pub use import::{
    decode_fit_file, FitImportError, ImportedFit, ImportedFitSummary, MAX_FIT_IMPORT_BYTES,
};

use fields::*;
use laps::*;
use records::*;

/// The FIT epoch is 1989-12-31T00:00:00Z, this many seconds after the Unix epoch.
const FIT_EPOCH_OFFSET: i64 = 631_065_600;

/// Latitude and longitude use semicircles: `degrees x 2^31 / 180`.
const SEMICIRCLES_PER_DEGREE: f64 = 2_147_483_648.0 / 180.0;

/// FIT `altitude` stores `(m + 500) x 5` in a u16, so the range is
/// -500 m to about 8192 m. Readings outside this range are not written, instead of being clamped to a fake altitude.
const ALTITUDE_OFFSET_M: f64 = 500.0;

const ALTITUDE_SCALE: f64 = 5.0;

/// All files from one export: `(file name, FIT bytes)`, plus the total number of `record`
/// messages in all files.
pub type FitFiles = (Vec<(String, Vec<u8>)>, usize);

/// Convert a normalized export into several `(file name, FIT bytes)`, one per workout.
///
/// The second return value is the total number of `record` messages in all files. It means the same as the track point count
/// that `to_gpx` returns: the caller can report truthfully "how many samples were written".
pub fn to_fit(export: &Value) -> Result<FitFiles, String> {
    let data = export
        .get("data")
        .ok_or_else(|| "export data is malformed: missing data section".to_string())?;

    let mut files = Vec::new();
    let mut total_records = 0usize;
    let mut used_names: BTreeMap<String, usize> = BTreeMap::new();

    for workout in array(data, "workouts") {
        let Some((bytes, records)) = encode_workout(workout)? else {
            continue;
        };
        let mut name = file_name_for(workout);
        // Two workouts that start in the same second would get the same name. Add a number instead of letting the second overwrite the first.
        let seen = used_names.entry(name.clone()).or_insert(0);
        *seen += 1;
        if *seen > 1 {
            name = format!("{}-{}", name.trim_end_matches(".fit"), seen);
            name.push_str(".fit");
        }
        files.push((name, bytes));
        total_records += records;
    }

    if files.is_empty() {
        return Err(
            "no workout details to export in this period (only workouts with per-second samples or a GPS track can produce FIT)"
                .to_string(),
        );
    }

    Ok((files, total_records))
}

#[cfg(test)]
mod tests;
