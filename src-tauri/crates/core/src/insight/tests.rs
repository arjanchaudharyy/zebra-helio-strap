use super::*;
use crate::models::{DailyMetric, SleepSession, SourceScope, Workout};
use chrono::{Datelike, TimeZone};

fn base() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 8, 20, 7, 0, 0).unwrap()
}

fn db() -> Database {
    Database::in_memory().unwrap()
}

/// Build a controlled run: `hr` is a function that returns the heart rate for the progress. The speed is fixed.
fn drift_workout(db: &Database, workout_id: &str, seconds: i64, speed: f64, hr: fn(f64) -> f64) {
    let start = Utc.with_ymd_and_hms(2026, 8, 24, 6, 0, 0).unwrap();
    db.conn
        .execute(
            "INSERT INTO workouts (workout_id, workout_type, start_time, end_time,
                                       source_scope, synced_at)
                 VALUES (?1, 'run', ?2, ?3, 'device', ?3)",
            rusqlite::params![
                workout_id,
                start.to_rfc3339(),
                (start + Duration::seconds(seconds)).to_rfc3339()
            ],
        )
        .unwrap();
    for second in 0..=seconds {
        let progress = second as f64 / seconds as f64;
        db.conn
            .execute(
                "INSERT INTO workout_samples (workout_id, timestamp, heart_rate, speed)
                     VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![
                    workout_id,
                    (start + Duration::seconds(second)).to_rfc3339(),
                    hr(progress),
                    speed
                ],
            )
            .unwrap();
    }
}

/// Steady speed while the heart rate keeps rising. This is the definition of heart rate drift: the same speed costs more heartbeats.
#[test]
fn a_steady_pace_with_a_rising_heart_rate_reads_as_drift() {
    let db = Database::in_memory().unwrap();
    // 40 minutes, steady pace of 3 m/s, heart rate rises linearly from 140 to 160.
    drift_workout(&db, "w1", 2400, 3.0, |p| 140.0 + 20.0 * p);

    let drift = db.heart_rate_drift("w1").unwrap().expect("the conditions are met");

    assert_eq!(drift.first_half_avg_speed_mps, 3.0);
    assert_eq!(drift.second_half_avg_speed_mps, 3.0);
    assert!(
        drift.second_half_avg_hr > drift.first_half_avg_hr,
        "the second half heart rate must be higher: {} vs {}",
        drift.second_half_avg_hr,
        drift.first_half_avg_hr
    );
    // The heart rate rose and the speed did not change, so the meters per beat must drop. Drift is negative.
    assert!(
        drift.drift_percent < 0.0,
        "the same speed cost more heartbeats, so drift must be negative: {}",
        drift.drift_percent
    );
    // Heart rate near 145 -> 155, so the efficiency difference is about -6.5%. A wide range pins the direction and magnitude.
    assert!(
        (-9.0..=-4.0).contains(&drift.drift_percent),
        "wrong magnitude: {}",
        drift.drift_percent
    );
    assert!(drift.speed_cv < 1e-9, "steady speed, so the coefficient of variation must be 0");
}

/// Steady heart rate and speed means no drift. Then it must report near 0, not "no data".
#[test]
fn a_steady_effort_reads_as_no_drift() {
    let db = Database::in_memory().unwrap();
    drift_workout(&db, "w1", 2400, 3.0, |_| 150.0);

    let drift = db.heart_rate_drift("w1").unwrap().expect("the conditions are met");
    assert!(
        drift.drift_percent.abs() < 0.001,
        "heart rate and speed did not change, so drift must be 0: {}",
        drift.drift_percent
    );
}

/// Too short does not count. In the first ten minutes the heart rate still rises, so a comparison measures the warm-up.
#[test]
fn a_short_workout_says_so_instead_of_reporting_a_number() {
    let db = Database::in_memory().unwrap();
    // 15 minutes: enough samples but not enough duration.
    drift_workout(&db, "w1", 900, 3.0, |p| 140.0 + 20.0 * p);

    assert_eq!(db.heart_rate_drift("w1").unwrap().unwrap_err(), "too_short");
}

/// An uneven pace does not count: traffic lights, intervals and hills look like this, and the halves are not comparable.
#[test]
fn a_variable_pace_refuses_to_produce_a_percentage() {
    let db = Database::in_memory().unwrap();
    let start = Utc.with_ymd_and_hms(2026, 8, 24, 6, 0, 0).unwrap();
    db.conn
        .execute(
            "INSERT INTO workouts (workout_id, workout_type, start_time, end_time,
                                       source_scope, synced_at)
                 VALUES ('w1', 'run', ?1, ?2, 'device', ?2)",
            rusqlite::params![
                start.to_rfc3339(),
                (start + Duration::seconds(2400)).to_rfc3339()
            ],
        )
        .unwrap();
    for second in 0..=2400 {
        // Switch between 1.5 and 4.5 m/s every 30 seconds: typical intervals.
        let speed = if (second / 30) % 2 == 0 { 1.5 } else { 4.5 };
        db.conn
            .execute(
                "INSERT INTO workout_samples (workout_id, timestamp, heart_rate, speed)
                     VALUES ('w1', ?1, 150, ?2)",
                rusqlite::params![(start + Duration::seconds(second)).to_rfc3339(), speed],
            )
            .unwrap();
    }

    assert_eq!(
        db.heart_rate_drift("w1").unwrap().unwrap_err(),
        "pace_too_variable"
    );
}

/// A workout with no per-point samples says "none" directly and does not guess.
#[test]
fn a_workout_without_samples_says_there_is_nothing_to_compare() {
    let db = Database::in_memory().unwrap();
    let start = Utc.with_ymd_and_hms(2026, 8, 24, 6, 0, 0).unwrap();
    db.conn
        .execute(
            "INSERT INTO workouts (workout_id, workout_type, start_time, end_time,
                                       source_scope, synced_at)
                 VALUES ('w1', 'run', ?1, ?2, 'device', ?2)",
            rusqlite::params![
                start.to_rfc3339(),
                (start + Duration::seconds(2400)).to_rfc3339()
            ],
        )
        .unwrap();

    assert_eq!(
        db.heart_rate_drift("w1").unwrap().unwrap_err(),
        "not_enough_samples"
    );
}

/// Single-digit heart rates from a poor fit, and 0 speed during stops, must not enter the average.
#[test]
fn implausible_readings_are_dropped_before_the_halves_are_compared() {
    let db = Database::in_memory().unwrap();
    let start = Utc.with_ymd_and_hms(2026, 8, 24, 6, 0, 0).unwrap();
    db.conn
        .execute(
            "INSERT INTO workouts (workout_id, workout_type, start_time, end_time,
                                       source_scope, synced_at)
                 VALUES ('w1', 'run', ?1, ?2, 'device', ?2)",
            rusqlite::params![
                start.to_rfc3339(),
                (start + Duration::seconds(2400)).to_rfc3339()
            ],
        )
        .unwrap();
    for second in 0..=2400 {
        // Insert a poor-fit reading every 100 seconds: heart rate 5, speed 0.
        let (hr, speed) = if second % 100 == 0 {
            (5.0, 0.0)
        } else {
            (150.0, 3.0)
        };
        db.conn
            .execute(
                "INSERT INTO workout_samples (workout_id, timestamp, heart_rate, speed)
                     VALUES ('w1', ?1, ?2, ?3)",
                rusqlite::params![(start + Duration::seconds(second)).to_rfc3339(), hr, speed],
            )
            .unwrap();
    }

    let drift = db
        .heart_rate_drift("w1")
        .unwrap()
        .expect("the conditions are still met after the bad points are dropped");
    assert_eq!(
        drift.first_half_avg_hr, 150.0,
        "the points with heart rate 5 must not enter the average"
    );
    assert_eq!(drift.first_half_avg_speed_mps, 3.0, "same for the points with speed 0");
    assert!(
        drift.speed_cv < 1e-9,
        "after the bad points are dropped the speed is steady, so the coefficient of variation must be 0: {}",
        drift.speed_cv
    );
}

/// One run. `minutes` is the duration, `distance` is in meters.
fn run(
    id: &str,
    days_ago: i64,
    distance: Option<f64>,
    minutes: i64,
    avg_hr: Option<i32>,
) -> Workout {
    let start = base() - Duration::days(days_ago);
    Workout {
        workout_id: id.into(),
        workout_type: "run".into(),
        normalized_type: "run".into(),
        type_source: "numeric_mapped".into(),
        user_override: None,
        effective_type: "run".into(),
        custom_label: None,
        start_time: start,
        end_time: start + Duration::minutes(minutes),
        distance_meters: distance,
        calories: None,
        avg_hr,
        max_hr: None,
        training_load: Some(50.0),
        vo2max: None,
        source_scope: SourceScope::Device,
        device_id: Some("device-a".into()),
        synced_at: None,
        gps_available: false,
        sample_count: 0,
        zepp_source: None,
        zepp_type: Some(1),
        ..Default::default()
    }
}

fn fact<'a>(insight: &'a WorkoutInsight, id: &str) -> &'a InsightFact {
    insight
        .facts
        .iter()
        .find(|fact| fact.fact_id == id)
        .unwrap_or_else(|| panic!("missing fact {id}"))
}

fn weekly_fact<'a>(report: &'a WeeklyReport, id: &str) -> &'a InsightFact {
    report
        .facts
        .iter()
        .find(|fact| fact.fact_id == id)
        .unwrap_or_else(|| panic!("missing fact {id}"))
}

#[test]
fn only_verified_running_gets_an_insight_and_the_rest_says_so_plainly() {
    let db = db();
    let mut strength = run("strength-1", 1, Some(0.0), 40, Some(120));
    strength.workout_type = "strength".into();
    strength.normalized_type = "strength".into();
    strength.effective_type = "strength".into();
    strength.zepp_type = Some(52);
    db.insert_workout(&strength).unwrap();

    let insight = db.workout_insight("strength-1").unwrap();
    assert!(!insight.supported);
    assert!(insight.facts.is_empty(), "an unsupported type must not invent facts");
    let reason = insight.unsupported_reason.unwrap();
    assert!(reason.contains("running"));
}

#[test]
fn a_run_without_enough_comparable_history_reports_the_value_but_no_percentage() {
    let db = db();
    db.insert_workout(&run("target", 0, Some(5000.0), 30, Some(150)))
        .unwrap();
    // Only two comparable past runs, below MIN_SAMPLES.
    db.insert_workout(&run("prev-1", 7, Some(5100.0), 31, Some(152)))
        .unwrap();
    db.insert_workout(&run("prev-2", 14, Some(4900.0), 32, Some(151)))
        .unwrap();

    let insight = db.workout_insight("target").unwrap();
    assert!(insight.supported);
    let pace = fact(&insight, "run.pace");
    assert!(pace.value.is_some(), "this run's value must still be given");
    assert!(pace.comparison.is_none(), "with too few samples, no forced percentage");
    assert_eq!(pace.confidence, Confidence::Insufficient);
    let reason = pace.reason.clone().unwrap();
    assert!(reason.contains("fewer than"), "{reason}");
    assert_eq!(pace.evidence_count, 2);
}

#[test]
fn the_baseline_only_uses_runs_of_a_similar_distance_and_says_what_it_dropped() {
    let db = db();
    db.insert_workout(&run("target", 0, Some(5000.0), 30, Some(150)))
        .unwrap();
    for (index, distance) in [5100.0, 4900.0, 5000.0].into_iter().enumerate() {
        db.insert_workout(&run(
            &format!("near-{index}"),
            (index as i64 + 1) * 3,
            Some(distance),
            31,
            Some(150),
        ))
        .unwrap();
    }
    // Distance too far off: a 10 km run must not be compared with a 5 km run.
    db.insert_workout(&run("far", 20, Some(10000.0), 62, Some(150)))
        .unwrap();
    // Outside the window.
    db.insert_workout(&run(
        "old",
        baseline::WINDOW_DAYS + 5,
        Some(5000.0),
        30,
        Some(150),
    ))
    .unwrap();

    let insight = db.workout_insight("target").unwrap();
    let included: Vec<&str> = insight
        .baseline_included
        .iter()
        .map(|entry| entry.workout_id.as_str())
        .collect();
    assert_eq!(included.len(), 3, "{included:?}");
    assert!(!included.contains(&"far"));
    assert!(!included.contains(&"old"), "records outside the window must not enter the baseline");

    let dropped: Vec<(&str, &str)> = insight
        .baseline_excluded
        .iter()
        .map(|entry| (entry.workout_id.as_str(), entry.reason.as_str()))
        .collect();
    assert!(
        dropped.contains(&("far", "distance_out_of_tolerance")),
        "{dropped:?}"
    );

    let pace = fact(&insight, "run.pace");
    let comparison = pace.comparison.clone().expect("three comparable records must be enough");
    assert_eq!(pace.evidence_count, 3);
    // This run: 5 km in 30 minutes = 360 s/km. The three baseline runs are all 31 minutes.
    assert!(comparison.delta < 0.0, "this run is faster, so delta must be negative");
    assert_eq!(comparison.direction, "lower");
    assert_eq!(pace.evidence_refs.len(), 3);
}

#[test]
fn a_run_with_an_impossible_pace_never_pollutes_the_baseline() {
    let db = db();
    db.insert_workout(&run("target", 0, Some(5000.0), 30, Some(150)))
        .unwrap();
    for index in 0..3 {
        db.insert_workout(&run(
            &format!("ok-{index}"),
            index + 1,
            Some(5000.0),
            30,
            Some(150),
        ))
        .unwrap();
    }
    // 5 km in 1 minute: the data itself is broken, it is not a very fast run.
    db.insert_workout(&run("broken", 5, Some(5000.0), 1, Some(150)))
        .unwrap();

    let insight = db.workout_insight("target").unwrap();
    assert!(insight
        .baseline_included
        .iter()
        .all(|entry| entry.workout_id != "broken"));
    assert!(insight
        .baseline_excluded
        .iter()
        .any(|entry| entry.workout_id == "broken" && entry.reason == "implausible_pace"));
}

#[test]
fn a_missing_metric_is_missing_not_zero() {
    let db = db();
    db.insert_workout(&run("target", 0, Some(5000.0), 30, None))
        .unwrap();
    for index in 0..3 {
        db.insert_workout(&run(
            &format!("ok-{index}"),
            index + 1,
            Some(5000.0),
            30,
            None,
        ))
        .unwrap();
    }
    let insight = db.workout_insight("target").unwrap();
    let hr = fact(&insight, "run.avg_hr");
    assert_eq!(hr.value, None, "no heart rate means none; do not fill 0");
    assert!(hr.comparison.is_none());
    assert_eq!(hr.evidence_count, 0, "the baseline must not count a missing value as 0 either");
    assert_eq!(hr.confidence, Confidence::Insufficient);
}

#[test]
fn the_baseline_never_grows_past_the_max_sample_count() {
    let db = db();
    db.insert_workout(&run("target", 0, Some(5000.0), 30, Some(150)))
        .unwrap();
    for index in 0..(baseline::MAX_SAMPLES + 4) {
        db.insert_workout(&run(
            &format!("prev-{index}"),
            index as i64 + 1,
            Some(5000.0),
            31,
            Some(150),
        ))
        .unwrap();
    }
    let insight = db.workout_insight("target").unwrap();
    assert_eq!(insight.baseline_included.len(), baseline::MAX_SAMPLES);
    assert!(insight
        .baseline_excluded
        .iter()
        .any(|entry| entry.reason == "beyond_max_samples"));
}

fn sleep(id: &str, days_ago: i64, start_hour: u32, minutes: i64) -> SleepSession {
    let day = (base() - Duration::days(days_ago)).date_naive();
    let start = Local
        .with_ymd_and_hms(day.year(), day.month(), day.day(), start_hour, 0, 0)
        .unwrap()
        .with_timezone(&Utc);
    SleepSession {
        sleep_id: id.into(),
        start_time: start,
        end_time: start + Duration::minutes(minutes),
        score: None,
        duration_minutes: minutes as i32,
        deep_minutes: Some(0),
        light_minutes: Some(minutes as i32),
        rem_minutes: None,
        awake_minutes: Some(0),
        synced_at: None,
        time_in_bed_minutes: None,
        wake_count: None,
        stages: Vec::new(),
        source_scope: SourceScope::Device,
        device_id: Some("device-a".into()),
    }
}

#[test]
fn the_weekly_report_compares_you_with_your_own_previous_four_weeks() {
    let db = db();
    // Last 7 days: 7 hours each night. Previous 28 days: 6 hours each night.
    for day in 0..7 {
        db.insert_sleep_session(&sleep(&format!("recent-{day}"), day, 23, 420))
            .unwrap();
    }
    // A night belongs to the day you wake up, so a night that starts at 23:00 counts for the next day.
    // The baseline starts on day 8, inside the previous 28-day window, and does not leak into the last 7 days.
    for day in 8..36 {
        db.insert_sleep_session(&sleep(&format!("base-{day}"), day, 23, 360))
            .unwrap();
    }

    let report = db
        .weekly_report_for_day(base().date_naive(), base())
        .unwrap();
    let duration = weekly_fact(&report, "weekly.sleep_duration");
    assert_eq!(duration.value, Some(420.0));
    let comparison = duration.comparison.clone().expect("the baseline has 28 days");
    assert_eq!(comparison.baseline_value, 360.0);
    assert_eq!(comparison.direction, "higher");
    assert!(
        (comparison.delta_percent - 16.7).abs() < 0.2,
        "{comparison:?}"
    );
    assert_eq!(duration.confidence, Confidence::High);
}

#[test]
fn a_thin_baseline_reports_the_current_value_without_a_comparison() {
    let db = db();
    for day in 0..7 {
        db.insert_sleep_session(&sleep(&format!("recent-{day}"), day, 23, 420))
            .unwrap();
    }
    // Only two nights before have records, fewer than MIN_BASELINE_DAYS.
    for day in 10..12 {
        db.insert_sleep_session(&sleep(&format!("base-{day}"), day, 23, 360))
            .unwrap();
    }
    let report = db
        .weekly_report_for_day(base().date_naive(), base())
        .unwrap();
    let duration = weekly_fact(&report, "weekly.sleep_duration");
    assert_eq!(duration.value, Some(420.0));
    assert!(duration.comparison.is_none());
    assert_eq!(duration.confidence, Confidence::Insufficient);
    assert!(duration.reason.clone().unwrap().contains("fewer than"));
}

#[test]
fn no_data_at_all_says_no_data_instead_of_zero() {
    let db = db();
    let report = db
        .weekly_report_for_day(base().date_naive(), base())
        .unwrap();
    for fact in &report.facts {
        assert_eq!(
            fact.value, None,
            "{} must not give a value when there is no data",
            fact.fact_id
        );
        assert!(fact.comparison.is_none());
        assert_eq!(fact.confidence, Confidence::Insufficient);
    }
}

#[test]
fn the_report_never_mentions_a_population_baseline() {
    let db = db();
    for day in 0..7 {
        db.insert_sleep_session(&sleep(&format!("recent-{day}"), day, 23, 420))
            .unwrap();
    }
    let report = db
        .weekly_report_for_day(base().date_naive(), base())
        .unwrap();
    let encoded = serde_json::to_string(&report).unwrap();
    for forbidden in [
        "population",
        "normal people",
        "average level",
        "healthy people",
        "diagnosis",
        "disease",
        "risk",
    ] {
        assert!(
            !encoded.contains(forbidden),
            "the weekly report uses wording with no local basis: {forbidden}"
        );
    }
    // The baseline window must state that it is "your own previous 28 days".
    let duration = weekly_fact(&report, "weekly.sleep_duration");
    assert_eq!(
        duration.baseline_window.clone().unwrap().kind,
        "previous_days"
    );
}

#[test]
fn sleep_regularity_does_not_read_midnight_as_a_twenty_three_hour_swing() {
    let db = db();
    // 23:50 and 00:10 are 20 minutes apart, not 23 hours 40 minutes.
    db.insert_sleep_session(&sleep("a", 1, 23, 400)).unwrap();
    db.insert_sleep_session(&sleep("b", 2, 0, 400)).unwrap();
    db.insert_sleep_session(&sleep("c", 3, 23, 400)).unwrap();
    let report = db
        .weekly_report_for_day(base().date_naive(), base())
        .unwrap();
    let regularity = weekly_fact(&report, "weekly.sleep_start_regularity");
    let spread = regularity.value.expect("three nights are enough to compute the spread");
    assert!(
        spread < 120.0,
        "the midnight crossing was read as a 23-hour swing: {spread} minutes"
    );
}

/// When start_time is near the lower time limit, the baseline window subtraction must not panic.
#[test]
fn a_run_near_datetime_min_does_not_panic_on_baseline_window() {
    let db = db();
    let year_one = Utc.with_ymd_and_hms(1, 1, 1, 12, 0, 0).unwrap();
    let mut workout = run("year-one", 0, Some(5000.0), 30, Some(150));
    workout.start_time = year_one;
    workout.end_time = year_one + Duration::minutes(30);
    db.insert_workout(&workout).unwrap();
    let insight = db.workout_insight("year-one").unwrap();
    assert!(insight.supported);
    assert!(insight.baseline_included.is_empty());

    let target = RunRow {
        workout_id: "min".into(),
        start_time: DateTime::<Utc>::MIN_UTC,
        end_time: DateTime::<Utc>::MIN_UTC
            .checked_add_signed(Duration::try_minutes(30).unwrap())
            .expect("MIN + 30 minutes is still in range"),
        distance_meters: Some(5000.0),
        avg_hr: Some(150),
        training_load: Some(50.0),
        source_scope: "device".into(),
    };
    let (included, excluded) = db.comparable_runs(&target).unwrap();
    assert!(included.is_empty());
    assert!(
        excluded.iter().any(|row| row.reason == "outside_window"),
        "an underflow must record outside_window: {excluded:?}"
    );

    let report = db
        .weekly_report_for_day(
            DateTime::<Utc>::MIN_UTC.date_naive(),
            DateTime::<Utc>::MIN_UTC,
        )
        .unwrap();
    assert_eq!(
        report.recent_end,
        DateTime::<Utc>::MIN_UTC.date_naive().to_string()
    );
}

/// One daily_metrics row. `source_scope` can change, to build "two sources on one day".
fn daily(days_ago: i64, metric: &str, value: f64, scope: SourceScope) -> DailyMetric {
    DailyMetric {
        date: (base() - Duration::days(days_ago)).date_naive().to_string(),
        metric: metric.into(),
        value,
        unit: "bpm".into(),
        source_scope: scope,
        device_id: None,
    }
}

/// Two sources with one row each on the same day are one day of data, not two. The mean folds per day,
/// and the extra row must not go directly into the window average.
#[test]
fn the_same_day_from_two_scopes_is_one_day_of_evidence() {
    let db = db();
    // Each of the last 7 days has a resting heart rate, so the comparison reaches the baseline check.
    for day in 0..7 {
        db.insert_daily_metric(&daily(day, "resting_hr", 60.0, SourceScope::Device))
            .unwrap();
    }
    // Baseline of 10 days: one day has a device row and a user_fused row, and still counts as 1 day.
    // 50 and 100 differ enough: a per-row average gives 54.5, and the per-day fold gives 52.5.
    for day in 8..18 {
        db.insert_daily_metric(&daily(day, "resting_hr", 50.0, SourceScope::Device))
            .unwrap();
    }
    db.insert_daily_metric(&daily(8, "resting_hr", 100.0, SourceScope::UserFused))
        .unwrap();

    let report = db
        .weekly_report_for_day(base().date_naive(), base())
        .unwrap();
    let resting = weekly_fact(&report, "weekly.resting_hr");
    assert_eq!(resting.baseline_count, 10, "two sources on one day count as one day");
    let comparison = resting
        .comparison
        .clone()
        .expect("a 10-day baseline passes the 7-day gate, so the comparison must happen");
    // Day 8 is (50+100)/2 = 75, the other 9 days are 50 -> daily mean 52.5.
    assert_eq!(comparison.baseline_value, 52.5);
    assert_eq!(resting.value, Some(60.0));
}

/// After "count" aggregates into one value, the baseline check still counts days with data.
#[test]
fn a_workout_count_baseline_counts_days_with_activity() {
    let db = db();
    // Last 7 days: 4 workouts on 3 days (two on day 0). Previous 10 days: 1 per day.
    for day in [0, 2, 4] {
        db.insert_workout(&run(
            &format!("recent-{day}"),
            day,
            Some(5000.0),
            30,
            Some(150),
        ))
        .unwrap();
    }
    db.insert_workout(&run("recent-0b", 0, Some(5000.0), 30, Some(150)))
        .unwrap();
    for day in 8..18 {
        db.insert_workout(&run(
            &format!("base-{day}"),
            day,
            Some(5000.0),
            30,
            Some(150),
        ))
        .unwrap();
    }
    let report = db
        .weekly_report_for_day(base().date_naive(), base())
        .unwrap();
    let count = weekly_fact(&report, "weekly.workout_count");
    assert_eq!(count.value, Some(4.0), "value is the total workouts, not distinct days");
    assert_eq!(count.evidence_count, 3, "days are counted as distinct dates");
    assert_eq!(count.baseline_count, 10);
    let comparison = count
        .comparison
        .clone()
        .expect("a 10-day baseline passes the 7-day gate, so no thin_baseline");
    // 10 workouts spread over the whole 28-day baseline window. Per 7 days the equivalent count is
    // 10 * 7 / 28 = 2.5, not the raw 10 (that compares a 7-day total with a 28-day total,
    // and the same training frequency reads as "dropped"). 4 against 2.5 is "more".
    assert_eq!(comparison.baseline_value, 2.5);
    assert_eq!(comparison.direction, "higher");
}

/// When the baseline mean is 0, no relative change is possible. This is a different reason, not "too few samples".
#[test]
fn a_zero_baseline_reports_no_comparison_instead_of_dividing_by_zero() {
    let db = db();
    for day in 0..7 {
        db.insert_daily_metric(&daily(day, "resting_hr", 60.0, SourceScope::Device))
            .unwrap();
    }
    // All 10 baseline days are 0: this metric was never measured before.
    for day in 8..18 {
        db.insert_daily_metric(&daily(day, "resting_hr", 0.0, SourceScope::Device))
            .unwrap();
    }
    let report = db
        .weekly_report_for_day(base().date_naive(), base())
        .unwrap();
    let resting = weekly_fact(&report, "weekly.resting_hr");
    assert_eq!(resting.value, Some(60.0));
    assert!(resting.comparison.is_none(), "0 as the divisor has no meaning");
    assert_eq!(resting.reason_code.as_deref(), Some("weekly_zero_baseline"));
    assert_eq!(resting.confidence, Confidence::Insufficient);
}

/// Below half a display unit is noise. A 1 bpm difference that the UI shows as whole numbers must have a direction.
#[test]
fn a_displayed_bpm_difference_is_not_flat() {
    assert_eq!(direction_of(0.4, 60.0), "same");
    assert_eq!(direction_of(-0.4, 51.0), "same");
    assert_eq!(direction_of(1.0, 60.0), "higher");
    assert_eq!(direction_of(-1.0, 51.0), "lower");
}

/// The wellness `hrvRmssd` item is stored as `hrv_rmssd`. The weekly report must read both names,
/// or days with HRV are reported as "no data".
#[test]
fn hrv_samples_stored_under_the_rmssd_name_feed_the_weekly_report() {
    let db = db();
    for day in 0..7 {
        let date = (base() - Duration::days(day)).date_naive();
        db.conn
            .execute(
                "INSERT INTO metric_samples
                        (metric, timestamp, value, unit, source_scope)
                     VALUES ('hrv_rmssd', ?1, 45.0, 'ms', 'device')",
                rusqlite::params![format!("{date}T08:00:00+00:00")],
            )
            .unwrap();
    }
    let report = db
        .weekly_report_for_day(base().date_naive(), base())
        .unwrap();
    let hrv = weekly_fact(&report, "weekly.hrv");
    assert_eq!(hrv.value, Some(45.0), "samples stored as hrv_rmssd must be read");
    assert_ne!(
        hrv.reason_code.as_deref(),
        Some("weekly_no_recent_data"),
        "days with HRV data must not report \"no data\""
    );
}

mod baseline_marks {
    use super::super::anomaly::{metric_baseline, BaselineDirection};
    use chrono::{Duration, NaiveDate};

    fn day(n: i64) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, 6).unwrap() + Duration::days(n)
    }

    /// The previous 28 days vary a little around 50, and the latest day is `latest`.
    fn series(days: i64, latest: f64) -> Vec<(NaiveDate, f64)> {
        let mut points: Vec<(NaiveDate, f64)> = (1..=days)
            .map(|i| (day(-i), 50.0 + ((i % 5) as f64 - 2.0) * 0.5))
            .collect();
        points.push((day(0), latest));
        points
    }

    #[test]
    fn only_compares_with_your_own_recent_month_and_says_how_far() {
        let high = metric_baseline("resting_hr", &series(28, 58.0), day(0)).unwrap();
        assert_eq!(high.direction, BaselineDirection::Above);
        assert_eq!(high.message_code, "ui.metric_baseline.above");
        assert_eq!(high.median, 50.0);
        assert!((high.delta - 8.0).abs() < 1e-9);
        assert_eq!(high.days, 28);
        let low = metric_baseline("hrv", &series(28, 40.0), day(0)).unwrap();
        assert_eq!(low.direction, BaselineDirection::Below);
        let usual = metric_baseline("hrv", &series(28, 50.5), day(0)).unwrap();
        assert_eq!(usual.direction, BaselineDirection::Usual);
    }

    #[test]
    fn too_few_days_or_a_stale_reading_gets_no_mark() {
        assert!(
            metric_baseline("hrv", &series(13, 80.0), day(0)).is_none(),
            "fewer than 14 days: no flag"
        );
        assert!(
            metric_baseline("hrv", &series(28, 80.0), day(4)).is_none(),
            "a latest reading that is too old is not now"
        );
        // One or two extreme days must not pull "usual" away (median + MAD).
        let mut spiky = series(28, 50.0);
        spiky[3].1 = 500.0;
        let mark = metric_baseline("hrv", &spiky, day(0)).unwrap();
        assert_eq!(mark.direction, BaselineDirection::Usual);
    }

    /// The norm band only uses the 90 days before the latest reading, not the latest day: today's outlier must not widen the "norm".
    #[test]
    fn the_usual_band_is_your_own_middle_eighty_percent_without_today() {
        let mut points: Vec<(NaiveDate, f64)> = (1..=100)
            .map(|i| (day(-i), (i % 10) as f64 + 50.0))
            .collect();
        points.push((day(0), 200.0));
        let mark = metric_baseline("resting_hr", &points, day(0)).unwrap();
        assert_eq!(mark.usual_days, 90);
        let (low, high) = (mark.usual_low.unwrap(), mark.usual_high.unwrap());
        assert!((50.0..=52.0).contains(&low), "low = {low}");
        assert!((57.0..=59.0).contains(&high), "high = {high}");
        let sparse = metric_baseline("hrv", &series(28, 50.0), day(0)).unwrap();
        assert!(sparse.usual_low.is_some(), "28 days are enough for the norm band");
        assert!(sparse.long_term.is_none(), "history of less than one year is not a long-term norm");
    }

    /// The long-term norm uses all history, not the latest day, with a median per year. Enough readings but less than one year of span: not given.
    #[test]
    fn the_long_term_norm_spans_every_year_and_never_counts_today() {
        use super::super::anomaly::long_term_norm;
        // Three years: the first year near 60, the next two near 50 (every other day), and today an extreme 200.
        let mut points: Vec<(NaiveDate, f64)> = (1..=1095)
            .step_by(2)
            .map(|i| (day(-i), if i > 730 { 60.0 } else { 50.0 } + (i % 3) as f64))
            .collect();
        points.push((day(0), 200.0));
        let mark = metric_baseline("resting_hr", &points, day(0)).unwrap();
        let norm = mark.long_term.expect("three years is enough");
        assert_eq!(norm.days, 548);
        assert!(
            norm.high < 70.0,
            "today's 200 must not stretch the norm: {}",
            norm.high
        );
        assert_eq!(norm.years.first().map(|y| y.year), Some(2023));
        assert_eq!(norm.years.last().map(|y| y.year), Some(2026));
        let early = norm.years.iter().find(|y| y.year == 2024).unwrap();
        assert!(
            early.median > 55.0,
            "2024 starts in the 60s: {}",
            early.median
        );

        let dense_but_short: Vec<(NaiveDate, f64)> = (1..=300).map(|i| (day(-i), 50.0)).collect();
        assert!(
            long_term_norm(&dense_but_short, day(0)).is_none(),
            "300 days is still less than one year"
        );
    }
}
