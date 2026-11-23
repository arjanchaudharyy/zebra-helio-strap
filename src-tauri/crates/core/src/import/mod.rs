//! Import workouts from other sources (batch 7): GPX, FIT, a full Strava export, an Apple Health export,
//! and since 2026-11 also unstrap (Helio Strap companion app) backup zips (Beta lab).
//!
//! Imported workouts join the workout list, and the UI shows their source. **They do not enter Zepp sync** and do not go back to the cloud. They are
//! data the user brought in, like ratings and shoes: replay does not touch them (replay deletes only by `raw_record_id`),
//! and retention cleanup does not touch them. Each import is one batch, and a batch can be undone as a whole (`storage::imports`).
//!
//! No invented data: no heart rate in the file means no heart rate. GPX without speed gets no per-point speed (GPS differences are very noisy
//! and give a fake smooth line), only cumulative distance. Kilometer splits come from cumulative distance, the same way as FIT import.
//! Entries we cannot read are counted as they are (`unreadable`). We do not guess.

pub mod apple;
pub mod gpx;
pub mod strava;
pub mod unstrap;
pub mod unstrap_health;
mod xml;

#[cfg(test)]
mod tests;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::decoder::{splits_from_cumulative_distance, DecodedWorkout, RoutePoint, WorkoutSample};

/// Size limit for one file. Apple Health exports can be many GB; that path parses while it streams and has no such limit.
pub const MAX_IMPORT_FILE_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImportSource {
    Gpx,
    Fit,
    Strava,
    AppleHealth,
    Unstrap,
}

impl ImportSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Gpx => "gpx",
            Self::Fit => "fit",
            Self::Strava => "strava",
            Self::AppleHealth => "apple_health",
            Self::Unstrap => "unstrap",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "gpx" => Some(Self::Gpx),
            "fit" => Some(Self::Fit),
            "strava" => Some(Self::Strava),
            "apple_health" => Some(Self::AppleHealth),
            "unstrap" => Some(Self::Unstrap),
            _ => None,
        }
    }
}

/// One workout that was read. Anything missing is `None`.
#[derive(Debug, Clone)]
pub struct ParsedActivity {
    /// Key in the sport catalog (`run`, `ride`, ...). Unknown types are `activity`.
    pub sport: String,
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    pub distance_m: Option<f64>,
    pub moving_seconds: Option<i64>,
    pub calories: Option<i32>,
    pub avg_hr: Option<i32>,
    pub max_hr: Option<i32>,
    pub elevation_gain_m: Option<f64>,
    /// Per-point series and track. Summary-only workouts (for example Apple Health strength training without a route) are `None`.
    pub decoded: Option<DecodedWorkout>,
    /// The name in the file (Strava activity name, GPX `<name>`). Display only.
    pub name: Option<String>,
}

/// Result of reading one file.
#[derive(Debug, Clone, Default)]
pub struct ParsedFile {
    pub activities: Vec<ParsedActivity>,
    /// Number of entries that cannot be read or have an unsupported format.
    pub unreadable: usize,
    /// Health samples (only unstrap backups have them; see `unstrap_health`).
    pub health: Option<unstrap_health::ParsedHealth>,
}

/// One track point. GPX and TCX are both read into this first.
#[derive(Debug, Clone, Default)]
pub(crate) struct TrackPoint {
    pub segment_start: bool,
    pub time: Option<DateTime<Utc>>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub altitude_m: Option<f64>,
    pub heart_rate: Option<i32>,
    pub cadence: Option<f64>,
    pub power_watts: Option<f64>,
    /// Cumulative distance from TCX itself. GPX has none, so we compute it from coordinates.
    pub distance_m: Option<f64>,
}

/// Distance in meters between two points on the Earth's surface (haversine formula).
pub(crate) fn haversine_m(a: (f64, f64), b: (f64, f64)) -> f64 {
    const R: f64 = 6_371_000.0;
    let (lat1, lat2) = (a.0.to_radians(), b.0.to_radians());
    let dlat = lat2 - lat1;
    let dlon = (b.1 - a.1).to_radians();
    let h = (dlat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (dlon / 2.0).sin().powi(2);
    2.0 * R * h.sqrt().asin()
}

/// A series of timed points -> one workout. Without timed points there is no workout (the day is unknown).
pub(crate) fn activity_from_track(
    points: Vec<TrackPoint>,
    sport: String,
    name: Option<String>,
) -> Option<ParsedActivity> {
    let mut points: Vec<TrackPoint> = points.into_iter().filter(|p| p.time.is_some()).collect();
    points.sort_by_key(|p| p.time);
    let start_time = points.first()?.time?;
    let end_time = points.last()?.time?;
    if end_time <= start_time {
        return None;
    }

    let mut samples = Vec::with_capacity(points.len());
    let mut route = Vec::new();
    let mut distances: BTreeMap<i64, f64> = BTreeMap::new();
    let mut walked = 0.0;
    let mut segment_start = false;
    let mut last_fix: Option<(f64, f64)> = None;
    let has_own_distance = points.iter().any(|p| p.distance_m.is_some());
    let mut gain = 0.0;
    let mut has_altitude = false;
    let mut last_altitude: Option<f64> = None;
    for point in &points {
        if point.segment_start {
            last_fix = None;
            last_altitude = None;
            segment_start = true;
        }
        let time = point.time?;
        let fix = point.latitude.zip(point.longitude).filter(|(lat, lon)| {
            lat.is_finite() && lon.is_finite() && lat.abs() <= 90.0 && lon.abs() <= 180.0
        });
        if let Some((latitude, longitude)) = fix {
            if let Some(previous) = last_fix {
                walked += haversine_m(previous, (latitude, longitude));
            }
            last_fix = Some((latitude, longitude));
            route.push(RoutePoint {
                segment_start: segment_start && !route.is_empty(),
                timestamp: time,
                latitude,
                longitude,
                altitude_m: point.altitude_m,
            });
            segment_start = false;
        }
        let distance = if has_own_distance {
            point.distance_m
        } else {
            last_fix.map(|_| walked)
        };
        if let Some(distance) = distance {
            distances.insert(time.timestamp(), distance);
        }
        // Accumulate ascent with a 1 m noise floor, the same rule as decoding cloud details.
        if let Some(altitude) = point.altitude_m {
            has_altitude = true;
            match last_altitude {
                Some(previous) if altitude - previous >= 1.0 => {
                    gain += altitude - previous;
                    last_altitude = Some(altitude);
                }
                Some(previous) if previous - altitude >= 1.0 => last_altitude = Some(altitude),
                None => last_altitude = Some(altitude),
                _ => {}
            }
        }
        samples.push(WorkoutSample {
            timestamp: time,
            heart_rate: point.heart_rate.filter(|hr| (25..=250).contains(hr)),
            speed: None,
            pace: None,
            cadence: point.cadence,
            stride_cm: None,
            altitude_m: point.altitude_m,
            power_watts: point.power_watts,
            ground_contact_ms: None,
            vertical_oscillation_mm: None,
            vertical_ratio_pct: None,
            equivalent_pace_s_per_km: None,
            distance_m: distance,
        });
    }
    let heart: Vec<i32> = samples.iter().filter_map(|s| s.heart_rate).collect();
    let distance_m = distances.values().next_back().copied().filter(|d| *d > 0.0);
    let splits = splits_from_cumulative_distance(&samples, &distances);
    Some(ParsedActivity {
        sport,
        start_time,
        end_time,
        distance_m,
        moving_seconds: None,
        calories: None,
        avg_hr: (!heart.is_empty())
            .then(|| (heart.iter().map(|hr| *hr as i64).sum::<i64>() / heart.len() as i64) as i32),
        max_hr: heart.iter().copied().max(),
        elevation_gain_m: has_altitude.then_some((gain * 10.0).round() / 10.0),
        decoded: Some(DecodedWorkout {
            track_id: start_time.timestamp(),
            source: Some("import".into()),
            start_time,
            end_time,
            route,
            route_dropped_points: 0,
            samples,
            pauses: Vec::new(),
            splits,
            laps: Vec::new(),
            strength_sets: Vec::new(),
        }),
        name,
    })
}

/// FIT file -> one workout.
pub fn activity_from_fit(bytes: &[u8]) -> Option<ParsedActivity> {
    let fit = crate::export_fit::decode_fit_file(bytes).ok()?;
    Some(parsed_fit(fit))
}

pub(crate) fn activities_from_fit(bytes: &[u8]) -> Vec<ParsedActivity> {
    crate::export_fit::decode_fit_activities(bytes)
        .map(|fits| fits.into_iter().map(parsed_fit).collect())
        .unwrap_or_default()
}

fn parsed_fit(fit: crate::export_fit::ImportedFit) -> ParsedActivity {
    let summary = fit.summary;
    ParsedActivity {
        sport: fit_sport_and_sub_sport(fit.sport, fit.sub_sport).to_string(),
        start_time: fit.decoded.start_time,
        end_time: fit.decoded.end_time,
        distance_m: summary.distance_m,
        moving_seconds: summary.moving_seconds,
        calories: None,
        avg_hr: summary.avg_hr,
        max_hr: summary.max_hr,
        elevation_gain_m: None,
        decoded: Some(fit.decoded),
        name: None,
    }
}

fn fit_sport_and_sub_sport(sport: Option<u8>, sub_sport: Option<u8>) -> &'static str {
    use rustyfit::profile::typedef::{Sport, SubSport};
    match (sport.map(Sport), sub_sport.map(SubSport)) {
        (Some(Sport::RUNNING), Some(SubSport::TRAIL)) => "trail_running",
        (Some(Sport::RUNNING), Some(SubSport::TREADMILL)) => "treadmill",
        (Some(Sport::CYCLING), Some(SubSport::MOUNTAIN)) => "mountain_biking",
        (Some(Sport::CYCLING), Some(SubSport::ROAD)) => "road_cycling",
        (Some(Sport::CYCLING), Some(SubSport::INDOOR_CYCLING)) => "indoor_cycling",
        (Some(Sport::CYCLING), Some(SubSport::SPIN)) => "spinning",
        (Some(Sport::CYCLING), Some(SubSport::BMX)) => "bmx",
        (Some(Sport::SWIMMING), Some(SubSport::OPEN_WATER)) => "open_water_swimming",
        (Some(Sport::TRAINING), Some(SubSport::STRENGTH_TRAINING)) => "strength",
        (Some(Sport::WALKING), Some(SubSport::INDOOR_WALKING)) => "indoor_walking",
        (Some(Sport::WALKING), Some(SubSport::SPEED_WALKING)) => "race_walking",
        (Some(Sport::ALPINE_SKIING), _) => "alpine_skiing",
        _ => sport.map_or("activity", fit_sport),
    }
}

/// FIT `sport` enum -> sport catalog key. Only common types are mapped; others are `activity`, and the user can change them.
pub fn fit_sport(code: u8) -> &'static str {
    match code {
        1 => "run",
        2 => "ride",
        4 => "indoor_fitness",
        5 => "pool_swimming",
        10 => "free_training",
        11 => "walking",
        12 => "cross_country_skiing",
        15 => "rowing",
        17 => "hiking",
        21 => "e_bike",
        _ => "activity",
    }
}

/// Text sport type (GPX `<type>`, the Strava "Activity Type", TCX `Sport`) -> sport catalog key.
pub fn text_sport(value: &str) -> &'static str {
    let key: String = value
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect();
    // Numbers and "swim" / "other" are habits of import files, so they stay out of the shared alias table.
    match key.as_str() {
        "9" => "run",
        "1" => "ride",
        "10" => "walking",
        "4" => "hiking",
        "swim" | "swimming" => "pool_swimming",
        "workout" | "other" => "free_training",
        _ => crate::sport_catalog::text_alias(value).unwrap_or("activity"),
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ImportReadError {
    #[error("file exceeds the size limit")]
    TooLarge,
    #[error("not a file from this source")]
    NotRecognized,
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Read one local file by source. The GPX source also accepts TCX; Apple Health accepts `export.zip` or the unzipped `export.xml`.
pub fn read_import_file(
    source: ImportSource,
    path: &std::path::Path,
) -> Result<ParsedFile, ImportReadError> {
    use std::io::{BufReader, Read};
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let file = std::fs::File::open(path)?;
    let read_small = |file: std::fs::File| -> Result<Vec<u8>, ImportReadError> {
        if file.metadata()?.len() > MAX_IMPORT_FILE_BYTES {
            return Err(ImportReadError::TooLarge);
        }
        let mut bytes = Vec::new();
        file.take(MAX_IMPORT_FILE_BYTES).read_to_end(&mut bytes)?;
        Ok(bytes)
    };
    let parsed = match source {
        ImportSource::Gpx | ImportSource::Fit => {
            let stem = name.strip_suffix(".gz").unwrap_or(&name);
            let fits = match source {
                ImportSource::Fit => stem.ends_with(".fit"),
                _ => stem.ends_with(".gpx") || stem.ends_with(".tcx"),
            };
            if !fits {
                return Err(ImportReadError::NotRecognized);
            }
            strava::parse_activity_file(&name, &read_small(file)?, None)
        }
        ImportSource::Strava => strava::parse_strava_archive(BufReader::new(file))
            .ok_or(ImportReadError::NotRecognized)?,
        ImportSource::AppleHealth if name.ends_with(".xml") => apple::parse_apple_xml(file)?,
        ImportSource::AppleHealth => apple::parse_apple_archive(BufReader::new(file))?,
        ImportSource::Unstrap => unstrap::parse_unstrap_archive(BufReader::new(file))
            .ok_or(ImportReadError::NotRecognized)?,
    };
    Ok(parsed)
}
