//! Health samples in an unstrap backup (plan C2, phase 2). Per-table rules are in `docs/development/unstrap-mapping.md`.
//!
//! Read-only and selective: heart rate (history stored on the strap, one row per minute), spot SpO2, resting heart rate (by the local day unstrap records),
//! and sleep (no naps, no scores). This module only reads tables into structs. "Skip what this device already has" happens at storage time
//! (`storage::import_health`).

use std::collections::{BTreeMap, HashMap};
use std::io::{Read, Seek};

use chrono::{DateTime, NaiveDate, Utc};

use super::strava::entry_name;
use super::unstrap::{epoch, read_table, Table};
use crate::models::{SleepSession, SleepStageSlice, SourceScope};

/// Device ID for imported health samples: separate from Zepp devices, so unique keys do not collide.
pub const UNSTRAP_DEVICE: &str = "unstrap";

/// One point sample.
#[derive(Debug, Clone, PartialEq)]
pub struct HealthSample {
    pub metric: &'static str,
    pub unit: &'static str,
    pub timestamp: DateTime<Utc>,
    pub value: f64,
}

/// One daily value. `date` is the local day (`YYYY-MM-DD`).
#[derive(Debug, Clone, PartialEq)]
pub struct HealthDaily {
    pub metric: &'static str,
    pub unit: &'static str,
    pub date: String,
    pub value: f64,
}

#[derive(Debug, Clone, Default)]
pub struct ParsedHealth {
    pub samples: Vec<HealthSample>,
    pub daily: Vec<HealthDaily>,
    pub sleeps: Vec<SleepSession>,
}

impl ParsedHealth {
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty() && self.daily.is_empty() && self.sleeps.is_empty()
    }
}

/// Read one table. If the archive has the table but it cannot be read (too large, not UTF-8), count one unreadable item.
fn load<R: Read + Seek>(
    archive: &mut zip::ZipArchive<R>,
    file: &str,
    unreadable: &mut usize,
) -> Option<Table> {
    entry_name(archive, |name| name == file)?;
    let table = read_table(archive, file);
    if table.is_none() {
        *unreadable += 1;
    }
    table
}

fn point_samples(
    table: Option<Table>,
    metric: &'static str,
    unit: &'static str,
    column: &str,
    range: std::ops::RangeInclusive<f64>,
    keep: impl Fn(&Table, &[String]) -> bool,
    out: &mut Vec<HealthSample>,
) -> usize {
    let Some(t) = table else { return 0 };
    let mut unreadable = 0;
    for row in &t.rows {
        if !keep(&t, row) {
            continue;
        }
        let (Some(timestamp), Some(value)) = (
            t.int(row, "epoch").and_then(epoch),
            t.float(row, column).filter(|v| range.contains(v)),
        ) else {
            unreadable += 1;
            continue;
        };
        out.push(HealthSample {
            metric,
            unit,
            timestamp,
            value,
        });
    }
    unreadable
}

fn stage_name(text: &str) -> &'static str {
    match text {
        "awake" | "0" => "awake",
        "light" | "1" => "light",
        "deep" | "2" => "deep",
        "rem" | "3" => "rem",
        _ => "unknown",
    }
}

fn instant(text: Option<&str>) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(text?)
        .ok()
        .map(|t| t.with_timezone(&Utc))
}

/// Read the health samples in a backup. Returns the samples and the number of unreadable rows.
pub fn parse_unstrap_health<R: Read + Seek>(
    archive: &mut zip::ZipArchive<R>,
) -> (ParsedHealth, usize) {
    let mut unreadable = 0;
    let mut health = ParsedHealth::default();

    // Heart rate: take only history stored on the strap (source = 0; old backups without the column are all history).
    let heart = load(archive, "heart_rate.csv", &mut unreadable);
    unreadable += point_samples(
        heart,
        "heart_rate",
        "bpm",
        "bpm",
        25.0..=250.0,
        |t, row| t.int(row, "source").unwrap_or(0) == 0,
        &mut health.samples,
    );
    let spo2 = load(archive, "spo2.csv", &mut unreadable);
    unreadable += point_samples(
        spo2,
        "spo2",
        "%",
        "percent",
        50.0..=100.0,
        |_, _| true,
        &mut health.samples,
    );

    // Resting heart rate: use the local day unstrap records. Old backups without a date column skip the whole table. Do not guess the time zone.
    if let Some(t) = load(archive, "resting_hr.csv", &mut unreadable) {
        for row in &t.rows {
            let date = t
                .text(row, "date")
                .filter(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").is_ok());
            let bpm = t.float(row, "bpm").filter(|v| (25.0..=250.0).contains(v));
            match (date, bpm) {
                (Some(date), Some(value)) => health.daily.push(HealthDaily {
                    metric: "resting_hr",
                    unit: "bpm",
                    date: date.to_string(),
                    value,
                }),
                _ => unreadable += 1,
            }
        }
    }

    // Sleep: drop entries the user deleted in unstrap. Use edited start and end times when present.
    let mut edits: HashMap<i64, (bool, Option<i64>, Option<i64>)> = HashMap::new();
    if let Some(t) = load(archive, "sleep_edit.csv", &mut unreadable) {
        for row in &t.rows {
            if let Some(strap_start) = t.int(row, "strap_start_epoch") {
                edits.insert(
                    strap_start,
                    (
                        t.flag(row, "deleted"),
                        t.int(row, "start_epoch"),
                        t.int(row, "end_epoch"),
                    ),
                );
            }
        }
    }
    let mut stages: BTreeMap<i64, Vec<SleepStageSlice>> = BTreeMap::new();
    if let Some(t) = load(archive, "sleep_stage.csv", &mut unreadable) {
        for row in &t.rows {
            let (Some(session), Some(start), Some(end)) = (
                t.int(row, "session_start_epoch"),
                instant(t.text(row, "start")),
                instant(t.text(row, "end")),
            ) else {
                unreadable += 1;
                continue;
            };
            if end <= start {
                unreadable += 1;
                continue;
            }
            stages.entry(session).or_default().push(SleepStageSlice {
                stage: stage_name(t.text(row, "stage").unwrap_or_default()).to_string(),
                start_time: start,
                end_time: end,
                raw_mode: None,
            });
        }
    }
    if let Some(t) = load(archive, "sleep_session.csv", &mut unreadable) {
        for row in &t.rows {
            if t.flag(row, "nap") {
                continue;
            }
            let Some(strap_start) = t.int(row, "start_epoch") else {
                unreadable += 1;
                continue;
            };
            let edit = edits.get(&strap_start);
            if edit.is_some_and(|(deleted, _, _)| *deleted) {
                continue;
            }
            let start = edit.and_then(|e| e.1).unwrap_or(strap_start);
            let end = edit.and_then(|e| e.2).or_else(|| t.int(row, "end_epoch"));
            let (Some(start_time), Some(end_time)) = (epoch(start), end.and_then(epoch)) else {
                unreadable += 1;
                continue;
            };
            if end_time <= start_time {
                unreadable += 1;
                continue;
            }
            let minutes = |column: &str| {
                t.int(row, column)
                    .filter(|m| *m >= 0)
                    .and_then(|m| i32::try_from(m).ok())
            };
            let mut slices = stages.remove(&strap_start).unwrap_or_default();
            slices.sort_by_key(|slice| slice.start_time);
            health.sleeps.push(SleepSession {
                sleep_id: format!("{UNSTRAP_DEVICE}:{start}"),
                start_time,
                end_time,
                // The score comes from the unstrap / strap algorithm. It does not replace the Zepp sleep score.
                score: None,
                duration_minutes: ((end_time - start_time).num_minutes()) as i32,
                deep_minutes: minutes("deep_min"),
                light_minutes: minutes("light_min"),
                rem_minutes: minutes("rem_min"),
                awake_minutes: minutes("awake_min"),
                source_scope: SourceScope::Device,
                device_id: Some(UNSTRAP_DEVICE.to_string()),
                synced_at: None,
                time_in_bed_minutes: None,
                stages: slices,
                wake_count: minutes("wake_count"),
            });
        }
    }
    (health, unreadable)
}
