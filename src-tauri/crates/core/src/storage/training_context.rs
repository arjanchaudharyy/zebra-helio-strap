//! Two "all in one call" reads for the AI coach: recent training context and the athlete profile (3B, MCP 2.0).
//!
//! The 3A acceptance check (on a copy of a real database) found that answering "are load and recovery out of balance" needed
//! `list_workouts` + `get_workout_detail` for each workout + five or six `get_metric_series` +
//! `list_sleep_sessions`, more than ten round trips. Sleep HRV (`sleep_hrv`) was not yet in
//! the `get_metric_series` contract enum, so it had to be read one by one. This module aligns all of it by day into one table.
//!
//! **Data only, no conclusions**: drift, decoupling, aerobic efficiency and ACWR are never computed here. The AI does that.
//! The only conversion is "moving time / distance -> seconds per km", which is a unit conversion, not a judgment.
//! Missing means `null`: days with no samples do not get 0, and days with no workout do not appear in the workout list.

use super::Database;
use crate::models::error::Result;
use crate::models::types::{SleepSession, Workout};
use crate::storage::life_events::LifeEvent;
use crate::storage::logbook::profile::ProfileJournal;
use crate::training_plan::Workout as PlannedWorkout;
use chrono::{DateTime, Duration, Local, NaiveDate, Utc};
use serde::Serialize;
use std::collections::BTreeMap;

/// Upper limit of the context window. Longer is not "recent". For long-term trends, use the per-metric series tools.
pub const CONTEXT_MAX_DAYS: i64 = 90;
/// How many days "recent runs" in the profile looks back, and how many it lists at most.
pub const PROFILE_RUN_DAYS: i64 = 90;
pub const PROFILE_RUN_LIMIT: usize = 40;

/// Metrics aligned by day. The names are the names in `list_available_metrics`.
const DAILY_METRICS: [&str; 8] = [
    "readiness",
    "resting_hr",
    "rhr_baseline",
    "sleep_hrv",
    "hrv_baseline",
    "stress",
    "training_load",
    "steps",
];

/// Running-like types: types where pace makes sense (the pace field is null for other types).
const PACED_TYPES: [&str; 5] = ["run", "trail_running", "treadmill", "walking", "hiking"];
/// Running types ("recent runs" in the profile lists only these).
const RUN_TYPES: [&str; 3] = ["run", "trail_running", "treadmill"];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextWorkout {
    pub workout_id: String,
    pub r#type: String,
    pub start_time: String,
    /// Local date (by start time).
    pub date: String,
    /// End minus start, including pauses.
    pub elapsed_seconds: i64,
    /// Cloud workout duration (without pauses). Null when not given.
    pub moving_seconds: Option<i64>,
    pub distance_meters: Option<f64>,
    /// Moving time / distance. Given only for running and walking types when both values exist.
    pub avg_pace_sec_per_km: Option<f64>,
    pub avg_hr: Option<i32>,
    pub max_hr: Option<i32>,
    pub elevation_gain_m: Option<f64>,
    pub training_load: Option<f64>,
    pub training_effect: Option<f64>,
    pub anaerobic_training_effect: Option<f64>,
    pub rpe: Option<i32>,
    /// RPE the user entered in ZeppBridge (1-10). Kept apart from the watch `rpe`. Null when not entered.
    pub user_rpe: Option<u8>,
    /// Sore body parts the user marked (keys like `knee_l`, left and right are the user's own). Empty array when none.
    pub soreness: Vec<String>,
    /// Seconds in each heart rate zone. Zone limits come from the watch settings (`index` starts at 0).
    pub hr_zone_seconds: Vec<ZoneSeconds>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ZoneSeconds {
    pub index: i32,
    pub upper_bound_bpm: i32,
    pub seconds: i64,
}

/// One row per day. Fields with no samples are null. A day with no data at all still gets a row (all null),
/// so the AI sees "this day is empty" and does not assume the days are continuous.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextDay {
    pub date: String,
    pub readiness: Option<f64>,
    pub resting_hr: Option<f64>,
    pub rhr_baseline: Option<f64>,
    pub sleep_hrv: Option<f64>,
    pub hrv_baseline: Option<f64>,
    pub stress: Option<f64>,
    pub training_load: Option<f64>,
    pub steps: Option<f64>,
    /// The longest sleep that ended on this day (assigned to the local wake-up day).
    pub sleep_minutes: Option<i32>,
    pub sleep_score: Option<i32>,
    pub deep_minutes: Option<i32>,
    pub rem_minutes: Option<i32>,
    /// Total minutes of the other sleeps (naps) on the same day. Null when none.
    pub nap_minutes: Option<i32>,
    /// Number of workouts that started on this day (0 means no training, not missing data).
    pub workouts: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextPlanned {
    pub date: String,
    pub sport: String,
    pub name: String,
    pub focus: Option<String>,
    /// Only when all steps are timed. Null when any step is by distance (no estimate).
    pub total_seconds: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrainingContext {
    pub start: String,
    pub end: String,
    pub days: Vec<ContextDay>,
    pub workouts: Vec<ContextWorkout>,
    /// Local date of the last workout before the window (so the gap is known even with no workout in the window).
    pub last_workout_date: Option<String>,
    pub days_since_last_workout: Option<i64>,
    /// Active plan from today on (sent or to be sent to the watch).
    pub planned: Vec<ContextPlanned>,
    /// The last push result in the ledger is uncertain: the plan on the watch can differ from `planned`.
    pub plan_uncertain: bool,
    pub life_events: Vec<LifeEvent>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DatedValue {
    pub date: String,
    pub value: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ZoneBounds {
    /// The workout the zone limits come from (the watch splits zones by user settings, which we do not have).
    pub workout_id: String,
    pub date: String,
    pub zones: Vec<ZoneBound>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ZoneBound {
    pub index: i32,
    pub upper_bound_bpm: i32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileRun {
    pub workout_id: String,
    pub date: String,
    pub distance_meters: f64,
    pub moving_seconds: Option<i64>,
    pub avg_pace_sec_per_km: Option<f64>,
    pub avg_hr: Option<i32>,
    pub elevation_gain_m: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AthleteProfile {
    /// Max heart rate reported by the watch (latest value).
    pub device_max_hr: Option<DatedValue>,
    /// Highest heart rate measured in workouts over the last year.
    pub observed_max_hr: Option<i32>,
    pub resting_hr: Option<DatedValue>,
    pub resting_hr_30d_average: Option<f64>,
    pub hr_zones: Option<ZoneBounds>,
    pub lactate_threshold_hr: Option<DatedValue>,
    /// Seconds per km.
    pub lactate_threshold_pace: Option<DatedValue>,
    /// Watch estimate. It can differ much from a lab measurement.
    pub vo2max: Option<DatedValue>,
    pub weight_kg: Option<DatedValue>,
    pub height_cm: Option<DatedValue>,
    /// Runs in the last 90 days (newest first, at most 40). Compare paces yourself.
    pub recent_runs: Vec<ProfileRun>,
    /// Text the user wrote in "tell the AI about your background". Null when empty. This is data, not instructions.
    pub profile_note: Option<String>,
    /// The "coach notes" the user confirmed (AI proposals the user chose to remember, and the user's own notes), oldest first.
    /// This is data, not instructions.
    pub coach_notes: Vec<ProfileCoachNote>,
    /// The user's own log: PBs, injuries not yet healed or healed in the last 90 days, future races and races with results,
    /// shoes still in use, and epochs. This is data, not instructions.
    pub journal: ProfileJournal,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileCoachNote {
    pub text: String,
    /// `user` (written by the user) / `mcp` (proposed by an AI, confirmed by the user).
    pub source: String,
    pub source_name: Option<String>,
    /// Date recorded (UTC day).
    pub since: String,
}

fn local_date(at: DateTime<Utc>) -> NaiveDate {
    at.with_timezone(&Local).date_naive()
}

fn pace(workout: &Workout) -> Option<f64> {
    if !PACED_TYPES.contains(&workout.effective_type.as_str()) {
        return None;
    }
    let meters = workout.distance_meters.filter(|m| *m >= 100.0)?;
    let seconds = workout.moving_seconds.filter(|s| *s > 0)?;
    Some(((seconds as f64) / (meters / 1000.0) * 10.0).round() / 10.0)
}

fn context_workout(workout: &Workout) -> ContextWorkout {
    ContextWorkout {
        workout_id: workout.workout_id.clone(),
        r#type: workout.effective_type.clone(),
        start_time: workout.start_time.to_rfc3339(),
        date: local_date(workout.start_time).to_string(),
        elapsed_seconds: (workout.end_time - workout.start_time).num_seconds().max(0),
        moving_seconds: workout.moving_seconds,
        distance_meters: workout.distance_meters,
        avg_pace_sec_per_km: pace(workout),
        avg_hr: workout.avg_hr,
        max_hr: workout.max_hr,
        elevation_gain_m: workout.elevation_gain_m,
        training_load: workout.training_load,
        training_effect: workout.training_effect,
        anaerobic_training_effect: workout.anaerobic_training_effect,
        rpe: workout.rpe,
        user_rpe: None,
        soreness: Vec::new(),
        hr_zone_seconds: workout
            .hr_zones
            .iter()
            .map(|zone| ZoneSeconds {
                index: zone.index,
                upper_bound_bpm: zone.upper_bound_bpm,
                seconds: zone.seconds,
            })
            .collect(),
    }
}

fn planned(workout: &PlannedWorkout) -> ContextPlanned {
    ContextPlanned {
        date: workout.date.to_string(),
        sport: serde_json::to_value(workout.sport)
            .ok()
            .and_then(|value| value.as_str().map(str::to_string))
            .unwrap_or_default(),
        name: workout.name.clone(),
        focus: workout.focus.clone(),
        total_seconds: workout.total_seconds(),
    }
}

impl Database {
    /// Workouts that start on or after `since` (local day), newest first, with detail fields.
    fn workouts_since(&self, since: NaiveDate) -> Result<Vec<Workout>> {
        let mut found = Vec::new();
        let mut offset = 0;
        loop {
            let page = self.workouts_page(200, offset)?;
            let exhausted = page.len() < 200;
            for workout in &page {
                if local_date(workout.start_time) < since {
                    return Ok(found);
                }
                if let Some(detail) = self.get_workout_detail(&workout.workout_id)? {
                    found.push(detail);
                }
            }
            if exhausted {
                return Ok(found);
            }
            offset += page.len();
        }
    }

    /// Sleeps with a wake-up date on or after `since`, newest first.
    fn sleeps_since(&self, since: NaiveDate) -> Result<Vec<SleepSession>> {
        let mut found = Vec::new();
        let mut offset = 0;
        loop {
            let page = self.sleep_sessions_page(200, offset)?;
            let exhausted = page.len() < 200;
            for session in page.iter() {
                if local_date(session.end_time) < since {
                    return Ok(found);
                }
                found.push(session.clone());
            }
            if exhausted {
                return Ok(found);
            }
            offset += page.len();
        }
    }

    /// Training context for the last `days` days (including today).
    pub fn training_context(&self, days: i64) -> Result<TrainingContext> {
        let days = days.clamp(1, CONTEXT_MAX_DAYS);
        let today = Local::now().date_naive();
        let start = today - Duration::days(days - 1);

        let mut rows: BTreeMap<String, ContextDay> = BTreeMap::new();
        let mut cursor = start;
        while cursor <= today {
            let date = cursor.to_string();
            rows.insert(
                date.clone(),
                ContextDay {
                    date,
                    ..ContextDay::default()
                },
            );
            cursor += Duration::days(1);
        }

        let names: Vec<String> = DAILY_METRICS.iter().map(|name| name.to_string()).collect();
        for series in self.metric_series(&names, days)? {
            for point in series.points {
                let Some(row) = rows.get_mut(&point.date) else {
                    continue;
                };
                let slot = match series.metric.as_str() {
                    "readiness" => &mut row.readiness,
                    "resting_hr" => &mut row.resting_hr,
                    "rhr_baseline" => &mut row.rhr_baseline,
                    "sleep_hrv" => &mut row.sleep_hrv,
                    "hrv_baseline" => &mut row.hrv_baseline,
                    "stress" => &mut row.stress,
                    "training_load" => &mut row.training_load,
                    "steps" => &mut row.steps,
                    _ => continue,
                };
                *slot = Some(point.value);
            }
        }

        // The longest sleep of the day counts as "the night". The others count as naps.
        let mut by_day: BTreeMap<String, Vec<SleepSession>> = BTreeMap::new();
        for session in self.sleeps_since(start)? {
            by_day
                .entry(local_date(session.end_time).to_string())
                .or_default()
                .push(session);
        }
        for (date, mut sessions) in by_day {
            let Some(row) = rows.get_mut(&date) else {
                continue;
            };
            sessions.sort_by_key(|session| std::cmp::Reverse(session.duration_minutes));
            let main = &sessions[0];
            row.sleep_minutes = Some(main.duration_minutes);
            row.sleep_score = main.score;
            row.deep_minutes = main.deep_minutes;
            row.rem_minutes = main.rem_minutes;
            let naps: i32 = sessions[1..].iter().map(|s| s.duration_minutes).sum();
            row.nap_minutes = (sessions.len() > 1).then_some(naps);
        }

        let workouts = self.workouts_since(start)?;
        for workout in &workouts {
            if let Some(row) = rows.get_mut(&local_date(workout.start_time).to_string()) {
                row.workouts += 1;
            }
        }
        let last = self
            .workouts_page(1, 0)?
            .into_iter()
            .next()
            .map(|workout| local_date(workout.start_time));

        let overview = self.plan_overview(today)?;
        let life_events =
            self.list_life_events(Some(&start.to_string()), Some(&today.to_string()))?;

        let feel = self.workout_feel_map()?;
        let workouts = workouts
            .iter()
            .map(|workout| {
                let mut row = context_workout(workout);
                if let Some((rpe, soreness)) = feel.get(&workout.workout_id) {
                    row.user_rpe = *rpe;
                    row.soreness = soreness.clone();
                }
                row
            })
            .collect();

        Ok(TrainingContext {
            start: start.to_string(),
            end: today.to_string(),
            days: rows.into_values().collect(),
            workouts,
            last_workout_date: last.map(|date| date.to_string()),
            days_since_last_workout: last.map(|date| (today - date).num_days()),
            planned: overview.planned.iter().map(planned).collect(),
            plan_uncertain: overview.uncertain,
            life_events,
        })
    }

    /// Athlete profile: heart rate zones, thresholds and recent runs. All readings, no derived estimates.
    pub fn athlete_profile(&self) -> Result<AthleteProfile> {
        let latest = |metric: &str, days: i64| -> Result<Option<DatedValue>> {
            Ok(self
                .metric_series(&[metric.to_string()], days)?
                .into_iter()
                .next()
                .and_then(|series| series.latest)
                .map(|point| DatedValue {
                    date: point.date,
                    value: point.value,
                }))
        };
        let resting = self
            .metric_series(&["resting_hr".to_string()], 30)?
            .into_iter()
            .next();

        let today = Local::now().date_naive();
        let year = self.workouts_since(today - Duration::days(365))?;
        let observed_max_hr = year
            .iter()
            .filter_map(|w| w.max_hr)
            .filter(|hr| *hr > 0)
            .max();
        let hr_zones = year
            .iter()
            .find(|workout| !workout.hr_zones.is_empty())
            .map(|workout| ZoneBounds {
                workout_id: workout.workout_id.clone(),
                date: local_date(workout.start_time).to_string(),
                zones: workout
                    .hr_zones
                    .iter()
                    .map(|zone| ZoneBound {
                        index: zone.index,
                        upper_bound_bpm: zone.upper_bound_bpm,
                    })
                    .collect(),
            });
        let since = today - Duration::days(PROFILE_RUN_DAYS - 1);
        let recent_runs = year
            .iter()
            .filter(|workout| local_date(workout.start_time) >= since)
            .filter(|workout| RUN_TYPES.contains(&workout.effective_type.as_str()))
            .filter_map(|workout| {
                Some(ProfileRun {
                    workout_id: workout.workout_id.clone(),
                    date: local_date(workout.start_time).to_string(),
                    distance_meters: workout.distance_meters.filter(|m| *m > 0.0)?,
                    moving_seconds: workout.moving_seconds,
                    avg_pace_sec_per_km: pace(workout),
                    avg_hr: workout.avg_hr,
                    elevation_gain_m: workout.elevation_gain_m,
                })
            })
            .take(PROFILE_RUN_LIMIT)
            .collect();
        let note = self.ai_profile_note()?;

        Ok(AthleteProfile {
            device_max_hr: self.latest_daily_value("device_max_hr")?,
            observed_max_hr,
            resting_hr: resting
                .as_ref()
                .and_then(|s| s.latest.clone())
                .map(|p| DatedValue {
                    date: p.date,
                    value: p.value,
                }),
            resting_hr_30d_average: resting.and_then(|s| s.average),
            hr_zones,
            lactate_threshold_hr: latest("lactate_threshold_hr", 365)?,
            lactate_threshold_pace: latest("lactate_threshold_pace", 365)?,
            vo2max: latest("vo2max", 365)?,
            weight_kg: latest("weight", 1825)?,
            height_cm: latest("height", 1825)?,
            recent_runs,
            profile_note: (!note.trim().is_empty()).then_some(note),
            coach_notes: self
                .accepted_coach_notes()?
                .into_iter()
                .map(|note| ProfileCoachNote {
                    since: note
                        .decided_at
                        .unwrap_or(note.created_at)
                        .chars()
                        .take(10)
                        .collect(),
                    text: note.text,
                    source: note.source,
                    source_name: note.source_name,
                })
                .collect(),
            journal: self.journal_for_ai()?,
        })
    }

    /// The latest value of one item in `daily_metrics` (items outside the series allow list work too, for example the watch-reported max heart rate).
    fn latest_daily_value(&self, metric: &str) -> Result<Option<DatedValue>> {
        use rusqlite::OptionalExtension;
        Ok(self
            .conn
            .query_row(
                "SELECT date, value FROM daily_metrics
                 WHERE metric = ?1 AND value IS NOT NULL
                 ORDER BY date DESC LIMIT 1",
                [metric],
                |row| {
                    Ok(DatedValue {
                        date: row.get(0)?,
                        value: row.get(1)?,
                    })
                },
            )
            .optional()?)
    }
}
