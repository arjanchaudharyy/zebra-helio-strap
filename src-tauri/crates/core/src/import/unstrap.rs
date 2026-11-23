//! Backup zip of unstrap (formerly OpenStrap, the open-source Helio Strap companion app): one CSV per table, read by column name.
//!
//! Phase 1 reads only workouts (plan C2):
//! - `workout.csv` + `workout_hr.csv`: workouts recorded by the strap itself, with a summary and per-second heart rate only, no track;
//! - `app_workout.csv` + `app_workout_hr.csv` + `app_workout_route.csv`: workouts started on the phone,
//!   with a track only if the user chose to keep the route.
//!
//! The strap record and the phone record are often the same workout (started on the phone while the strap records). Starts within two minutes merge into one,
//! and the phone record wins (it has the track and pauses); missing heart rate comes from the strap record. Rows in `workout_deleted.csv` were
//! deleted by the user in unstrap and are not imported.
//!
//! No invented data: scores unstrap computes (recovery time, training effect, VO₂max) are not imported and do not replace Zepp's.
//! Distance comes only from the GPS track or the distance the phone measured. Distance estimated from steps (`distance_from_steps`) is dropped.
//! Sport types are Zepp OS protocol numbers, not the same set as cloud history (6 is pool swimming, not walking),
//! so they have their own table ([`unstrap_sport`]). Unknown types are `activity`.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{Read, Seek};

use chrono::{DateTime, TimeZone, Utc};

use super::strava::{entry_name, parse_csv, read_entry};
use super::{activity_from_track, ParsedActivity, ParsedFile, TrackPoint};
use crate::decoder::PauseInterval;

/// Same window as storage dedup: starts within this many seconds count as the same workout.
const SAME_WORKOUT_SECONDS: i64 = 120;

/// One CSV table: header + rows, values read by column name. An empty cell means no value.
pub(crate) struct Table {
    columns: HashMap<String, usize>,
    pub(crate) rows: Vec<Vec<String>>,
}

impl Table {
    pub(crate) fn parse(text: &str) -> Option<Self> {
        let mut rows = parse_csv(text).into_iter();
        let header = rows.next()?;
        let columns = header
            .iter()
            .enumerate()
            .map(|(i, name)| (name.trim().to_string(), i))
            .collect();
        Some(Self {
            columns,
            rows: rows
                .filter(|row| row.iter().any(|cell| !cell.is_empty()))
                .collect(),
        })
    }

    pub(crate) fn text<'a>(&self, row: &'a [String], column: &str) -> Option<&'a str> {
        let index = *self.columns.get(column)?;
        row.get(index)
            .map(|cell| cell.trim())
            .filter(|cell| !cell.is_empty())
    }

    pub(crate) fn int(&self, row: &[String], column: &str) -> Option<i64> {
        self.text(row, column)?.parse().ok()
    }

    pub(crate) fn float(&self, row: &[String], column: &str) -> Option<f64> {
        self.text(row, column)?
            .parse::<f64>()
            .ok()
            .filter(|v| v.is_finite())
    }

    pub(crate) fn flag(&self, row: &[String], column: &str) -> bool {
        matches!(self.text(row, column), Some("true" | "1"))
    }
}

/// Whether the archive has unstrap workout or health tables. If it has neither, it is not an unstrap backup.
pub fn is_unstrap_archive<R: Read + Seek>(archive: &mut zip::ZipArchive<R>) -> bool {
    entry_name(archive, |name| {
        matches!(
            name,
            "workout.csv" | "app_workout.csv" | "heart_rate.csv" | "sleep_session.csv"
        )
    })
    .is_some()
}

pub(crate) fn read_table<R: Read + Seek>(
    archive: &mut zip::ZipArchive<R>,
    file: &str,
) -> Option<Table> {
    let name = entry_name(archive, |name| name == file)?;
    let bytes = read_entry(archive, &name)?;
    Table::parse(&String::from_utf8(bytes).ok()?)
}

pub(crate) fn epoch(seconds: i64) -> Option<DateTime<Utc>> {
    Utc.timestamp_opt(seconds, 0).single()
}

/// Per-point table grouped by the start of its workout (`workout_start_epoch`).
fn by_workout(table: Option<Table>) -> (Option<Table>, BTreeMap<i64, Vec<usize>>) {
    let mut groups: BTreeMap<i64, Vec<usize>> = BTreeMap::new();
    if let Some(t) = &table {
        for (i, row) in t.rows.iter().enumerate() {
            if let Some(start) = t.int(row, "workout_start_epoch") {
                groups.entry(start).or_default().push(i);
            }
        }
    }
    (table, groups)
}

/// A group of per-second heart rates (`epoch`, `bpm`) -> track points. Values outside 25–250 are dropped.
fn heart_points(table: &Option<Table>, rows: Option<&Vec<usize>>) -> Vec<TrackPoint> {
    let (Some(t), Some(rows)) = (table, rows) else {
        return Vec::new();
    };
    rows.iter()
        .filter_map(|&i| {
            let row = &t.rows[i];
            let time = epoch(t.int(row, "epoch")?)?;
            let bpm = t.int(row, "bpm").filter(|bpm| (25..=250).contains(bpm))? as i32;
            Some(TrackPoint {
                time: Some(time),
                heart_rate: Some(bpm),
                ..TrackPoint::default()
            })
        })
        .collect()
}

/// One workout: build the series from per-point data, then apply the table's start, end and summary (the table is more exact than the points).
struct Summary {
    sport: String,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    moving_seconds: Option<i64>,
    calories: Option<i32>,
    avg_hr: Option<i32>,
    max_hr: Option<i32>,
    distance_m: Option<f64>,
    pauses: Vec<PauseInterval>,
}

fn build(summary: Summary, points: Vec<TrackPoint>) -> ParsedActivity {
    let mut activity =
        activity_from_track(points, summary.sport.clone(), None).unwrap_or(ParsedActivity {
            sport: summary.sport.clone(),
            start_time: summary.start,
            end_time: summary.end,
            distance_m: None,
            moving_seconds: None,
            calories: None,
            avg_hr: None,
            max_hr: None,
            elevation_gain_m: None,
            decoded: None,
            name: None,
        });
    activity.start_time = summary.start;
    activity.end_time = summary.end;
    activity.moving_seconds = summary.moving_seconds;
    activity.calories = summary.calories;
    activity.avg_hr = summary.avg_hr.or(activity.avg_hr);
    activity.max_hr = summary.max_hr.or(activity.max_hr);
    activity.distance_m = summary.distance_m.or(activity.distance_m);
    if let Some(decoded) = activity.decoded.as_mut() {
        decoded.track_id = summary.start.timestamp();
        decoded.source = Some("import".into());
        decoded.start_time = summary.start;
        decoded.end_time = summary.end;
        decoded.pauses = summary.pauses;
    }
    activity
}

fn positive_int(value: Option<i64>) -> Option<i32> {
    value.filter(|v| *v > 0).and_then(|v| i32::try_from(v).ok())
}

fn heart(value: Option<i64>) -> Option<i32> {
    value.filter(|v| (25..=250).contains(v)).map(|v| v as i32)
}

/// `pauses` column: pairs of `start-end` second timestamps, comma separated. Segments we cannot read are skipped.
fn parse_pauses(text: Option<&str>) -> Vec<PauseInterval> {
    text.unwrap_or_default()
        .split(',')
        .filter_map(|pair| {
            let (from, to) = pair.trim().split_once('-')?;
            let (from, to) = (from.parse().ok()?, to.parse().ok()?);
            (to > from).then_some(())?;
            Some(PauseInterval {
                start_time: epoch(from)?,
                end_time: epoch(to)?,
                kind: "manual".into(),
            })
        })
        .collect()
}

/// Read the workouts in a full unstrap backup. Returns `None` for an archive that is not from unstrap.
pub fn parse_unstrap_archive<R: Read + Seek>(reader: R) -> Option<ParsedFile> {
    let mut archive = zip::ZipArchive::new(reader).ok()?;
    if !is_unstrap_archive(&mut archive) {
        return None;
    }
    let deleted: HashSet<i64> = read_table(&mut archive, "workout_deleted.csv")
        .map(|t| {
            t.rows
                .iter()
                .filter_map(|row| t.int(row, "start_epoch"))
                .collect()
        })
        .unwrap_or_default();
    let strap = read_table(&mut archive, "workout.csv");
    let (strap_hr, strap_hr_groups) = by_workout(read_table(&mut archive, "workout_hr.csv"));
    let phone = read_table(&mut archive, "app_workout.csv");
    let (phone_hr, phone_hr_groups) = by_workout(read_table(&mut archive, "app_workout_hr.csv"));
    let (route, route_groups) = by_workout(read_table(&mut archive, "app_workout_route.csv"));

    let mut file = ParsedFile::default();

    // Strap records: start -> (summary, per-second heart rate).
    let mut strap_workouts: BTreeMap<i64, (Summary, Vec<TrackPoint>)> = BTreeMap::new();
    if let Some(t) = &strap {
        for row in &t.rows {
            let (Some(start), Some(end)) = (t.int(row, "start_epoch"), t.int(row, "end_epoch"))
            else {
                file.unreadable += 1;
                continue;
            };
            if deleted.contains(&start) {
                continue;
            }
            let (Some(start_time), Some(end_time)) = (epoch(start), epoch(end)) else {
                file.unreadable += 1;
                continue;
            };
            if end_time <= start_time {
                file.unreadable += 1;
                continue;
            }
            let summary = Summary {
                sport: unstrap_sport(t.int(row, "sport_type").unwrap_or(-1)).to_string(),
                start: start_time,
                end: end_time,
                moving_seconds: t.int(row, "exercise_seconds").filter(|s| *s > 0),
                calories: positive_int(t.int(row, "calories")),
                avg_hr: heart(t.int(row, "avg_hr")),
                max_hr: heart(t.int(row, "max_hr")),
                distance_m: None,
                pauses: Vec::new(),
            };
            let points = heart_points(&strap_hr, strap_hr_groups.get(&start));
            strap_workouts.insert(start, (summary, points));
        }
    }

    if let Some(t) = &phone {
        for row in &t.rows {
            let (Some(start), Some(end)) = (t.int(row, "start_epoch"), t.int(row, "end_epoch"))
            else {
                file.unreadable += 1;
                continue;
            };
            let (Some(start_time), Some(end_time)) = (epoch(start), epoch(end)) else {
                file.unreadable += 1;
                continue;
            };
            if end_time <= start_time {
                file.unreadable += 1;
                continue;
            }
            // The strap also recorded this workout: merge it here, and do not import the strap record on its own.
            let twin = strap_workouts
                .range(start - SAME_WORKOUT_SECONDS..=start + SAME_WORKOUT_SECONDS)
                .map(|(key, _)| *key)
                .next()
                .and_then(|key| strap_workouts.remove(&key));
            let mut points = heart_points(&phone_hr, phone_hr_groups.get(&start));
            if points.is_empty() {
                if let Some((_, strap_points)) = &twin {
                    points = strap_points.clone();
                }
            }
            if let (Some(rt), Some(rows)) = (&route, route_groups.get(&start)) {
                for &i in rows {
                    let r = &rt.rows[i];
                    let (Some(time), Some(lat), Some(lon)) = (
                        rt.int(r, "epoch").and_then(epoch),
                        rt.float(r, "lat"),
                        rt.float(r, "lon"),
                    ) else {
                        continue;
                    };
                    points.push(TrackPoint {
                        time: Some(time),
                        latitude: Some(lat),
                        longitude: Some(lon),
                        altitude_m: rt.float(r, "altitude_m"),
                        ..TrackPoint::default()
                    });
                }
            }
            let measured = (!t.flag(row, "distance_from_steps"))
                .then(|| t.float(row, "distance_m"))
                .flatten()
                .filter(|d| *d > 0.0);
            let twin_summary = twin.as_ref().map(|(summary, _)| summary);
            let kcal = t
                .float(row, "total_kcal")
                .map(|k| k.round() as i64)
                .and_then(|k| positive_int(Some(k)))
                .or_else(|| twin_summary.and_then(|s| s.calories));
            let summary = Summary {
                sport: unstrap_sport(t.int(row, "sport_type").unwrap_or(-1)).to_string(),
                start: start_time,
                end: end_time,
                moving_seconds: twin_summary.and_then(|s| s.moving_seconds),
                calories: kcal,
                avg_hr: heart(t.int(row, "avg_hr")).or(twin_summary.and_then(|s| s.avg_hr)),
                max_hr: heart(t.int(row, "max_hr")).or(twin_summary.and_then(|s| s.max_hr)),
                distance_m: measured,
                pauses: parse_pauses(t.text(row, "pauses")),
            };
            file.activities.push(build(summary, points));
        }
    }

    file.activities.extend(
        strap_workouts
            .into_values()
            .map(|(summary, points)| build(summary, points)),
    );
    file.activities.sort_by_key(|activity| activity.start_time);
    let (health, unreadable) = super::unstrap_health::parse_unstrap_health(&mut archive);
    file.unreadable += unreadable;
    file.health = (!health.is_empty()).then_some(health);
    Some(file)
}

/// unstrap sport numbers (Zepp OS protocol, `Workouts.SPORT_NAMES`) -> sport catalog key.
/// Only certain matches are mapped. Golf, pickleball, HYROX, hybrid training, "skiing" (alpine or cross-country is unknown),
/// and "climbing" (rock climbing or mountaineering is unknown) have no catalog entry, so they are `activity`, and the user can change them.
pub fn unstrap_sport(code: i64) -> &'static str {
    match code {
        1 => "run",
        2 => "treadmill",
        3 => "walking",
        4 => "ride",
        5 | 16 => "free_training",
        6 => "pool_swimming",
        7 => "open_water_swimming",
        8 => "indoor_cycling",
        9 => "elliptical",
        11 => "trail_running",
        15 | 224 => "hiking",
        17 => "tennis",
        18 | 191 => "soccer",
        21 => "jump_rope",
        23 => "rowing",
        24 => "indoor_fitness",
        40 => "indoor_walking",
        49 => "hiit",
        50 => "core_training",
        51 => "aerobic_combo",
        52 => "strength",
        53 => "stretching",
        55 => "flexibility",
        57 => "stepper",
        58 => "stair_climber",
        60 => "yoga",
        61 => "pilates",
        70 => "rock_climbing",
        76 => "dance",
        85 => "basketball",
        88 => "volleyball",
        89 => "table_tennis",
        92 => "badminton",
        97 => "boxing",
        108 => "stair_climbing",
        109 => "aerobics",
        130 => "cross_training",
        143 => "spinning",
        205 | 216 => "padel",
        215 => "dog_walking",
        225 => "rucking",
        _ => "activity",
    }
}
